//! Web endpoint for setup WebRTC connection
use crate::{
    cli,
    helper::catch,
    publisher,
    state::{SharedState, SHARED_STATE},
    subscriber,
};
use actix_cors::Cors;
use actix_files::Files;
use actix_web::{
    dev::ServerHandle,
    get,
    http::{header, StatusCode},
    post, web, App, HttpServer, Responder,
};
use actix_web_httpauth::extractors::bearer::BearerAuth;
use anyhow::{anyhow, bail, Context, Result};
use log::{debug, error, info};
use once_cell::sync::OnceCell;
use prometheus::{GaugeVec, Opts, Registry, TextEncoder};
use rustls::server::ServerConfig;
use rustls_pemfile::{certs, pkcs8_private_keys};
use serde::{Deserialize, Serialize};

/// publich API server handle
pub static PUBLIC_SERVER: OnceCell<ServerHandle> = OnceCell::new();
/// private API server handle
pub static PRIVATE_SERVER: OnceCell<ServerHandle> = OnceCell::new();
/// global flag for checking if the instance is going to stop
pub static IS_STOPPING: OnceCell<bool> = OnceCell::new();

/// Web server for communicating with web clients
#[tokio::main]
pub async fn web_main(cli: cli::CliOptions) -> Result<()> {
    // load ssl keys
    let raw_certs = &mut std::io::BufReader::new(
        std::fs::File::open(&cli.cert_file).context("can't read SSL cert file")?,
    );
    let raw_keys = &mut std::io::BufReader::new(
        std::fs::File::open(&cli.key_file).context("can't read SSL key file")?,
    );
    let cert_chain = certs(raw_certs)
        .context("cert parse error")?
        .iter()
        .map(|v| rustls::Certificate(v.to_vec()))
        .collect();
    let mut keys = pkcs8_private_keys(raw_keys).context("ssl key parse error")?;
    if keys.is_empty() {
        bail!("SSL key (PKCS) is emtpy");
    }
    let key = rustls::PrivateKey(keys.remove(0));
    let config = ServerConfig::builder()
        .with_safe_defaults()
        .with_no_client_auth()
        .with_single_cert(cert_chain, key)
        .context("cert setup failed")?;

    // Redis
    let redis_client = redis::Client::open(cli.redis.clone()).context("can't connect to Redis")?;
    let conn = redis_client
        .get_multiplexed_tokio_connection()
        .await
        .context("can't get multiplexed Redis client")?;
    SHARED_STATE.set_redis(conn)?;

    // NATS
    info!("connecting NATS");
    let nats = nats::asynk::connect(&cli.nats)
        .await
        .context("can't connect to NATS")?;
    SHARED_STATE.set_nats(nats)?;
    // run task for listening upcoming commands
    SHARED_STATE.listen_on_commands().await?;

    let url = format!("{}:{}", cli.host, cli.port);
    let private_url = format!("{}:{}", cli.host, cli.private_port);
    info!("public URL {url}");
    info!("private URL {private_url}");
    let public_server = HttpServer::new(move || {
        let data = web::Data::new(cli.clone());
        let cors_domain = format!("//{}", cli.cors_domain);
        // set CORS for easier frontend development
        let cors = Cors::default()
            .allowed_origin_fn(move |origin, _req_head| {
                let domain = origin
                    .to_str()
                    .unwrap_or("")
                    .splitn(3, ':')
                    .skip(1)
                    .take(1)
                    .next()
                    .unwrap_or("");
                // allow any localhost for frontend local development
                // e.g. "http://localhost" or "https://localhost" or "https://localhost:3000"
                //
                // and a allow any host with explicit set cors_domain
                matches!(domain, "//localhost") || matches!(domain, _ if domain == cors_domain)
            })
            .allowed_methods(vec!["GET", "POST"])
            .allowed_headers(vec![header::AUTHORIZATION, header::ACCEPT])
            .allowed_header(header::CONTENT_TYPE);

        let mut app = App::new()
            // enable logger
            .wrap(actix_web::middleware::Logger::default())
            .wrap(cors)
            .app_data(data)
            .service(publish)
            .service(subscribe);

        // if the debug option is enabled, we show the demo site
        if cli.debug {
            app = app
                .service(Files::new("/", "site").index_file("index.html").prefer_utf8(true)) // demo site
                .service(create_pub)
                .service(create_sub)
                .service(list_pub)
                .service(list_sub);
        }

        app
    })
    .bind_rustls(url, config)?
    .system_exit()
    .shutdown_timeout(10)
    .run();

    let private_server = HttpServer::new(move || {
        App::new()
            .wrap(actix_web::middleware::Logger::default())
            .service(liveness)
            .service(readiness)
            .service(prestop)
            .service(metrics)
            .service(create_pub)
            .service(create_sub)
            .service(list_pub)
            .service(list_sub)
    })
    .bind(private_url)?
    .system_exit()
    .shutdown_timeout(10)
    .run();

    PUBLIC_SERVER
        .set(public_server.handle())
        .map_err(|_| anyhow!("set public web server handle failed"))?;
    PRIVATE_SERVER
        .set(private_server.handle())
        .map_err(|_| anyhow!("set private web server handle failed"))?;

    let (result1, result2) = tokio::join!(public_server, private_server);
    result1.context("actix web public server error")?;
    result2.context("actix web private server error")
}

/// Parameters for creating publisher auth token
#[derive(Debug, Serialize, Deserialize)]
struct CreatePubParams {
    room: String,
    id: String,
    token: Option<String>,
}

/// Parameters for creating subscriber auth token
#[derive(Debug, Serialize, Deserialize)]
struct CreateSubParams {
    room: String,
    id: String,
    token: Option<String>,
}

/// Register publisher token
#[post("/create/pub")]
async fn create_pub(params: web::Json<CreatePubParams>) -> impl Responder {
    info!("create pub: {:?}", params);

    if params.id.is_empty() {
        return "id should not be empty";
    }

    if !params.id.chars().all(|c| c.is_ascii_graphic()) {
        return "id should be ascii graphic";
    }

    // "." will conflict with NATS subject seperator
    if params.id.contains('.') {
        return "id should not contain '.'";
    }

    if params.room.is_empty() {
        return "room should not be empty";
    }

    if !params.room.chars().all(|c| c.is_ascii_graphic()) {
        return "room should be ascii graphic";
    }

    // "." will conflict with NATS subject seperator
    if params.room.contains('.') {
        return "room should not contain '.'";
    }

    if let Some(token) = params.token.clone() {
        catch(SHARED_STATE.set_pub_token(params.room.clone(), params.id.clone(), token)).await;
    }

    "pub set"
}

/// Register subscriber token
#[post("/create/sub")]
async fn create_sub(params: web::Json<CreateSubParams>) -> impl Responder {
    info!("create sub: {:?}", params);

    if params.id.is_empty() {
        return "id should not be empty";
    }

    if !params.id.chars().all(|c| c.is_ascii_graphic()) {
        return "id should be ascii graphic";
    }

    // "." will conflict with NATS subject seperator
    if params.id.contains('.') {
        return "id should not contain '.'";
    }

    if params.room.is_empty() {
        return "room should not be empty";
    }

    if !params.room.chars().all(|c| c.is_ascii_graphic()) {
        return "room should be ascii graphic";
    }

    // "." will conflict with NATS subject seperator
    if params.room.contains('.') {
        return "room should not contain '.'";
    }

    if let Some(token) = params.token.clone() {
        catch(SHARED_STATE.set_sub_token(params.room.clone(), params.id.clone(), token)).await;
    }

    "sub set"
}

/// WebRTC WHIP compatible (sort of) endpoint for publisher connection
#[post("/pub/{room}/{id}")]
async fn publish(
    auth: BearerAuth,
    cli: web::Data<cli::CliOptions>,
    path: web::Path<(String, String)>,
    sdp: web::Bytes,
) -> impl Responder {
    let (room, id) = path.into_inner();

    if id.is_empty() {
        return "id should not be empty"
            .to_string()
            .customize()
            .with_status(StatusCode::BAD_REQUEST);
    }

    if !id.chars().all(|c| c.is_ascii_graphic()) {
        return "id should be ascii graphic"
            .to_string()
            .customize()
            .with_status(StatusCode::BAD_REQUEST);
    }

    // "." will conflict with NATS subject seperator
    if id.contains('.') {
        return "id should not contain '.'"
            .to_string()
            .customize()
            .with_status(StatusCode::BAD_REQUEST);
    }

    if room.is_empty() {
        return "room should not be empty"
            .to_string()
            .customize()
            .with_status(StatusCode::BAD_REQUEST);
    }

    if !room.chars().all(|c| c.is_ascii_graphic()) {
        return "room should be ascii graphic"
            .to_string()
            .customize()
            .with_status(StatusCode::BAD_REQUEST);
    }

    // "." will conflict with NATS subject seperator
    if room.contains('.') {
        return "room should not contain '.'"
            .to_string()
            .customize()
            .with_status(StatusCode::BAD_REQUEST);
    }

    // TODO: verify "Content-Type: application/sdp"

    // token verification
    if cli.auth {
        let token = SHARED_STATE.get_pub_token(&room, &id).await;
        if let Ok(token) = token {
            if token != auth.token() {
                return "bad token"
                    .to_string()
                    .customize()
                    .with_status(StatusCode::UNAUTHORIZED);
            }
        } else {
            return "bad token"
                .to_string()
                .customize()
                .with_status(StatusCode::BAD_REQUEST);
        }
    }

    // check if there is another publisher in the room with same id
    match SHARED_STATE.exist_publisher(&room, &id).await {
        Ok(true) => {
            return "duplicate publisher"
                .to_string()
                .customize()
                .with_status(StatusCode::BAD_REQUEST)
        }
        Err(_) => {
            return "publisher check error"
                .to_string()
                .customize()
                .with_status(StatusCode::BAD_REQUEST)
        }
        _ => {}
    }

    let sdp = match String::from_utf8(sdp.to_vec()) {
        Ok(s) => s,
        Err(e) => {
            error!("SDP parsed error: {}", e);
            return "bad SDP"
                .to_string()
                .customize()
                .with_status(StatusCode::BAD_REQUEST);
        }
    };
    debug!("pub: auth {} sdp {:.20?}", auth.token(), sdp);
    let (tx, rx) = tokio::sync::oneshot::channel();

    // get a time based id to represent following Tokio task for this user
    // if user call it again later
    // we will be able to identify in logs
    let now = match std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH) {
        Ok(d) => d,
        Err(e) => {
            error!("system time error: {}", e);
            return "time error"
                .to_string()
                .customize()
                .with_status(StatusCode::INTERNAL_SERVER_ERROR);
        }
    }
    .as_micros();
    let tid = now.wrapping_div(10000) as u16;

    tokio::spawn(catch(publisher::webrtc_to_nats(
        cli.get_ref().clone(),
        room.clone(),
        id.clone(),
        sdp,
        tx,
        tid,
    )));
    // TODO: timeout
    let sdp_answer = match rx.await {
        Ok(s) => s,
        Err(e) => {
            error!("SDP answer get error: {}", e);
            return "SDP answer generation error"
                .to_string()
                .customize()
                .with_status(StatusCode::BAD_REQUEST);
        }
    };
    debug!("SDP answer: {:.20}", sdp_answer);
    sdp_answer
        .customize()
        .with_status(StatusCode::CREATED) // 201
        .insert_header((header::CONTENT_TYPE, "application/sdp"))
        .insert_header((header::LOCATION, "")) // TODO: what's the need?
}

/// WebRTC WHEP compatible (sort of) endpoint for subscriber connection
#[post("/sub/{room}/{id}")]
async fn subscribe(
    auth: BearerAuth,
    cli: web::Data<cli::CliOptions>,
    path: web::Path<(String, String)>,
    sdp: web::Bytes,
) -> impl Responder {
    let (room, id) = path.into_inner();

    if id.is_empty() {
        return "id should not be empty"
            .to_string()
            .customize()
            .with_status(StatusCode::BAD_REQUEST);
    }

    if !id.chars().all(|c| c.is_ascii_graphic()) {
        return "id should be ascii graphic"
            .to_string()
            .customize()
            .with_status(StatusCode::BAD_REQUEST);
    }

    // "." will conflict with NATS subject seperator
    if id.contains('.') {
        return "id should not contain '.'"
            .to_string()
            .customize()
            .with_status(StatusCode::BAD_REQUEST);
    }

    if room.is_empty() {
        return "room should not be empty"
            .to_string()
            .customize()
            .with_status(StatusCode::BAD_REQUEST);
    }

    if !room.chars().all(|c| c.is_ascii_graphic()) {
        return "room should be ascii graphic"
            .to_string()
            .customize()
            .with_status(StatusCode::BAD_REQUEST);
    }

    // "." will conflict with NATS subject seperator
    if room.contains('.') {
        return "room should not contain '.'"
            .to_string()
            .customize()
            .with_status(StatusCode::BAD_REQUEST);
    }

    // TODO: verify "Content-Type: application/sdp"

    // token verification
    if cli.auth {
        let token = SHARED_STATE.get_sub_token(&room, &id).await;
        if let Ok(token) = token {
            if token != auth.token() {
                return "bad token"
                    .to_string()
                    .customize()
                    .with_status(StatusCode::UNAUTHORIZED);
            }
        } else {
            return "bad token"
                .to_string()
                .customize()
                .with_status(StatusCode::BAD_REQUEST);
        }
    }

    // check if there is another publisher in the room with same id
    match SHARED_STATE.exist_subscriber(&room, &id).await {
        Ok(true) => {
            return "duplicate subscriber"
                .to_string()
                .customize()
                .with_status(StatusCode::BAD_REQUEST)
        }
        Err(_) => {
            return "subscriber check error"
                .to_string()
                .customize()
                .with_status(StatusCode::BAD_REQUEST)
        }
        _ => {}
    }

    let sdp = match String::from_utf8(sdp.to_vec()) {
        Ok(s) => s,
        Err(e) => {
            error!("SDP parsed error: {}", e);
            return "bad SDP"
                .to_string()
                .customize()
                .with_status(StatusCode::BAD_REQUEST);
        }
    };
    debug!("sub: auth {} sdp {:.20?}", auth.token(), sdp);
    let (tx, rx) = tokio::sync::oneshot::channel();

    // get a time based id to represent following Tokio task for this user
    // if user call it again later
    // we will be able to identify in logs
    let now = match std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH) {
        Ok(d) => d,
        Err(e) => {
            error!("system time error: {}", e);
            return "time error"
                .to_string()
                .customize()
                .with_status(StatusCode::INTERNAL_SERVER_ERROR);
        }
    }
    .as_micros();
    let tid = now.wrapping_div(10000) as u16;

    tokio::spawn(catch(subscriber::nats_to_webrtc(
        cli.get_ref().clone(),
        room.clone(),
        id.clone(),
        sdp,
        tx,
        tid,
    )));
    // TODO: timeout for safety
    let sdp_answer = match rx.await {
        Ok(s) => s,
        Err(e) => {
            error!("SDP answer get error: {}", e);
            return "SDP answer generation error"
                .to_string()
                .customize()
                .with_status(StatusCode::BAD_REQUEST);
        }
    };
    debug!("SDP answer: {:.20}", sdp_answer);
    sdp_answer
        .customize()
        .with_status(StatusCode::CREATED) // 201
        .insert_header((header::CONTENT_TYPE, "application/sdp"))
}

/// List publishers in specific room
#[get("/list/pub/{room}")]
async fn list_pub(path: web::Path<String>) -> impl Responder {
    let room = path.into_inner();

    if room.is_empty() {
        return "room should not be empty"
            .to_string()
            .customize()
            .with_status(StatusCode::BAD_REQUEST);
    }

    if !room.chars().all(|c| c.is_ascii_graphic()) {
        return "room should be ascii graphic"
            .to_string()
            .customize()
            .with_status(StatusCode::BAD_REQUEST);
    }

    info!("listing publishers for room {}", room);

    SHARED_STATE
        .list_publishers(&room)
        .await
        .unwrap_or_default()
        .into_iter()
        .reduce(|s, p| s + "," + &p)
        .unwrap_or_default()
        .customize()
        .with_status(StatusCode::OK)
}

/// List subscribers in specific room
#[get("/list/sub/{room}")]
async fn list_sub(path: web::Path<String>) -> impl Responder {
    let room = path.into_inner();

    if room.is_empty() {
        return "room should not be empty"
            .to_string()
            .customize()
            .with_status(StatusCode::BAD_REQUEST);
    }

    if !room.chars().all(|c| c.is_ascii_graphic()) {
        return "room should be ascii graphic"
            .to_string()
            .customize()
            .with_status(StatusCode::BAD_REQUEST);
    }

    info!("listing subscribers for room {}", room);

    SHARED_STATE
        .list_subscribers(&room)
        .await
        .unwrap_or_default()
        .into_iter()
        .reduce(|s, p| s + "," + &p)
        .unwrap_or_default()
        .customize()
        .with_status(StatusCode::OK)
}

/// Kubernetes liveness probe
#[get("/liveness")]
async fn liveness() -> impl Responder {
    "OK"
}

/// Kubernetes readiness probe
/// We won't route new peers in when we are closing.
#[get("/readiness")]
async fn readiness() -> impl Responder {
    // TODO: also check if we have too much peers
    match IS_STOPPING.get() {
        Some(true) => "BUSY"
            .customize()
            .with_status(StatusCode::SERVICE_UNAVAILABLE),
        _ => "OK".customize().with_status(StatusCode::OK),
    }
}

/// [Kubernetes preStop hook](https://kubernetes.io/docs/concepts/containers/container-lifecycle-hooks/#container-hooks)
#[get("/preStop")]
async fn prestop() -> impl Responder {
    info!("stopping system");

    // We won't route new peers in when we are closing.
    let _ = IS_STOPPING.set(true);

    // wait for existing peers, so we won't break existing streams
    while SHARED_STATE.has_peers().await {
        tokio::time::sleep(std::time::Duration::from_secs(3)).await;
        info!("still have peers");
    }

    // stop the servers
    if let Some(handle) = PUBLIC_SERVER.get() {
        // spawn task to run the stop, so we won't block current request
        // (and the stop wait for this request to finish)
        tokio::spawn(handle.stop(true));
    }
    if let Some(handle) = PRIVATE_SERVER.get() {
        // spawn task to run the stop, so we won't block current request
        // (and the stop wait for this request to finish)
        tokio::spawn(handle.stop(true));
    }

    "OK"
}

/// Prometheus metrics
#[get("/metrics")]
async fn metrics() -> impl Responder {
    let reg = Registry::new();

    // we will pass POD_NAME via Kubernetes setup
    let pod = std::env::var("POD_NAME").unwrap_or_default();

    // sfu_pod_peer_count
    let gauge_vec = GaugeVec::new(
        Opts::new(
            "sfu_pod_peer_count",
            "publishers and subscribers count in current pod (by room, by type)",
        ),
        &["pod", "room", "type"],
    )
    .unwrap();
    for (name, room) in SHARED_STATE.read().unwrap().rooms.iter() {
        gauge_vec
            .with_label_values(&[&pod, name, "pub"])
            .set(room.pubs.len() as f64);
        gauge_vec
            .with_label_values(&[&pod, name, "sub"])
            .set(room.subs.len() as f64);
    }
    reg.register(Box::new(gauge_vec)).unwrap();

    let encoder = TextEncoder::new();
    let metric_families = reg.gather();
    encoder.encode_to_string(&metric_families).unwrap()
}
