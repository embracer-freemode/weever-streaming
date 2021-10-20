//! WebRTC SFU with horizontal scale design

use anyhow::{Result, Context};
use log::{debug, info, warn, error};
use interceptor::registry::Registry;
use rtcp::payload_feedbacks::picture_loss_indication::PictureLossIndication;
use webrtc::api::interceptor_registry::register_default_interceptors;
use webrtc::api::media_engine::{MediaEngine, MIME_TYPE_OPUS, MIME_TYPE_VP8};
use webrtc::api::APIBuilder;
use webrtc::media::rtp::rtp_codec::{RTCRtpCodecCapability, RTCRtpCodecParameters, RTPCodecType};
use webrtc::media::rtp::rtp_receiver::RTCRtpReceiver;
use webrtc::media::rtp::rtp_sender::RTCRtpSender;
use webrtc::media::track::track_remote::TrackRemote;
use webrtc::media::track::track_local::track_local_static_rtp::TrackLocalStaticRTP;
use webrtc::media::track::track_local::{TrackLocal, TrackLocalWriter};
use webrtc::peer::peer_connection::RTCPeerConnection;
use webrtc::peer::peer_connection::{
    OnTrackHdlrFn,
    OnICEConnectionStateChangeHdlrFn,
    OnPeerConnectionStateChangeHdlrFn,
    OnDataChannelHdlrFn,
    OnNegotiationNeededHdlrFn,
};
use webrtc::peer::configuration::RTCConfiguration;
use webrtc::peer::ice::ice_connection_state::RTCIceConnectionState;
use webrtc::peer::ice::ice_server::RTCIceServer;
use webrtc::peer::peer_connection_state::RTCPeerConnectionState;
use webrtc::peer::sdp::session_description::RTCSessionDescription;
use webrtc::peer::sdp::sdp_type::RTCSdpType;
use webrtc::data::data_channel::data_channel_message::DataChannelMessage;
use webrtc::data::data_channel::RTCDataChannel;
use webrtc::data::data_channel::{
    OnMessageHdlrFn,
    OnOpenHdlrFn,
};
use std::sync::Arc;
use std::collections::HashMap;
use std::pin::Pin;
use tokio::time::{Duration, timeout};
use tokio::sync::oneshot;
use tokio::sync::mpsc;
use actix_web::{post, web, App, HttpServer, Responder, HttpResponse};
use actix_web_httpauth::extractors::bearer::BearerAuth;
// use actix_cors::Cors;
use actix_files::Files;
use serde::{Deserialize, Serialize};
use once_cell::sync::Lazy;
use tracing_subscriber::{fmt, layer::SubscriberExt, EnvFilter};
use tracing::{Instrument, info_span};
use std::sync::Mutex;
use std::sync::RwLock;


fn main() -> Result<()> {
    // logger
    // bridge "log" crate and "tracing" crate
    tracing_log::LogTracer::init()?;
    // create "logs" dir if not exist
    if !std::path::Path::new("./logs").is_dir() {
        std::fs::create_dir("logs")?;
    }
    // logfile writer
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)?.as_millis();
    let file = format!("rtc.{}.log", now);
    let file_appender = tracing_appender::rolling::never("logs", file);
    let (non_blocking, _guard) = tracing_appender::non_blocking(file_appender);
    // compose our complex logger
    // 1. filter via RUST_LOG env
    // 2. output to stdout
    // 3. output to logfile
    let subscriber = tracing_subscriber::registry()
        .with(EnvFilter::from_default_env())    // RUST_LOG env filter
        .with(fmt::Layer::new().with_writer(std::io::stdout))
        .with(fmt::Layer::new().with_writer(non_blocking));
    // set our logger as global default
    tracing::subscriber::set_global_default(subscriber).context("Unable to set global collector")?;

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
    sub_tokens: HashMap<String, String>,
}

#[derive(Debug, Default)]
struct PeerConnetionInfo {
    name: String,
    // pc: Option<RTCPeerConnection>,
    notify_message: Option<Arc<mpsc::Sender<String>>>,  // TODO: special enum for all the cases
    // notify_close: ...,
}

static HACK_STATE: Lazy<Mutex<State>> = Lazy::new(|| Default::default());


/////////////
// Publisher
/////////////

struct PublisherDetails {
    user: String,
    room: String,
    pc: Arc<RTCPeerConnection>,
    nats: nats::asynk::Connection,
    notify_close: Arc<tokio::sync::Notify>,
}

// for logging only
impl std::ops::Drop for PublisherDetails {
    fn drop(&mut self) {
        info!("dropping PublisherDetails for room {} user {}", self.room, self.user);
    }
}

impl PublisherDetails {
    fn get_nats_subect(&self) -> String {
        format!("rtc.{}.{}", self.room, self.user)
    }

    async fn create_pc() -> Result<RTCPeerConnection, webrtc::Error> {
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

        info!("creating PeerConnection");
        // Create a new RTCPeerConnection
        api.new_peer_connection(config).await
    }

    async fn add_transceivers_based_on_sdp(&self, offer: &str) -> Result<()> {
        info!("add tranceivers based on SDP offer");

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
                    let room_info = room_info.rooms.entry(self.room.clone()).or_default();
                    room_info.user_track_to_mime.insert((self.user.clone(), id.to_string()), "video".to_string());
                }
                self.pc
                    .add_transceiver_from_kind(RTPCodecType::Video, &[])
                    .await?;
            } else if sdp_media.starts_with("audio") {
                // FIXME: use better way
                // TODO: add id in
                {
                    let room_info = HACK_STATE.lock();
                    let mut room_info = room_info.unwrap();
                    let room_info = room_info.rooms.entry(self.room.clone()).or_default();
                    room_info.user_track_to_mime.insert((self.user.clone(), id.to_string()), "audio".to_string());
                }
                self.pc
                    .add_transceiver_from_kind(RTPCodecType::Audio, &[])
                    .await?;
            }
        }

        Ok(())
    }

    /// Handler for incoming streams
    fn on_track(&self) -> OnTrackHdlrFn {
        let span = tracing::Span::current();

        let nc = self.nats.clone();
        let pc = self.pc.clone();
        let subject = self.get_nats_subect();

        Box::new(move |track: Option<Arc<TrackRemote>>, _receiver: Option<Arc<RTCRtpReceiver>>| {
            let _enter = span.enter();  // populate user & room info in following logs

            if let Some(track) = track {
                // Send a PLI on an interval so that the publisher is pushing a keyframe every rtcpPLIInterval
                let media_ssrc = track.ssrc();
                Self::spawn_periodic_pli(pc.clone(), media_ssrc);

                // push RTP to NATS
                Self::spawn_rtp_to_nats(subject.clone(), track, nc.clone());
            }

            Box::pin(async {})
        })
    }

    /// Send a PLI on an interval so that the publisher is pushing a keyframe every rtcpPLIInterval
    fn spawn_periodic_pli(pc: Arc<RTCPeerConnection>, media_ssrc: u32) {
        tokio::spawn(async move {
            let mut result = Ok(0);
            while result.is_ok() {
                let timeout = tokio::time::sleep(Duration::from_secs(3));
                tokio::pin!(timeout);

                tokio::select! {
                    _ = timeout.as_mut() => {
                        result = pc.write_rtcp(&PictureLossIndication{
                                sender_ssrc: 0,
                                media_ssrc,
                        }).await;
                    }
                };
            }
            info!("leaving periodic PLI");
        }.instrument(tracing::Span::current()));
    }

    fn spawn_rtp_to_nats(subject: String, track: Arc<TrackRemote>, nats: nats::asynk::Connection) {
        // push RTP to NATS
        // use ID to disquish streams from same publisher
        // TODO: can we use SSRC?
        tokio::spawn(async move {
            // FIXME: the id here generated from browser might be "{...}"
            let subject = format!("{}.{}", subject, track.id().await);
            info!("publish to {}", subject);
            let mut b = vec![0u8; 1500];
            while let Ok((n, _)) = track.read(&mut b).await {
                nats.publish(&subject, &b[..n]).await?;
            }
            info!("leaving RTP to NATS push: {}", subject);
            Result::<()>::Ok(())
        }.instrument(tracing::Span::current()));
    }

    fn on_ice_connection_state_change(&self) -> OnICEConnectionStateChangeHdlrFn {
        let span = tracing::Span::current();
        Box::new(move |connection_state: RTCIceConnectionState| {
            let _enter = span.enter();  // populate user & room info in following logs
            info!("ICE Connection State has changed: {}", connection_state);
            // if connection_state == RTCIceConnectionState::Connected {
            // }
            Box::pin(async {})
        })
    }

    fn on_peer_connection_state_change(&self) -> OnPeerConnectionStateChangeHdlrFn {
        let span = tracing::Span::current();
        let room = self.room.clone();
        let user = self.user.clone();
        let notify_close = self.notify_close.clone();

        Box::new(move |s: RTCPeerConnectionState| {
            let _enter = span.enter();  // populate user & room info in following logs

            info!("PeerConnection State has changed: {}", s);

            if s == RTCPeerConnectionState::Failed {
                // Wait until PeerConnection has had no network activity for 30 seconds or another failure. It may be reconnected using an ICE Restart.
                // Use webrtc.PeerConnectionStateDisconnected if you are interested in detecting faster timeout.
                // Note that the PeerConnection may come back from PeerConnectionStateDisconnected.
                info!("Peer Connection has gone to failed exiting: Done forwarding");

                // TODO: make sure we will cleanup related stuffs
            }

            if s == RTCPeerConnectionState::Disconnected {
                // TODO: also remove the media from state

                notify_close.notify_waiters();

                // tell subscribers a new publisher just leave
                // ask subscribers to renegotiation
                return Self::notify_subs_for_leave(room.clone(), user.clone());
                // TODO: make sure we will cleanup related stuffs
            }

            // if s == RTCPeerConnectionState::Connected {
            //     info!("webrtc to nats connected!");
            // }

            Box::pin(async {})
        })
    }

    /// tell subscribers a new publisher just join
    /// ask subscribers to renegotiation
    async fn notify_subs_for_join(&self) {
        info!("notify subscribers for publisher join");
        let subs = {
            let state = HACK_STATE.lock().unwrap();
            let room = state.rooms.get(&self.room).unwrap();
            room.sub_peers.iter().map(|(_, sub)| sub.notify_message.as_ref().unwrap().clone()).collect::<Vec<_>>()
        };
        let user = self.user.clone();
        for sub in subs {
            // TODO: special enum for all the cases
            sub.send(format!("PUB_JOIN {}", user)).await.unwrap();
        }
    }

    /// tell subscribers a new publisher just leave
    fn notify_subs_for_leave(room: String, user: String) -> Pin<Box<impl std::future::Future<Output = ()>>> {
        return Box::pin(async move {
            info!("notify subscribers for publisher leave");

            let room = room.clone();   // TODO: avoid this?
            let user = user.clone();   // TODO: avoid this?

            // remove from global state
            let mut tracks = vec![];    // TODO: better mechanism
            {
                let mut state = HACK_STATE.lock().unwrap();
                let room_obj = state.rooms.get(&room).unwrap();
                for ((pub_user, track_id), _) in room_obj.user_track_to_mime.iter() {
                    if pub_user == &user {
                        tracks.push(track_id.to_string());
                    }
                }
                let user_track_to_mime = &mut state.rooms.get_mut(&room).unwrap().user_track_to_mime;
                for track in tracks {
                    user_track_to_mime.remove(&(user.to_string(), track.to_string()));
                }
            }

            let subs = {
                let state = HACK_STATE.lock().unwrap();
                let room = state.rooms.get(&room).unwrap();
                room.sub_peers.iter().map(|(_, sub)| sub.notify_message.as_ref().unwrap().clone()).collect::<Vec<_>>()
            };
            for sub in subs {
                // TODO: special enum for all the cases
                sub.send(format!("PUB_LEFT {}", user)).await.unwrap();
            }
        }.instrument(tracing::Span::current()));
    }
}

/// Extract RTP streams from WebRTC, and send it to NATS
///
/// based on [rtp-forwarder](https://github.com/webrtc-rs/webrtc/tree/master/examples/rtp-forwarder) example
#[tracing::instrument(name = "pub", skip(offer, answer_tx), level = "info")]  // following log will have "pub{room=..., user=...}" in INFO level
async fn webrtc_to_nats(room: String, user: String, offer: String, answer_tx: oneshot::Sender<String>, tid: u16) -> Result<()> {
    // NATS
    // TODO: share NATS connection
    info!("connecting NATS");
    let nc = nats::asynk::connect("localhost").await.context("can't connect to NATS")?;

    let peer_connection = Arc::new(PublisherDetails::create_pc().await?);
    let publisher = PublisherDetails {
        user: user.clone(),
        room: room.clone(),
        pc: peer_connection.clone(),
        nats: nc.clone(),
        notify_close: Default::default(),
    };  // TODO: remove clone

    publisher.add_transceivers_based_on_sdp(&offer).await?;

    // build SDP Offer type
    let mut sdp = RTCSessionDescription::default();
    sdp.sdp_type = RTCSdpType::Offer;
    sdp.sdp = offer;
    let offer = sdp;

    // Set a handler for when a new remote track starts, this handler will forward data to our UDP listeners.
    // In your application this is where you would handle/process audio/video
    peer_connection
        .on_track(publisher.on_track())
        .instrument(info_span!("pub"))
        .await;

    // Set the handler for ICE connection state
    // This will notify you when the peer has connected/disconnected
    peer_connection
        .on_ice_connection_state_change(publisher.on_ice_connection_state_change())
        .await;

    // Set the handler for Peer connection state
    // This will notify you when the peer has connected/disconnected
    peer_connection
        .on_peer_connection_state_change(publisher.on_peer_connection_state_change())
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
        answer_tx.send(local_desc.sdp).unwrap();
    } else {
        // TODO: when will this happen?
        warn!("generate local_description failed!");
    }

    publisher.notify_subs_for_join().await;

    // limit a publisher to 3 hours for now
    // after 3 hours, we close the connection
    let max_time = Duration::from_secs(3 * 60 * 60);
    timeout(max_time, publisher.notify_close.notified()).await?;
    peer_connection.close().await?;
    info!("leaving publisher main");

    Ok(())
}


//////////////
// Subscriber
//////////////

struct SubscriberDetails {
    user: String,
    room: String,
    pc: Arc<RTCPeerConnection>,
    nats: nats::asynk::Connection,
    notify_close: Arc<tokio::sync::Notify>,
    tracks: Arc<RwLock<HashMap<(String, String), Arc<TrackLocalStaticRTP>>>>,
    user_media_to_tracks: Arc<RwLock<HashMap<(String, String), Arc<TrackLocalStaticRTP>>>>,
    user_media_to_senders: Arc<RwLock<HashMap<(String, String), Arc<RTCRtpSender>>>>,
    rtp_senders: Arc<RwLock<HashMap<(String, String), Arc<RTCRtpSender>>>>,
    notify_sender: Option<mpsc::Sender<String>>,
    notify_receiver: Option<mpsc::Receiver<String>>,
}

// for logging only
impl std::ops::Drop for SubscriberDetails {
    fn drop(&mut self) {
        info!("dropping SubscriberDetails for room {} user {}", self.room, self.user);
    }
}

impl SubscriberDetails {
    fn get_nats_subect(&self) -> String {
        format!("rtc.{}.*.*", self.room)
    }

    async fn create_pc() -> Result<RTCPeerConnection, webrtc::Error> {
        info!("creating MediaEngine");
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

        info!("preparing RTCConfiguration");
        // Prepare the configuration
        let config = RTCConfiguration {
            ice_servers: vec![RTCIceServer {
                urls: vec!["stun:stun.l.google.com:19302".to_owned()],
                ..Default::default()
            }],
            ..Default::default()
        };

        info!("creating PeerConnection");
        // Create a new RTCPeerConnection
        api.new_peer_connection(config).await
    }

    async fn add_trasceivers_based_on_room(&self) -> Result<()> {
        // Create Track that we send video back to browser on
        // TODO: dynamic creation
        // TODO: how to handle video/audio from same publisher and send to different track?
        // HACK_MEDIA.lock().unwrap().push("video".to_string());

        let media = HACK_STATE.lock().unwrap().rooms.get(&self.room).unwrap().user_track_to_mime.clone(); // TODO: avoid this?
        for ((user, track_id), mime) in media {
            let app_id = match mime.as_ref() {
                "video" => "video0",
                "audio" => "audio0",
                _ => unreachable!(),
            };

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
                // id is the unique identifier for this Track.
                // This should be unique for the stream, but doesn’t have to globally unique.
                // A common example would be 'audio' or 'video' or 'desktop' or 'webcam'
                app_id.to_string(),         // msid, application id part
                // stream_id is the group this track belongs too.
                // This must be unique.
                user.to_string(),           // msid, group id part
            ));

            // for later dyanmic RTP dispatch from NATS
            self.tracks.write().unwrap().insert((user.clone(), track_id.clone()), track.clone());
            self.user_media_to_tracks.write().unwrap().insert((user.clone(), app_id.to_string()), track.clone());

            let rtp_sender = self.pc
                .add_track(Arc::clone(&track) as Arc<dyn TrackLocal + Send + Sync>)
                .await?;

            // for later cleanup
            self.rtp_senders.write().unwrap().insert((user.clone(), track_id.clone()), rtp_sender.clone());
            self.user_media_to_senders.write().unwrap().insert((user.clone(), app_id.to_string()), rtp_sender.clone());

            // Read incoming RTCP packets
            // Before these packets are returned they are processed by interceptors. For things
            // like NACK this needs to be called.
            self.spawn_rtcp_reader(rtp_sender);
        }

        Ok(())
    }

    /// Read incoming RTCP packets
    /// Before these packets are returned they are processed by interceptors.
    /// For things like NACK this needs to be called.
    fn spawn_rtcp_reader(&self, rtp_sender: Arc<RTCRtpSender>) {
        tokio::spawn(async move {
            let mut rtcp_buf = vec![0u8; 1500];
            info!("running RTP sender read");
            while let Ok((_, _)) = rtp_sender.read(&mut rtcp_buf).await {}
            info!("leaving RTP sender read");
            Result::<()>::Ok(())
        }.instrument(tracing::Span::current()));
    }

    fn register_notify_message(&mut self) {
        // set notify
        let (sender, receiver) = mpsc::channel(10);
        let mut state = HACK_STATE.lock().unwrap();
        let room = state.rooms.get_mut(&self.room).unwrap();
        let user = room.sub_peers.entry(self.user.clone()).or_default();
        user.notify_message = Some(Arc::new(sender.clone()));
        self.notify_sender = Some(sender);
        self.notify_receiver = Some(receiver);
    }

    fn on_ice_connection_state_change(&self) -> OnICEConnectionStateChangeHdlrFn {
        let span = tracing::Span::current();
        Box::new(move |connection_state: RTCIceConnectionState| {
            let _enter = span.enter();  // populate user & room info in following logs
            info!("ICE Connection State has changed: {}", connection_state);
            // if connection_state == RTCIceConnectionState::Connected {
            // }
            Box::pin(async {})
        })
    }

    fn on_peer_connection_state_change(&self) -> OnPeerConnectionStateChangeHdlrFn {
        let span = tracing::Span::current();
        let notify_close = self.notify_close.clone();
        Box::new(move |s: RTCPeerConnectionState| {
            let _enter = span.enter();  // populate user & room info in following logs
            info!("PeerConnection State has changed: {}", s);
            if s == RTCPeerConnectionState::Failed {
                // Wait until PeerConnection has had no network activity for 30 seconds or another failure. It may be reconnected using an ICE Restart.
                // Use webrtc.PeerConnectionStateDisconnected if you are interested in detecting faster timeout.
                // Note that the PeerConnection may come back from PeerConnectionStateDisconnected.
                info!("Peer Connection has gone to failed exiting: Done forwarding");
                // TODO: make sure we cleanup related resource
            }

            if s == RTCPeerConnectionState::Disconnected {
                notify_close.notify_waiters();
            }

            // if s == RTCPeerConnectionState::Connected {
            // }

            Box::pin(async {})
        })
    }

    fn on_negotiation_needed(&self) -> OnNegotiationNeededHdlrFn {
        Box::new(move || {
            info!("on_negotiation_needed");
            Box::pin(async {})
        })
    }

    fn on_data_channel(&mut self) -> OnDataChannelHdlrFn {
        let span = tracing::Span::current();

        let pc = self.pc.clone();
        let tracks = self.tracks.clone();
        let rtp_senders = self.rtp_senders.clone();
        let notify_message = Arc::new(tokio::sync::Mutex::new(self.notify_receiver.take().unwrap()));
        let user_media_to_tracks = self.user_media_to_tracks.clone();
        let user_media_to_senders = self.user_media_to_senders.clone();
        let notify_sender = self.notify_sender.as_ref().unwrap().clone();
        let room = self.room.clone();

        Box::new(move |dc: Arc<RTCDataChannel>| {
            let _enter = span.enter();  // populate user & room info in following logs

            let dc_label = dc.label().to_owned();
            // only accept data channel with label "control"
            if dc_label != "control" {
               return Box::pin(async {});
            }
            let dc_id = dc.id();
            info!("New DataChannel {} {}", dc_label, dc_id);

            // channel open handling
            Self::on_data_channel_open(
                room.clone(),
                pc.clone(),
                dc,
                dc_label,
                dc_id.to_string(),
                tracks.clone(),
                rtp_senders.clone(),
                notify_message.clone(),
                user_media_to_tracks.clone(),
                user_media_to_senders.clone(),
                notify_sender.clone())
        })
    }

    fn on_data_channel_open(
            room: String,
            pc: Arc<RTCPeerConnection>,
            dc: Arc<RTCDataChannel>,
            dc_label: String,
            dc_id: String,
            tracks: Arc<RwLock<HashMap<(String, String), Arc<TrackLocalStaticRTP>>>>,
            rtp_senders: Arc<RwLock<HashMap<(String, String), Arc<RTCRtpSender>>>>,
            notify_message: Arc<tokio::sync::Mutex<mpsc::Receiver<String>>>,
            user_media_to_tracks: Arc<RwLock<HashMap<(String, String), Arc<TrackLocalStaticRTP>>>>,
            user_media_to_senders: Arc<RwLock<HashMap<(String, String), Arc<RTCRtpSender>>>>,
            notify_sender: mpsc::Sender<String>)
        -> Pin<Box<tracing::instrument::Instrumented<impl std::future::Future<Output = ()>>>> {

        Box::pin(async move {
            dc.on_open(Self::on_data_channel_sender(
                    room,
                    pc.clone(),
                    dc.clone(),
                    dc_label.clone(),
                    dc_id,
                    tracks.clone(),
                    rtp_senders.clone(),
                    notify_message.clone(),
                    user_media_to_tracks.clone(),
                    user_media_to_senders.clone()))
                .instrument(tracing::Span::current()).await;

            // Register text message handling
            dc.on_message(Self::on_data_channel_msg(
                    pc,
                    dc.clone(),
                    dc_label,
                    notify_sender))
                .instrument(tracing::Span::current()).await;
        }.instrument(tracing::Span::current()))
    }

    fn on_data_channel_sender(room: String,
                              pc: Arc<RTCPeerConnection>,
                              dc: Arc<RTCDataChannel>,
                              dc_label: String,
                              dc_id: String,
                              tracks: Arc<RwLock<HashMap<(String, String), Arc<TrackLocalStaticRTP>>>>,
                              rtp_senders: Arc<RwLock<HashMap<(String, String), Arc<RTCRtpSender>>>>,
                              notify_message: Arc<tokio::sync::Mutex<mpsc::Receiver<String>>>,
                              user_media_to_tracks: Arc<RwLock<HashMap<(String, String), Arc<TrackLocalStaticRTP>>>>,
                              user_media_to_senders: Arc<RwLock<HashMap<(String, String), Arc<RTCRtpSender>>>>) -> OnOpenHdlrFn {
        let span = tracing::Span::current();
        Box::new(move || {
            let _enter = span.enter();  // populate user & room info in following logs
            info!("Data channel '{}'-'{}' open", dc_label, dc_id);

            Box::pin(async move {
                let mut result = Ok(0);
                let mut notify_message = notify_message.lock().await;  // TODO: avoid this?
                let max_time = Duration::from_secs(30);

                // ask for renegotiation immediately after datachannel is connected
                // TODO: detect if there is media?
                result = Self::send_data_renegotiation(dc.clone(), pc.clone()).await;

                while result.is_ok() {
                    // use a timeout to make sure we have chance to leave the waiting task even it's closed
                    // TODO: use notify_close directly
                    let msg = timeout(max_time, notify_message.recv()).await;
                    if let Ok(msg) = msg {
                        // we get data before timeout
                        let media = HACK_STATE.lock().unwrap().rooms.get(&room).unwrap().user_track_to_mime.clone(); // TODO: avoid this?

                        let msg = msg.unwrap();

                        if msg.starts_with("PUB_LEFT ") {
                            // NOTE: we don't delete track for now
                            //       when publisher rejoin, we will replace tracks

                            // let left_user = msg.splitn(2, " ").skip(1).next().unwrap();
                            // Self::on_pub_leave(
                            //     left_user.to_string(),
                            //     room.clone(),
                            //     pc.clone(),
                            //     media.clone(),
                            //     tracks.clone(),
                            //     rtp_senders.clone(),
                            //     user_media_to_tracks.clone(),
                            //     user_media_to_senders.clone(),
                            // ).await;

                            Self::send_data(dc.clone(), msg).await.unwrap();
                            result = Self::send_data_renegotiation(dc.clone(), pc.clone()).await;

                        } else if msg.starts_with("PUB_JOIN ") {
                            let join_user = msg.splitn(2, " ").skip(1).next().unwrap();
                            Self::on_pub_join(
                                room.clone(),
                                join_user.to_string(),
                                pc.clone(),
                                media.clone(),
                                tracks.clone(),
                                rtp_senders.clone(),
                                user_media_to_tracks.clone(),
                                user_media_to_senders.clone(),
                            ).await;

                            Self::send_data(dc.clone(), msg).await.unwrap();
                            result = Self::send_data_renegotiation(dc.clone(), pc.clone()).await;

                        } else if msg == "RENEGOTIATION" {
                            // quick hack for triggering renegotiation from frontend
                            result = Self::send_data_renegotiation(dc.clone(), pc.clone()).await;
                        } else {
                            result = Self::send_data(dc.clone(), msg).await;
                        }
                    } else {
                        result = Self::send_data(dc.clone(), "PING".to_string()).await;
                    };
                }

                info!("leaving data channel loop for '{}'-'{}'", dc_label, dc_id);
            }.instrument(span.clone()))
        })
    }

    fn on_data_channel_msg(pc: Arc<RTCPeerConnection>, dc: Arc<RTCDataChannel>, dc_label: String, notify_sender: mpsc::Sender<String>) -> OnMessageHdlrFn {
        let span = tracing::Span::current();
        Box::new(move |msg: DataChannelMessage| {
            let _enter = span.enter();  // populate user & room info in following logs
            let pc = pc.clone();
            let dc = dc.clone();
            let notify_sender = notify_sender.clone();

            let msg_str = String::from_utf8(msg.data.to_vec()).unwrap();
            info!("Message from DataChannel '{}': '{:.20}'", dc_label, msg_str);

            if msg_str.starts_with("SDP_OFFER ") {
                let offer = msg_str.splitn(2, " ").skip(1).next().unwrap();
                debug!("got new SDP offer: {}", offer);
                // build SDP Offer type
                let mut sdp = RTCSessionDescription::default();
                sdp.sdp_type = RTCSdpType::Offer;
                sdp.sdp = offer.to_string();
                let offer = sdp;
                return Box::pin(async move {
                    let dc = dc.clone();
                    pc.set_remote_description(offer).await.unwrap();
                    info!("updated new SDP offer");
                    let answer = pc.create_answer(None).await.unwrap();
                    pc.set_local_description(answer.clone()).await.unwrap();
                    if let Some(answer) = pc.local_description().await {
                        info!("sent new SDP answer");
                        dc.send_text(format!("SDP_ANSWER {}", answer.sdp)).await.unwrap();
                    }
                }.instrument(span.clone()));
            }

            // FIXME: remove this?
            if msg_str.starts_with("RENEGOTIATION") {
                let notify_sender = notify_sender.clone();
                return Box::pin(async move {
                    notify_sender.send("RENEGOTIATION".to_string()).await.unwrap();
                });
            }

            Box::pin(async {})
        })
    }

    async fn on_pub_join(
            room: String,
            join_user: String,
            pc: Arc<RTCPeerConnection>,
            media: HashMap<(String, String), String>,
            tracks: Arc<RwLock<HashMap<(String, String), Arc<TrackLocalStaticRTP>>>>,
            rtp_senders: Arc<RwLock<HashMap<(String, String), Arc<RTCRtpSender>>>>,
            user_media_to_tracks: Arc<RwLock<HashMap<(String, String), Arc<TrackLocalStaticRTP>>>>,
            user_media_to_senders: Arc<RwLock<HashMap<(String, String), Arc<RTCRtpSender>>>>) {

        info!("pub join: {}", join_user);

        // add new track for PUB_JOIN
        for ((pub_user, track_id), mime) in media {
            if tracks.read().unwrap().contains_key(&(pub_user.clone(), track_id.clone())) {
                continue;
            }

            // hardcode this for now
            // we may need to change it to support like screen sharing later
            let app_id = match mime.as_ref() {
                "video" => "video0",
                "audio" => "audio0",
                _ => unreachable!(),
            };

            let mime = match mime.as_ref() {
                "video" => MIME_TYPE_VP8,
                "audio" => MIME_TYPE_OPUS,
                _ => unreachable!(),
            };

            // info!("{} add new track for {} {}", user, pub_user, track_id);   // TODO: or use tracing's span
            info!("add new track for {} {}", pub_user, track_id);

            let track = Arc::new(TrackLocalStaticRTP::new(
                RTCRtpCodecCapability {
                    mime_type: mime.to_owned(),
                    ..Default::default()
                },
                // id is the unique identifier for this Track.
                // This should be unique for the stream, but doesn’t have to globally unique.
                // A common example would be 'audio' or 'video' or 'desktop' or 'webcam'
                app_id.to_string(),     // msid, application id part
                // stream_id is the group this track belongs too.
                // This must be unique.
                pub_user.to_string(),   // msid, group id part
            ));

            // for later dyanmic RTP dispatch from NATS
            tracks.write().unwrap().insert((pub_user.to_string(), track_id.to_string()), track.clone());

            // TODO: cleanup old track
            user_media_to_tracks.write().unwrap().entry((pub_user.to_string(), app_id.to_string()))
                .and_modify(|e| *e = track.clone())
                .or_insert(track.clone());

            let sender = {
                let user_media_to_senders = user_media_to_senders.read().unwrap();
                user_media_to_senders.get(&(pub_user.to_string(), app_id.to_string())).cloned().clone()
            };

            if let Some(sender) = sender {
                // reuse RtcRTPSender
                // apply new track
                info!("switch track for {} {}", pub_user, track_id);
                sender.replace_track(Some(track.clone())).await.unwrap();
                info!("switch track for {} {} done", pub_user, track_id);
            } else {
                // add tracck to pc
                // insert rtp sender to cache
                let pc = Arc::clone(&pc);
                let pub_user = pub_user.clone();
                let track_id = track_id.clone();
                let track = track.clone();
                let rtp_senders = rtp_senders.clone();
                let user_media_to_senders = user_media_to_senders.clone();

                // Read incoming RTCP packets
                // Before these packets are returned they are processed by interceptors. For things
                // like NACK this needs to be called.
                tokio::spawn(async move {
                    info!("create new rtp sender for {} {}", pub_user, track_id);
                    let rtp_sender = pc
                        .add_track(Arc::clone(&track) as Arc<dyn TrackLocal + Send + Sync>)
                        .await?;

                    rtp_senders.write().unwrap().insert((pub_user.clone(), track_id.clone()), rtp_sender.clone());
                    user_media_to_senders.write().unwrap().insert((pub_user.clone(), app_id.to_string()), rtp_sender.clone());
                    let mut rtcp_buf = vec![0u8; 1500];
                    while let Ok((_, _)) = rtp_sender.read(&mut rtcp_buf).await {}
                    info!("leaving RTP sender read");
                    Result::<()>::Ok(())
                }.instrument(tracing::Span::current()));
            }
        }

    }

    async fn on_pub_leave(
            left_user: String,
            room: String,
            pc: Arc<RTCPeerConnection>,
            media: HashMap<(String, String), String>,
            tracks: Arc<RwLock<HashMap<(String, String), Arc<TrackLocalStaticRTP>>>>,
            rtp_senders: Arc<RwLock<HashMap<(String, String), Arc<RTCRtpSender>>>>,
            user_media_to_tracks: Arc<RwLock<HashMap<(String, String), Arc<TrackLocalStaticRTP>>>>,
            user_media_to_senders: Arc<RwLock<HashMap<(String, String), Arc<RTCRtpSender>>>>) {

        info!("pub leave: {}", left_user);

        // delete old track for PUB_LEFT
        let mut remove_targets = vec![];
        for ((pub_user, track_id), _) in tracks.read().unwrap().iter() {
            if pub_user == &left_user {
                info!("remove track for {} {}", pub_user, track_id);
                remove_targets.push((pub_user.to_string(), track_id.to_string()));
            }
        }
        for target in remove_targets {
            tracks.write().unwrap().remove(&target);
            let rtp_sender = {
                let rtp_senders = rtp_senders.read().unwrap();
                rtp_senders.get(&target).unwrap().clone()
            };
            // remove track from subscriber's PeerConnection
            // pc.remove_track(&rtp_sender).await.unwrap();
            // rtp_sender.stop().await.unwrap();
            rtp_senders.write().unwrap().remove(&target);
            // TODO: make sure RTCP task is killed
        }

        // TODO: is this truely remove media info from SDP?
        for transceiver in pc.get_transceivers().await {
            if let Some(rtp_sender) = transceiver.sender().await {
                if let Some(track) = rtp_sender.track().await {
                    if track.stream_id() == left_user {
                        // this include sender.stop() & receiver.stop() & diection = "inactive"
                        transceiver.stop().await.unwrap();
                        // TODO: do we need this?
                        pc.remove_track(&rtp_sender).await.unwrap();
                    }
                }
            }
        }

        // {
        let mut user_media_to_senders = user_media_to_senders.write().unwrap();
        user_media_to_senders.remove(&(left_user.clone(), "video0".to_string()));
        user_media_to_senders.remove(&(left_user.clone(), "audio0".to_string()));
        // }

        // // debug: pc status
        // for t in pc.get_transceivers().await {
        //     info!("t: mid {} kind {}", t.mid().await, t.kind());
        //     if let Some(rtp_sender) = t.sender().await {
        //         info!("t: mid {} kind {}: sender {:?}",
        //               t.mid().await, t.kind(), rtp_sender.get_parameters().await.rtp_parameters);
        //     }
        //     if let Some(rtp_receiver) = t.receiver().await {
        //         info!("t: mid {} kind {}: receiver {:?}",
        //               t.mid().await, t.kind(), rtp_receiver.kind());
        //     }
        // }
    }

    async fn send_data(dc: Arc<RTCDataChannel>, msg: String) -> Result<usize, webrtc::Error> {
        dc.send_text(msg).await
    }

    async fn send_data_renegotiation(dc: Arc<RTCDataChannel>, pc: Arc<RTCPeerConnection>) -> Result<usize, webrtc::Error> {
        let transceiver = pc.get_receivers().await;
        let videos = transceiver.iter().filter(|t| t.kind() == RTPCodecType::Video).count();
        let audios = transceiver.iter().filter(|t| t.kind() == RTPCodecType::Audio).count();
        dc.send_text(format!("RENEGOTIATION videos {} audios {}", videos, audios)).await
    }

    async fn spawn_rtp_foward_task(&self) -> Result<()> {
        // get RTP from NATS
        let subject = self.get_nats_subect();
        let sub = self.nats.subscribe(&subject).await?;
        let tracks = self.tracks.clone();

        // Read RTP packets forever and send them to the WebRTC Client
        tokio::spawn(async move {
            use webrtc::Error;
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
                    if Error::ErrClosedPipe == err {
                        // The peerConnection has been closed.
                        return;
                    } else {
                        error!("track write err: {}", err);
                        // TODO: cleanup?
                        // std::process::exit(0);
                    }
                }
            }
            info!("leaving NATS to RTP pull: {}", subject);
        });

        Ok(())
    }
}


/// Pull RTP streams from NATS, and send it to WebRTC
///
// based on [rtp-to-webrtc](https://github.com/webrtc-rs/webrtc/tree/master/examples/rtp-to-webrtc)
#[tracing::instrument(name = "sub", skip(offer, answer_tx), level = "info")]  // following log will have "sub{room=..., user=...}" in INFO level
async fn nats_to_webrtc(room: String, user: String, offer: String, answer_tx: oneshot::Sender<String>, tid: u16) -> Result<()> {
    // build SDP Offer type
    let mut sdp = RTCSessionDescription::default();
    sdp.sdp_type = RTCSdpType::Offer;
    sdp.sdp = offer;
    let offer = sdp;

    // NATS
    // TODO: share NATS connection
    let nc = nats::asynk::connect("localhost").await.context("can't connect to NATS")?;
    let peer_connection = Arc::new(SubscriberDetails::create_pc().await?);

    let mut subscriber = SubscriberDetails {
        user: user.clone(),
        room: room.clone(),
        nats: nc.clone(),
        pc: peer_connection.clone(),
        notify_close: Default::default(),
        tracks: Default::default(),
        user_media_to_tracks: Default::default(),
        user_media_to_senders: Default::default(),
        rtp_senders: Default::default(),
        notify_sender: None,
        notify_receiver: None,
    };

    subscriber.add_trasceivers_based_on_room().await?;
    subscriber.register_notify_message();

    // Set the handler for ICE connection state
    // This will notify you when the peer has connected/disconnected
    peer_connection
        .on_ice_connection_state_change(subscriber.on_ice_connection_state_change())
        .await;

    // Register data channel creation handling
    peer_connection
        .on_data_channel(subscriber.on_data_channel())
        .await;

    // Set the handler for Peer connection state
    // This will notify you when the peer has connected/disconnected
    peer_connection
        .on_peer_connection_state_change(subscriber.on_peer_connection_state_change())
        .await;

    peer_connection
        .on_negotiation_needed(subscriber.on_negotiation_needed())
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
        answer_tx.send(local_desc.sdp).unwrap();
    } else {
        // TODO: when will this happen?
        warn!("generate local_description failed!");
    }

    subscriber.spawn_rtp_foward_task().await?;

    // limit a subscriber to 3 hours for now
    // after 3 hours, we close the connection
    let max_time = Duration::from_secs(3 * 60 * 60);
    timeout(max_time, subscriber.notify_close.notified()).await?;
    peer_connection.close().await?;
    info!("leaving subscriber main");

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
                .service(Files::new("/demo", "site").prefer_utf8(true))   // demo site
                .service(create_pub)
                .service(create_sub)
                .service(publish)
                .service(subscribe)
        )
        .bind("127.0.0.1:8080")?
        .run()
        .await
}

#[derive(Debug, Serialize, Deserialize)]
struct CreatePubParams {
    room: String,
    id: String,
    token: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct CreateSubParams {
    room: String,
    id: String,
    token: Option<String>,
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


#[post("/create/sub")]
async fn create_sub(params: web::Json<CreateSubParams>) -> impl Responder {
    // TODO: save to cache that shared across instances
    info!("create sub: {:?}", params);
    if let Some(token) = params.token.clone() {
        let mut state = HACK_STATE.lock().unwrap();
        let room = state.rooms.entry(params.room.clone()).or_default();
        let sub_token = room.sub_tokens.entry(params.id.clone()).or_default();
        *sub_token = token;
    }
    "sub set"
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
        if let Some(token) = token {
            if token != auth.token() {
                return HttpResponse::Unauthorized().body("bad token");
            }
        } else {
            return HttpResponse::Unauthorized().body("bad token");
        }
    }

    let sdp = String::from_utf8(sdp.to_vec()).unwrap(); // FIXME: no unwrap
    debug!("pub: auth {} sdp {:.20?}", auth.token(), sdp);
    let (tx, rx) = tokio::sync::oneshot::channel();

    // get a time based id to represent following Tokio task for this user
    // if user call it again later
    // we will be able to identify in logs
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_micros();
    let tid = now.wrapping_div(10000) as u16;

    tokio::spawn(webrtc_to_nats(room.clone(), id.clone(), sdp, tx, tid));
    // TODO: timeout
    let sdp_answer = rx.await.unwrap();     // FIXME: no unwrap
    debug!("SDP answer: {:.20}", sdp_answer);
    HttpResponse::Created() // 201
        .content_type("application/sdp")
        .append_header(("Location", ""))    // TODO: what's the need?
        .body(sdp_answer)
}

#[post("/sub/{room}/{id}")]
async fn subscribe(auth: BearerAuth,
                   path: web::Path<(String, String)>,
                   sdp: web::Bytes) -> impl Responder {
    let (room, id) = path.into_inner();

    // TODO: verify "Content-Type: application/sdp"

    // token verification
    {
        let mut state = HACK_STATE.lock().unwrap();
        let room = state.rooms.entry(room.clone()).or_default();
        let token = room.sub_tokens.get(&id);
        if let Some(token) = token {
            if token != auth.token() {
                return HttpResponse::Unauthorized().body("bad token");
            }
        } else {
            return HttpResponse::Unauthorized().body("bad token");
        }
    }

    let sdp = String::from_utf8(sdp.to_vec()).unwrap(); // FIXME: no unwrap
    debug!("sub_all: auth {} sdp {:.20?}", auth.token(), sdp);
    let (tx, rx) = tokio::sync::oneshot::channel();

    // get a time based id to represent following Tokio task for this user
    // if user call it again later
    // we will be able to identify in logs
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_micros();
    let tid = now.wrapping_div(10000) as u16;

    tokio::spawn(nats_to_webrtc(room.clone(), id.clone(), sdp, tx, tid));
    // TODO: timeout
    let sdp_answer = rx.await.unwrap();     // FIXME: no unwrap
    debug!("SDP answer: {:.20}", sdp_answer);
    HttpResponse::Created() // 201
        .content_type("application/sdp")
        .body(sdp_answer)
}
