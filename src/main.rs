//! WebRTC SFU with horizontal scale design

use anyhow::Result;
use log::{info, warn, error};
use interceptor::registry::Registry;
use rtcp::payload_feedbacks::picture_loss_indication::PictureLossIndication;
use webrtc::api::interceptor_registry::register_default_interceptors;
use webrtc::api::media_engine::{MediaEngine, MIME_TYPE_OPUS, MIME_TYPE_VP8};
use webrtc::api::APIBuilder;
use webrtc::media::rtp::rtp_codec::{RTCRtpCodecCapability, RTCRtpCodecParameters, RTPCodecType};
use webrtc::media::rtp::rtp_receiver::RTCRtpReceiver;
use webrtc::media::track::track_remote::TrackRemote;
use webrtc::media::track::track_local::track_local_static_rtp::TrackLocalStaticRTP;
use webrtc::media::track::track_local::{TrackLocal, TrackLocalWriter};
use webrtc::peer::configuration::RTCConfiguration;
use webrtc::peer::ice::ice_connection_state::RTCIceConnectionState;
use webrtc::peer::ice::ice_server::RTCIceServer;
use webrtc::peer::peer_connection_state::RTCPeerConnectionState;
use webrtc::peer::sdp::session_description::RTCSessionDescription;
use webrtc::peer::sdp::sdp_type::RTCSdpType;
use webrtc::data::data_channel::data_channel_message::DataChannelMessage;
use webrtc::data::data_channel::RTCDataChannel;
use std::sync::Arc;
use std::collections::HashMap;
use tokio::time::Duration;
use tokio::sync::oneshot::Sender;
use actix_web::{get, post, web, App, HttpServer, Responder, HttpResponse};
use actix_web_httpauth::extractors::bearer::BearerAuth;
// use actix_cors::Cors;
use actix_files::Files;
use serde::{Deserialize, Serialize};
use once_cell::sync::Lazy;
use std::sync::Mutex;
use std::sync::RwLock;


fn main() -> Result<()> {
    // TODO: use tracking crate with more span info
    env_logger::init();
    web_main()?;
    Ok(())
}


#[derive(Debug, Default)]
struct State {
    rooms: HashMap<String, Room>,
}

#[derive(Debug, Default)]
struct Room {
    name: String,
    /// (user, track id) -> mime type
    user_track_to_mime: HashMap<(String, String), String>,
    sub_peers: HashMap<String, PeerConnetionInfo>,
    /// user -> token
    pub_tokens: HashMap<String, String>,
    sub_token: String,
}

#[derive(Debug, Default)]
struct PeerConnetionInfo {
    name: String,
    // pc: Option<RTCPeerConnection>,
    notify_renegotiation: Arc<Notify>,
    // notify_close: ...,
}

static HACK_STATE: Lazy<Mutex<State>> = Lazy::new(|| Default::default());



/// Extract RTP streams from WebRTC, and send it to NATS
///
/// based on [rtp-forwarder](https://github.com/webrtc-rs/webrtc/tree/master/examples/rtp-forwarder) example
async fn webrtc_to_nats(room: String, user: String, offer: String, answer_tx: Sender<String>, subject: String) -> Result<()> {
    // NATS
    // TODO: share NATS connection
    info!("connecting NATS");
    let nc = nats::asynk::connect("localhost").await?;

    // Create a MediaEngine object to configure the supported codec
    info!("creating MediaEngine");
    let mut m = MediaEngine::default();

    // Setup the codecs you want to use.
    // We'll use a VP8 and Opus but you can also define your own
    m.register_codec(
        RTCRtpCodecParameters {
            capability: RTCRtpCodecCapability {
                mime_type: MIME_TYPE_VP8.to_owned(),
                clock_rate: 90000,
                channels: 0,
                sdp_fmtp_line: "".to_owned(),
                rtcp_feedback: vec![],
            },
            payload_type: 96,
            ..Default::default()
        },
        RTPCodecType::Video,
    )?;

    m.register_codec(
        RTCRtpCodecParameters {
            capability: RTCRtpCodecCapability {
                mime_type: MIME_TYPE_OPUS.to_owned(),
                clock_rate: 48000,
                channels: 2,
                sdp_fmtp_line: "".to_owned(),
                rtcp_feedback: vec![],
            },
            payload_type: 111,
            ..Default::default()
        },
        RTPCodecType::Audio,
    )?;

    // Create a InterceptorRegistry. This is the user configurable RTP/RTCP Pipeline.
    // This provides NACKs, RTCP Reports and other features. If you use `webrtc.NewPeerConnection`
    // this is enabled by default. If you are manually managing You MUST create a InterceptorRegistry
    // for each PeerConnection.
    let mut registry = Registry::new();

    // Use the default set of Interceptors
    registry = register_default_interceptors(registry, &mut m)?;

    // Create the API object with the MediaEngine
    let api = APIBuilder::new()
        .with_media_engine(m)
        .with_interceptor_registry(registry)
        .build();

    // Prepare the configuration
    info!("preparing RTCConfiguration");
    let config = RTCConfiguration {
        ice_servers: vec![RTCIceServer {
            urls: vec!["stun:stun.l.google.com:19302".to_owned()],
            ..Default::default()
        }],
        ..Default::default()
    };

    // Create a new RTCPeerConnection
    let peer_connection = Arc::new(api.new_peer_connection(config).await?);

    // TODO: more efficent way?
    // TODO: limitation for safety?
    //
    // Allow us to receive N video/audio tracks based on SDP Offer
    // sample of SDP line: "m=video 9 UDP/TLS/RTP/SAVPF 96"
    for sdp_media in offer.rsplit_terminator("m=") {
        let msid = sdp_media.rsplit_once("msid:");
        if msid.is_none() {
            continue;
        }
        let (_, msid) = msid.unwrap();
        let id = msid.split_whitespace().take(2).skip(1).next();
        if id.is_none() {
            continue;
        }
        let id = id.unwrap();

        // TODO: global cache for media count
        if sdp_media.starts_with("video") {
            // FIXME: use better way
            // TODO: add id in
            {
                let room_info = HACK_STATE.lock();
                let mut room_info = room_info.unwrap();
                let room_info = room_info.rooms.entry(room.clone()).or_default();
                room_info.user_track_to_mime.insert((user.clone(), id.to_string()), "video".to_string());
            }
            peer_connection
                .add_transceiver_from_kind(RTPCodecType::Video, &[])
                .await?;
        } else if sdp_media.starts_with("audio") {
            // FIXME: use better way
            // TODO: add id in
            {
                let room_info = HACK_STATE.lock();
                let mut room_info = room_info.unwrap();
                let room_info = room_info.rooms.entry(room.clone()).or_default();
                room_info.user_track_to_mime.insert((user.clone(), id.to_string()), "audio".to_string());
            }
            peer_connection
                .add_transceiver_from_kind(RTPCodecType::Audio, &[])
                .await?;
        }
    }

    // build SDP Offer type
    let mut sdp = RTCSessionDescription::default();
    sdp.sdp_type = RTCSdpType::Offer;
    sdp.sdp = offer;
    let offer = sdp;

    // Set a handler for when a new remote track starts, this handler will forward data to
    // our UDP listeners.
    // In your application this is where you would handle/process audio/video
    let pc = Arc::clone(&peer_connection);
    peer_connection
        .on_track(Box::new(
            move |track: Option<Arc<TrackRemote>>, _receiver: Option<Arc<RTCRtpReceiver>>| {
                if let Some(track) = track {
                    // Send a PLI on an interval so that the publisher is pushing a keyframe every rtcpPLIInterval
                    let media_ssrc = track.ssrc();
                    let pc2 = Arc::clone(&pc);
                    tokio::spawn(async move {
                        let mut result = Result::<usize>::Ok(0);
                        while result.is_ok() {
                            let timeout = tokio::time::sleep(Duration::from_secs(3));
                            tokio::pin!(timeout);

                            tokio::select! {
                                _ = timeout.as_mut() =>{
                                    result = pc2.write_rtcp(&PictureLossIndication{
                                            sender_ssrc: 0,
                                            media_ssrc,
                                    }).await;
                                }
                            };
                        }
                    });

                    let nc = nc.clone();
                    // push RTP to NATS
                    // use ID to disquish streams from same publisher
                    // TODO: can we use SSRC?
                    let subject = subject.clone(); // FIXME: avoid this
                    tokio::spawn(async move {
                        // FIXME: the id here generated from browser might be "{...}"
                        let subject = format!("{}.{}", subject, track.id().await);
                        info!("publish to {}", subject);
                        let mut b = vec![0u8; 1500];
                        while let Ok((n, _)) = track.read(&mut b).await {
                            nc.publish(&subject, &b[..n]).await?;
                        }
                        Result::<()>::Ok(())
                    });
                }

                Box::pin(async {})
            },
        ))
        .await;

    // Set the handler for ICE connection state
    // This will notify you when the peer has connected/disconnected
    peer_connection
        .on_ice_connection_state_change(Box::new(move |connection_state: RTCIceConnectionState| {
            info!("Connection State has changed {}", connection_state);
            // if connection_state == RTCIceConnectionState::Connected {
            // }
            Box::pin(async {})
        }))
        .await;

    // Set the handler for Peer connection state
    // This will notify you when the peer has connected/disconnected
    peer_connection
        .on_peer_connection_state_change(Box::new(move |s: RTCPeerConnectionState| {
            info!("Peer Connection State has changed: {}", s);

            if s == RTCPeerConnectionState::Failed {
                // Wait until PeerConnection has had no network activity for 30 seconds or another failure. It may be reconnected using an ICE Restart.
                // Use webrtc.PeerConnectionStateDisconnected if you are interested in detecting faster timeout.
                // Note that the PeerConnection may come back from PeerConnectionStateDisconnected.
                info!("Peer Connection has gone to failed exiting: Done forwarding");
                // TODO: cleanup?
                // std::process::exit(0);
            }

            // if s == RTCPeerConnectionState::Connected {
            //     info!("webrtc to nats connected!");
            // }

            Box::pin(async {})
        }))
        .await;

    // Set the remote SessionDescription
    info!("PC set remote SDP");
    peer_connection.set_remote_description(offer).await?;

    // Create an answer
    info!("PC create local SDP");
    let answer = peer_connection.create_answer(None).await?;

    // Create channel that is blocked until ICE Gathering is complete
    let mut gather_complete = peer_connection.gathering_complete_promise().await;

    // Sets the LocalDescription, and starts our UDP listeners
    peer_connection.set_local_description(answer).await?;

    // Block until ICE Gathering is complete, disabling trickle ICE
    // we do this because we only can exchange one signaling message
    // in a production application you should exchange ICE Candidates via OnICECandidate
    let _ = gather_complete.recv().await;

    // Send out the SDP answer via Sender
    if let Some(local_desc) = peer_connection.local_description().await {
        info!("PC send local SDP");
        answer_tx.send(local_desc.sdp);
    } else {
        warn!("generate local_description failed!");
    }

    // ask subscribers to renegotiation
    {
        let mut state = HACK_STATE.lock().unwrap();
        let room = state.rooms.get_mut(&room).unwrap();
        for (_, user) in room.sub_peers.iter() {
            user.notify_renegotiation.notify_one();
        }
    }

    // TODO: a signal to close connection?
    info!("PC wait");
    tokio::time::sleep(Duration::from_secs(3000)).await;
    peer_connection.close().await?;

    Ok(())
}

/// Pull RTP streams from NATS, and send it to WebRTC
///
// based on [rtp-to-webrtc](https://github.com/webrtc-rs/webrtc/tree/master/examples/rtp-to-webrtc)
async fn nats_to_webrtc(room: String, user: String, offer: String, answer_tx: Sender<String>, subject: String) -> Result<()> {
    // build SDP Offer type
    let mut sdp = RTCSessionDescription::default();
    sdp.sdp_type = RTCSdpType::Offer;
    sdp.sdp = offer;
    let offer = sdp;

    // NATS
    // TODO: share NATS connection
    let nc = nats::asynk::connect("localhost").await?;

    // Create a MediaEngine object to configure the supported codec
    let mut m = MediaEngine::default();

    // m.register_default_codecs()?;
    m.register_codec(
        RTCRtpCodecParameters {
            capability: RTCRtpCodecCapability {
                mime_type: MIME_TYPE_VP8.to_owned(),
                clock_rate: 90000,
                channels: 0,
                sdp_fmtp_line: "".to_owned(),
                rtcp_feedback: vec![],
            },
            payload_type: 96,
            ..Default::default()
        },
        RTPCodecType::Video,
    )?;

    m.register_codec(
        RTCRtpCodecParameters {
            capability: RTCRtpCodecCapability {
                mime_type: MIME_TYPE_OPUS.to_owned(),
                clock_rate: 48000,
                channels: 2,
                sdp_fmtp_line: "".to_owned(),
                rtcp_feedback: vec![],
            },
            payload_type: 111,
            ..Default::default()
        },
        RTPCodecType::Audio,
    )?;

    // Create a InterceptorRegistry. This is the user configurable RTP/RTCP Pipeline.
    // This provides NACKs, RTCP Reports and other features. If you use `webrtc.NewPeerConnection`
    // this is enabled by default. If you are manually managing You MUST create a InterceptorRegistry
    // for each PeerConnection.
    let mut registry = Registry::new();

    // Use the default set of Interceptors
    registry = register_default_interceptors(registry, &mut m)?;

    // Create the API object with the MediaEngine
    let api = APIBuilder::new()
        .with_media_engine(m)
        .with_interceptor_registry(registry)
        .build();

    // Prepare the configuration
    let config = RTCConfiguration {
        ice_servers: vec![RTCIceServer {
            urls: vec!["stun:stun.l.google.com:19302".to_owned()],
            ..Default::default()
        }],
        ..Default::default()
    };

    // Create a new RTCPeerConnection
    let peer_connection = Arc::new(api.new_peer_connection(config).await?);

    // Create Track that we send video back to browser on
    // TODO: dynamic creation
    // TODO: how to handle video/audio from same publisher and send to different track?
    // HACK_MEDIA.lock().unwrap().push("video".to_string());

    let mut tracks = Arc::new(RwLock::new(HashMap::new()));
    let media = HACK_STATE.lock().unwrap().rooms.get(&room).unwrap().user_track_to_mime.clone(); // TODO: avoid this?
    for ((user, track_id), mime) in media {
        let mime = match mime.as_ref() {
            "video" => MIME_TYPE_VP8,
            "audio" => MIME_TYPE_OPUS,
            _ => unreachable!(),
        };

        let track = Arc::new(TrackLocalStaticRTP::new(
            RTCRtpCodecCapability {
                mime_type: mime.to_owned(),
                ..Default::default()
            },
            track_id.to_string(),       // TODO: verify it, id to media id
            user.to_string(),           // TODO: verify it, msid to user
        ));

        // for later dyanmic RTP dispatch from NATS
        tracks.write().unwrap().insert((user, track_id), track.clone());

        let rtp_sender = peer_connection
            .add_track(Arc::clone(&track) as Arc<dyn TrackLocal + Send + Sync>)
            .await?;

        // Read incoming RTCP packets
        // Before these packets are returned they are processed by interceptors. For things
        // like NACK this needs to be called.
        tokio::spawn(async move {
            let mut rtcp_buf = vec![0u8; 1500];
            while let Ok((_, _)) = rtp_sender.read(&mut rtcp_buf).await {}
            Result::<()>::Ok(())
        });
    }

    // Set the handler for ICE connection state
    // This will notify you when the peer has connected/disconnected
    let pc = Arc::clone(&peer_connection);

    // set notify
    let notify_renegotiation = Arc::new(Notify::new());
    {
        let mut state = HACK_STATE.lock().unwrap();
        let room = state.rooms.get_mut(&room).unwrap();
        let user = room.sub_peers.entry(user).or_default();
        user.notify_renegotiation = notify_renegotiation.clone();
    }

    peer_connection
        .on_ice_connection_state_change(Box::new(move |connection_state: RTCIceConnectionState| {
            info!("Connection State has changed {}", connection_state);
            let pc2 = Arc::clone(&pc);
            Box::pin(async move {
                if connection_state == RTCIceConnectionState::Failed {
                    if let Err(err) = pc2.close().await {
                        info!("peer connection close err: {}", err);
                        // TODO: cleanup?
                        // std::process::exit(0);
                    }
                }
            })
        }))
        .await;

    // Register data channel creation handling
    let pc = Arc::clone(&peer_connection);  // TODO: avoid this?
    let tracks2 = Arc::clone(&tracks);       // TODO: avoid this?
    peer_connection
        .on_data_channel(Box::new(move |dc: Arc<RTCDataChannel>| {
            let notify_renegotiation = notify_renegotiation.clone();    // TODO: avoid this?
            let room = room.clone();    // TODO: avoid this?
            let pc = Arc::clone(&pc);  // TODO: avoid this?
            let pc2 = Arc::clone(&pc);  // TODO: avoid this?
            let tracks2 = Arc::clone(&tracks2);       // TODO: avoid this?

            let dc_label = dc.label().to_owned();

            // only accept data channel with label "control"
            if dc_label != "control" {
               return Box::pin(async {});
            }

            let dc_id = dc.id();
            info!("New DataChannel {} {}", dc_label, dc_id);

            // Register channel opening handling
            // let pc = pc.clone();
            Box::pin(async move {
                let dc2 = Arc::clone(&dc);
                let dc_label2 = dc_label.clone();
                let dc_id2 = dc_id.clone();
                let tracks2 = Arc::clone(&tracks2);       // TODO: avoid this?
                let pc = Arc::clone(&pc);  // TODO: avoid this?
                dc.on_open(Box::new(move || {
                    info!("Data channel '{}'-'{}' open", dc_label2, dc_id2);

                    Box::pin(async move {
                        let mut tracks2 = Arc::clone(&tracks2);       // TODO: avoid this?
                        let mut result = Result::<usize>::Ok(0);
                        while result.is_ok() {
                            let timeout = tokio::time::sleep(Duration::from_secs(5));
                            tokio::pin!(timeout);

                            tokio::select! {
                                _ = notify_renegotiation.notified() => {
                                    let media = HACK_STATE.lock().unwrap().rooms.get(&room).unwrap().user_track_to_mime.clone(); // TODO: avoid this?

                                    let videos = media.iter().filter(|(_, mime)| mime.as_str() == "video").count();
                                    let audios = media.iter().filter(|(_, mime)| mime.as_str() == "audio").count();

                                    for ((user, track_id), mime) in media {
                                        if tracks2.read().unwrap().contains_key(&(user.clone(), track_id.clone())) {
                                            continue;
                                        }

                                        let mime = match mime.as_ref() {
                                            "video" => MIME_TYPE_VP8,
                                            "audio" => MIME_TYPE_OPUS,
                                            _ => unreachable!(),
                                        };

                                        info!("add new track for {} {}", user, track_id);

                                        let track = Arc::new(TrackLocalStaticRTP::new(
                                            RTCRtpCodecCapability {
                                                mime_type: mime.to_owned(),
                                                ..Default::default()
                                            },
                                            track_id.to_string(),       // TODO: verify it, id to media id
                                            user.to_string(),           // TODO: verify it, msid to user
                                        ));

                                        // for later dyanmic RTP dispatch from NATS
                                        tracks2.write().unwrap().insert((user, track_id), track.clone());
                                        let pc = Arc::clone(&pc);

                                        // Read incoming RTCP packets
                                        // Before these packets are returned they are processed by interceptors. For things
                                        // like NACK this needs to be called.
                                        tokio::spawn(async move {
                                            let rtp_sender = pc
                                                .add_track(Arc::clone(&track) as Arc<dyn TrackLocal + Send + Sync>)
                                                .await?;
                                            let mut rtcp_buf = vec![0u8; 1500];
                                            while let Ok((_, _)) = rtp_sender.read(&mut rtcp_buf).await {}
                                            Result::<()>::Ok(())
                                        });
                                    }

                                    dc2.send_text(format!("RENEGOTIATION videos {} audios {}", videos, audios)).await;
                                },
                                _ = timeout.as_mut() => {
                                    let message = "hello".to_string();
                                    info!("Sending '{}'", message);
                                    result = dc2.send_text(message).await;
                                }
                            };
                        }
                    })
                })).await;

                // Register text message handling
                let dc3 = Arc::clone(&dc);  // TODO: avoid this?
                dc.on_message(Box::new(move |msg: DataChannelMessage| {
                    let pc = Arc::clone(&pc2);  // TODO: avoid this?
                    let dc3 = Arc::clone(&dc3);  // TODO: avoid this?
                    let msg_str = String::from_utf8(msg.data.to_vec()).unwrap();
                    info!("Message from DataChannel '{}': '{}'", dc_label, msg_str);
                    if msg_str.starts_with("SDP_OFFER ") {
                        let offer = msg_str.splitn(2, " ").skip(1).next().unwrap();
                        info!("got new SDP offer: {}", offer);
                        // build SDP Offer type
                        let mut sdp = RTCSessionDescription::default();
                        sdp.sdp_type = RTCSdpType::Offer;
                        sdp.sdp = offer.to_string();
                        let offer = sdp;
                        return Box::pin(async move {
                            let dc3 = Arc::clone(&dc3);
                            pc.set_remote_description(offer).await.unwrap();
                            info!("updated new SDP offer");
                            let answer = pc.create_answer(None).await.unwrap();
                            pc.set_local_description(answer.clone()).await.unwrap();
                            if let Some(answer) = pc.local_description().await {
                                info!("sent new SDP answer");
                                dc3.send_text(format!("SDP_ANSWER {}", answer.sdp)).await;
                            }
                        });
                    }

                    Box::pin(async {})
                })).await;
            })
        }))
        .await;

    // Set the handler for Peer connection state
    // This will notify you when the peer has connected/disconnected
    peer_connection
        .on_peer_connection_state_change(Box::new(move |s: RTCPeerConnectionState| {
            info!("Peer Connection State has changed: {}", s);

            if s == RTCPeerConnectionState::Failed {
                // Wait until PeerConnection has had no network activity for 30 seconds or another failure. It may be reconnected using an ICE Restart.
                // Use webrtc.PeerConnectionStateDisconnected if you are interested in detecting faster timeout.
                // Note that the PeerConnection may come back from PeerConnectionStateDisconnected.
                info!("Peer Connection has gone to failed exiting: Done forwarding");
                // TODO: cleanup?
                // std::process::exit(0);
            }

            // if s == RTCPeerConnectionState::Connected {
            //     info!("nats to webrtc connected!")
            // }

            Box::pin(async {})
        }))
        .await;

    // Set the remote SessionDescription
    peer_connection.set_remote_description(offer).await?;

    // Create an answer
    let answer = peer_connection.create_answer(None).await?;

    // Create channel that is blocked until ICE Gathering is complete
    let mut gather_complete = peer_connection.gathering_complete_promise().await;

    // Sets the LocalDescription, and starts our UDP listeners
    peer_connection.set_local_description(answer).await?;

    // Block until ICE Gathering is complete, disabling trickle ICE
    // we do this because we only can exchange one signaling message
    // in a production application you should exchange ICE Candidates via OnICECandidate
    let _ = gather_complete.recv().await;

    // Output the answer in base64 so we can paste it in browser
    if let Some(local_desc) = peer_connection.local_description().await {
        info!("PC send local SDP");
        answer_tx.send(local_desc.sdp);
    } else {
        warn!("generate local_description failed!");
    }

    // get RTP from NATS
    let sub = nc.subscribe(&subject).await?;

    // Read RTP packets forever and send them to the WebRTC Client
    tokio::spawn(async move {
        use webrtc::error::Error;
        while let Some(msg) = sub.next().await {
            let raw_rtp = msg.data;

            // TODO: real dyanmic dispatch for RTP
            // subject sample: "rtc.1234.user1.video1"
            let mut it = msg.subject.rsplitn(3, ".").take(2);
            let track_id = it.next().unwrap().to_string();
            let user = it.next().unwrap().to_string();
            let track = {
                let tracks = tracks.read().unwrap();
                let track = tracks.get(&(user, track_id));
                // FIXME: we should always prepare all the tracks for sending RTP
                // TODO: create new tracks in renegotiation case
                if track.is_none() {
                    continue;
                }
                track.unwrap().clone()
            };

            if let Err(err) = track.write(&raw_rtp).await {
                error!("nats forward err: {:?}", err);
                if Error::ErrClosedPipe.equal(&err) {
                    // The peerConnection has been closed.
                    return;
                } else {
                    error!("track write err: {}", err);
                    // TODO: cleanup?
                    // std::process::exit(0);
                }
            }
        }
    });

    // TODO: a signal to close connection?
    info!("PC wait");
    tokio::time::sleep(Duration::from_secs(3000)).await;
    peer_connection.close().await?;

    Ok(())
}


/// Web server for communicating with web clients
#[actix_web::main]
async fn web_main() -> std::io::Result<()> {
    HttpServer::new(||
            App::new()
                // .wrap(Cors::default())
                // enable logger
                .wrap(actix_web::middleware::Logger::default())
                .service(Files::new("/static", "site").prefer_utf8(true))   // demo site
                .service(create_room)
                .service(create_pub)
                .service(publish)
                .service(subscribe_all)
                .service(list)
        )
        .bind("127.0.0.1:8080")?
        .run()
        .await
}

#[derive(Debug, Serialize, Deserialize)]
struct CreateRoomParams {
    room: String,
    token: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct CreatePubParams {
    room: String,
    id: String,
    token: Option<String>,
}


#[post("/create/room")]
async fn create_room(params: web::Json<CreateRoomParams>) -> impl Responder {
    // TODO: save to cache that shared across instances
    info!("create room: {:?}", params);
    if let Some(token) = params.token.clone() {
        let mut state = HACK_STATE.lock().unwrap();
        let mut room = state.rooms.entry(params.room.clone()).or_default();
        room.sub_token = token;
    }
    "room set"
}


#[post("/create/pub")]
async fn create_pub(params: web::Json<CreatePubParams>) -> impl Responder {
    // TODO: save to cache that shared across instances
    info!("create pub: {:?}", params);
    if let Some(token) = params.token.clone() {
        let mut state = HACK_STATE.lock().unwrap();
        let room = state.rooms.entry(params.room.clone()).or_default();
        let pub_token = room.pub_tokens.entry(params.id.clone()).or_default();
        *pub_token = token;
    }
    "pub set"
}


/// WebRTC WHIP compatible endpoint for publisher
#[post("/pub/{room}/{id}")]
async fn publish(auth: BearerAuth,
                 path: web::Path<(String, String)>,
                 sdp: web::Bytes) -> impl Responder {
                 // web::Json(sdp): web::Json<RTCSessionDescription>) -> impl Responder {
    let (room, id) = path.into_inner();

    // TODO: verify "Content-Type: application/sdp"

    // token verification
    {
        let mut state = HACK_STATE.lock().unwrap();
        let room = state.rooms.entry(room.clone()).or_default();
        let token = room.pub_tokens.get(&id);
        // if let Some(token) = token {
        //     if token != auth.token() {
        //         return HttpResponse::Unauthorized().body("bad token");
        //     }
        // }
    }

    let sdp = String::from_utf8(sdp.to_vec()).unwrap(); // FIXME: no unwrap
    info!("pub: auth {} sdp {:?}", auth.token(), sdp);
    let (tx, rx) = tokio::sync::oneshot::channel();
    tokio::spawn(webrtc_to_nats(room.clone(), id.clone(), sdp, tx, format!("rtc.{}.{}", room, id)));
    // TODO: timeout
    let sdp_answer = rx.await.unwrap();     // FIXME: no unwrap
    info!("SDP answer: {}", sdp_answer);
    HttpResponse::Created() // 201
        .content_type("application/sdp")
        .append_header(("Location", ""))    // TODO: what's the need?
        .body(sdp_answer)
}

#[post("/sub/{room}/{id}")]
async fn subscribe_all(auth: BearerAuth,
                       path: web::Path<(String, String)>,
                       sdp: web::Bytes) -> impl Responder {
    let (room, id) = path.into_inner();

    // TODO: verify "Content-Type: application/sdp"

    // token verification
    // TODO: per user token?
    {
        let mut state = HACK_STATE.lock().unwrap();
        let room = state.rooms.entry(room.clone()).or_default();
        // if room.sub_token != auth.token() {
        //     return HttpResponse::Unauthorized().body("bad token");
        // }
    }

    let sdp = String::from_utf8(sdp.to_vec()).unwrap(); // FIXME: no unwrap
    info!("sub_all: auth {} sdp {:?}", auth.token(), sdp);
    let (tx, rx) = tokio::sync::oneshot::channel();
    tokio::spawn(nats_to_webrtc(room.clone(), id.clone(), sdp, tx, format!("rtc.{}.*.*", room)));
    // TODO: timeout
    let sdp_answer = rx.await.unwrap();     // FIXME: no unwrap
    info!("SDP answer: {}", sdp_answer);
    HttpResponse::Created() // 201
        .content_type("application/sdp")
        .body(sdp_answer)
}

#[post("/info/{room}/list")]
async fn list(auth: BearerAuth,
              path: web::Path<String>) -> impl Responder {
    let room = path.into_inner();
    // TODO: token verification
    // auth.token().to_string()
    // return participants
    unimplemented!();
    ""
}
