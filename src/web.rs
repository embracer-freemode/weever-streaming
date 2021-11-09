use crate::{
    cli,
    publisher,
    subscriber,
    helper::catch,
    state::{SharedState, SHARED_STATE},
};
use anyhow::{Result, Context, bail};
use log::{debug, info, error};
use serde::{Deserialize, Serialize};
use actix_web::{
    post, get, web,
    App, HttpServer, Responder,
    http::{
        StatusCode,
        header,
    }
};
use actix_web_httpauth::extractors::bearer::BearerAuth;
use actix_files::Files;
use rustls::server::ServerConfig;
use rustls_pemfile::{certs, pkcs8_private_keys};


/// Web server for communicating with web clients
#[actix_web::main]
pub async fn web_main(cli: cli::CliOptions) -> Result<()> {
    // load ssl keys
    let raw_certs = &mut std::io::BufReader::new(
        std::fs::File::open(&cli.cert_file).context("can't read SSL cert file")?
    );
    let raw_keys = &mut std::io::BufReader::new(
        std::fs::File::open(&cli.key_file).context("can't read SSL key file")?
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
    let conn = redis_client.get_multiplexed_tokio_connection().await.context("can't get multiplexed Redis client")?;
    SHARED_STATE.set_redis(conn);

    // NATS
    info!("connecting NATS");
    let nats = nats::asynk::connect(&cli.nats).await.context("can't connect to NATS")?;
    SHARED_STATE.set_nats(nats);
    // run task for listening upcoming commands
    SHARED_STATE.listen_on_commands().await?;

    let url = format!("{}:{}", cli.host, cli.port);
    HttpServer::new(move || {
            let data = web::Data::new(cli.clone());
            App::new()
                // enable logger
                .wrap(actix_web::middleware::Logger::default())
                .app_data(data)
                .service(Files::new("/demo", "site").prefer_utf8(true))   // demo site
                .service(create_pub)
                .service(create_sub)
                .service(publish)
                .service(subscribe)
                .service(list_pub)
                .service(list_sub)
        })
        .bind_rustls(url, config)?
        .run()
        .await
        .context("actix web server error")
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


/// API for creating publisher
#[post("/create/pub")]
async fn create_pub(params: web::Json<CreatePubParams>) -> impl Responder {
    info!("create pub: {:?}", params);

    if params.id.is_empty() {
        return "id should not be empty";
    }

    if !params.id.chars().all(|c| c.is_ascii_graphic()) {
        return "id should be ascii graphic";
    }

    if params.room.is_empty() {
        return "room should not be empty";
    }

    if !params.room.chars().all(|c| c.is_ascii_graphic()) {
        return "room should be ascii graphic";
    }

    if let Some(token) = params.token.clone() {
        catch(SHARED_STATE.set_pub_token(params.room.clone(), params.id.clone(), token)).await;
    }

    "pub set"
}


/// API for creating subscriber
#[post("/create/sub")]
async fn create_sub(params: web::Json<CreateSubParams>) -> impl Responder {
    info!("create sub: {:?}", params);

    if params.id.is_empty() {
        return "id should not be empty";
    }

    if !params.id.chars().all(|c| c.is_ascii_graphic()) {
        return "id should be ascii graphic";
    }

    if params.room.is_empty() {
        return "room should not be empty";
    }

    if !params.room.chars().all(|c| c.is_ascii_graphic()) {
        return "room should be ascii graphic";
    }

    if let Some(token) = params.token.clone() {
        catch(SHARED_STATE.set_sub_token(params.room.clone(), params.id.clone(), token)).await;
    }

    "sub set"
}


/// WebRTC WHIP compatible (sort of) endpoint for running publisher
#[post("/pub/{room}/{id}")]
async fn publish(auth: BearerAuth,
                 cli: web::Data<cli::CliOptions>,
                 path: web::Path<(String, String)>,
                 sdp: web::Bytes) -> impl Responder {
    let (room, id) = path.into_inner();

    if id.is_empty() {
        return "id should not be empty".to_string().with_status(StatusCode::BAD_REQUEST);
    }

    if !id.chars().all(|c| c.is_ascii_graphic()) {
        return "id should be ascii graphic".to_string().with_status(StatusCode::BAD_REQUEST);
    }

    if room.is_empty() {
        return "room should not be empty".to_string().with_status(StatusCode::BAD_REQUEST);
    }

    if !room.chars().all(|c| c.is_ascii_graphic()) {
        return "room should be ascii graphic".to_string().with_status(StatusCode::BAD_REQUEST);
    }

    // TODO: verify "Content-Type: application/sdp"

    // disable auth for easier integration for now
    // // token verification
    // let token = SHARED_STATE.get_pub_token(&room, &id).await;
    // if let Ok(token) = token {
    //     if token != auth.token() {
    //         return "bad token".to_string().with_status(StatusCode::UNAUTHORIZED);
    //     }
    // } else {
    //     return "bad token".to_string().with_status(StatusCode::BAD_REQUEST);
    // }

    // check if there is another publisher in the room with same id
    match SHARED_STATE.exist_publisher(&room, &id).await {
        Ok(true) => return "duplicate publisher".to_string().with_status(StatusCode::BAD_REQUEST),
        Err(_) => return "publisher check error".to_string().with_status(StatusCode::BAD_REQUEST),
        _ => {},
    }

    let sdp = match String::from_utf8(sdp.to_vec()) {
        Ok(s) => s,
        Err(e) => {
            error!("SDP parsed error: {}", e);
            return "bad SDP".to_string().with_status(StatusCode::BAD_REQUEST);
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
            return "time error".to_string().with_status(StatusCode::INTERNAL_SERVER_ERROR);
        },
    }.as_micros();
    let tid = now.wrapping_div(10000) as u16;

    tokio::spawn(catch(publisher::webrtc_to_nats(cli.get_ref().clone(), room.clone(), id.clone(), sdp, tx, tid)));
    // TODO: timeout
    let sdp_answer = match rx.await {
        Ok(s) => s,
        Err(e) => {
            error!("SDP answer get error: {}", e);
            return "SDP answer generation error".to_string().with_status(StatusCode::BAD_REQUEST);
        }
    };
    debug!("SDP answer: {:.20}", sdp_answer);
    sdp_answer
        .with_status(StatusCode::CREATED)       // 201
        .with_header((header::CONTENT_TYPE, "application/sdp"))
        .with_header((header::LOCATION, ""))    // TODO: what's the need?
}

/// API for running subscriber
#[post("/sub/{room}/{id}")]
async fn subscribe(auth: BearerAuth,
                   cli: web::Data<cli::CliOptions>,
                   path: web::Path<(String, String)>,
                   sdp: web::Bytes) -> impl Responder {
    let (room, id) = path.into_inner();

    if id.is_empty() {
        return "id should not be empty".to_string().with_status(StatusCode::BAD_REQUEST);
    }

    if !id.chars().all(|c| c.is_ascii_graphic()) {
        return "id should be ascii graphic".to_string().with_status(StatusCode::BAD_REQUEST);
    }

    if room.is_empty() {
        return "room should not be empty".to_string().with_status(StatusCode::BAD_REQUEST);
    }

    if !room.chars().all(|c| c.is_ascii_graphic()) {
        return "room should be ascii graphic".to_string().with_status(StatusCode::BAD_REQUEST);
    }

    // TODO: verify "Content-Type: application/sdp"

    // disable auth for easier integration for now
    // // token verification
    // let token = SHARED_STATE.get_sub_token(&room, &id).await;
    // if let Ok(token) = token {
    //     if token != auth.token() {
    //         return "bad token".to_string().with_status(StatusCode::UNAUTHORIZED);
    //     }
    // } else {
    //     return "bad token".to_string().with_status(StatusCode::BAD_REQUEST);
    // }

    // check if there is another publisher in the room with same id
    match SHARED_STATE.exist_subscriber(&room, &id).await {
        Ok(true) => return "duplicate subscriber".to_string().with_status(StatusCode::BAD_REQUEST),
        Err(_) => return "subscriber check error".to_string().with_status(StatusCode::BAD_REQUEST),
        _ => {},
    }

    let sdp = match String::from_utf8(sdp.to_vec()) {
        Ok(s) => s,
        Err(e) => {
            error!("SDP parsed error: {}", e);
            return "bad SDP".to_string().with_status(StatusCode::BAD_REQUEST);
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
            return "time error".to_string().with_status(StatusCode::INTERNAL_SERVER_ERROR);
        },
    }.as_micros();
    let tid = now.wrapping_div(10000) as u16;

    tokio::spawn(catch(subscriber::nats_to_webrtc(cli.get_ref().clone(), room.clone(), id.clone(), sdp, tx, tid)));
    // TODO: timeout for safety
    let sdp_answer = match rx.await {
        Ok(s) => s,
        Err(e) => {
            error!("SDP answer get error: {}", e);
            return "SDP answer generation error".to_string().with_status(StatusCode::BAD_REQUEST);
        }
    };
    debug!("SDP answer: {:.20}", sdp_answer);
    sdp_answer
        .with_status(StatusCode::CREATED)   // 201
        .with_header((header::CONTENT_TYPE, "application/sdp"))
}

/// List publishers in specific room
#[get("/list/pub/{room}")]
async fn list_pub(path: web::Path<String>) -> impl Responder {
    let room = path.into_inner();

    if room.is_empty() {
        return "room should not be empty".to_string().with_status(StatusCode::BAD_REQUEST);
    }

    if !room.chars().all(|c| c.is_ascii_graphic()) {
        return "room should be ascii graphic".to_string().with_status(StatusCode::BAD_REQUEST);
    }

    // TODO: auth? we check nothing for now

    info!("listing publishers for room {}", room);

    SHARED_STATE.list_publishers(&room).await.unwrap_or_default()
        .into_iter()
        .reduce(|s, p| s + "," + &p)
        .unwrap_or_default()
        .with_status(StatusCode::OK)
}

/// List subscribers in specific room
#[get("/list/sub/{room}")]
async fn list_sub(path: web::Path<String>) -> impl Responder {
    let room = path.into_inner();

    if room.is_empty() {
        return "room should not be empty".to_string().with_status(StatusCode::BAD_REQUEST);
    }

    if !room.chars().all(|c| c.is_ascii_graphic()) {
        return "room should be ascii graphic".to_string().with_status(StatusCode::BAD_REQUEST);
    }

    // TODO: auth? we check nothing for now

    info!("listing subscribers for room {}", room);

    SHARED_STATE.list_subscribers(&room).await.unwrap_or_default()
        .into_iter()
        .reduce(|s, p| s + "," + &p)
        .unwrap_or_default()
        .with_status(StatusCode::OK)
}