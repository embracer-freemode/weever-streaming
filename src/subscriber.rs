use crate::state::{SharedState, SHARED_STATE, Command};
use crate::helper::catch;
use crate::cli;
use anyhow::{Result, Context, anyhow};
use log::{debug, info, warn, error};
use webrtc::{
    api::{
        APIBuilder,
        interceptor_registry::register_default_interceptors,
        media_engine::{MediaEngine, MIME_TYPE_OPUS, MIME_TYPE_VP8},
        setting_engine::SettingEngine,
    },
    interceptor::registry::Registry,
    peer_connection::{
        RTCPeerConnection,
        OnICEConnectionStateChangeHdlrFn,
        OnPeerConnectionStateChangeHdlrFn,
        OnDataChannelHdlrFn,
        OnNegotiationNeededHdlrFn,
        sdp::session_description::RTCSessionDescription,
        sdp::sdp_type::RTCSdpType,
        configuration::RTCConfiguration,
        peer_connection_state::RTCPeerConnectionState,
    },
    ice_transport::{
        ice_connection_state::RTCIceConnectionState,
        ice_server::RTCIceServer,
        ice_candidate_type::RTCIceCandidateType,
    },
    rtp_transceiver::{
        rtp_codec::{RTCRtpCodecCapability, RTCRtpCodecParameters, RTPCodecType},
        rtp_sender::RTCRtpSender,
    },
    track::{
        track_local::track_local_static_rtp::TrackLocalStaticRTP,
        track_local::{TrackLocal, TrackLocalWriter},
    },
    data_channel::{
        data_channel_message::DataChannelMessage,
        RTCDataChannel,
        OnMessageHdlrFn,
        OnOpenHdlrFn,
    },
};
use tokio::time::{Duration, timeout};
use tokio::sync::oneshot;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tracing::Instrument;
use std::sync::{Arc, RwLock};
use std::collections::HashMap;
use std::pin::Pin;


/////////////////////////
// Weak reference helper
/////////////////////////

#[derive(Clone)]
struct WeakPeerConnection(std::sync::Weak<RTCPeerConnection>);

impl WeakPeerConnection {
    // Try upgrading a weak reference to a strong one
    fn upgrade(&self) -> Option<Arc<RTCPeerConnection>> {
        self.0.upgrade()
    }
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
    notify_sender: Option<mpsc::Sender<Command>>,
    notify_receiver: Option<mpsc::Receiver<Command>>,
    created: std::time::SystemTime,
    tokio_tasks: Arc<RwLock<Vec<JoinHandle<()>>>>,
}

// for logging only
impl std::ops::Drop for SubscriberDetails {
    fn drop(&mut self) {
        info!("dropping SubscriberDetails for room {} user {}", self.room, self.user);
    }
}

impl SubscriberDetails {
    // Downgrade the strong reference to a weak reference
    fn pc_downgrade(&self) -> WeakPeerConnection {
        WeakPeerConnection(Arc::downgrade(&self.pc))
    }

    fn get_nats_subect(&self) -> String {
        format!("rtc.{}.*.*.*", self.room)
    }

    async fn create_pc(_stun: String,
                       turn: Option<String>,
                       turn_username: Option<String>,
                       turn_password: Option<String>,
                       public_ip: Option<String>) -> Result<RTCPeerConnection> {
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
        registry = register_default_interceptors(registry, &mut m).await?;

        let mut setting = SettingEngine::default();
        setting.set_ice_timeouts(
            Some(Duration::from_secs(3)),   // disconnected timeout
            Some(Duration::from_secs(6)),   // failed timeout
            Some(Duration::from_secs(1)),   // keep alive interval
        );

        if let Some(ip) = public_ip {
            // setting.set_nat_1to1_ips(vec![ip], RTCIceCandidateType::Host);
            setting.set_nat_1to1_ips(vec![ip], RTCIceCandidateType::Srflx);
        }

        // Create the API object with the MediaEngine
        let api = APIBuilder::new()
            .with_setting_engine(setting)
            .with_media_engine(m)
            .with_interceptor_registry(registry)
            .build();

        info!("preparing RTCConfiguration");
        // Prepare the configuration
        let mut servers = vec![];
        // disable STUN to try 1 to 1 IP with Srflx for now
        // servers.push(
        //     RTCIceServer {
        //         // e.g.: stun:stun.l.google.com:19302
        //         urls: vec![stun],
        //         ..Default::default()
        //     }
        // );
        if let Some(turn) = turn {
            let username = turn_username.context("TURN username not preset")?;
            let password = turn_password.context("TURN password not preset")?;
            servers.push(
                RTCIceServer {
                    urls: vec![turn],
                    username,
                    credential: password,
                    ..Default::default()
                }
            );
        }
        let config = RTCConfiguration {
            ice_servers: servers,
            ..Default::default()
        };

        info!("creating PeerConnection");
        // Create a new RTCPeerConnection
        api.new_peer_connection(config).await.map_err(|e| anyhow!(e))
    }

    async fn add_trasceivers_based_on_room(&self) -> Result<()> {
        // Create Track that we send video back to browser on
        // TODO: dynamic creation
        // TODO: how to handle video/audio from same publisher and send to different track?
        // HACK_MEDIA.lock().unwrap().push("video".to_string());
        //
        // TODO: share with PUB_JOIN handler

        let mut video_count = 0;
        let mut audio_count = 0;

        let media = SHARED_STATE.get_user_track_to_mime(&self.room).await.context("get user track to mime failed")?;
        for ((user, track_id), mime) in media {
            // skip it if publisher is the same as subscriber
            // so we won't pull back our own media streams and cause echo effect
            // TODO: remove the hardcode "-screen" case
            if user == self.user || user == format!("{}-screen", self.user) {
                continue;
            }

            let app_id = match mime.as_ref() {
                "video" => {
                    let result = format!("video{}", video_count);
                    video_count += 1;
                    result
                },
                "audio" => {
                    let result = format!("audio{}", audio_count);
                    audio_count += 1;
                    result
                },
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

            info!("track map ({}, {}) -> {}", user, track_id, app_id);

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
        // NOTE: busy loop
        self.tokio_tasks.write().unwrap().push(tokio::spawn(async move {
            let mut rtcp_buf = vec![0u8; 1500];
            info!("running RTP sender read");
            while let Ok((_, _)) = rtp_sender.read(&mut rtcp_buf).await {}
            info!("leaving RTP sender read");
        }.instrument(tracing::Span::current())));
    }

    fn register_notify_message(&mut self) {
        // set notify
        let (sender, receiver) = mpsc::channel(10);
        SHARED_STATE.add_sub_notify(&self.room, &self.user, sender.clone());
        self.notify_sender = Some(sender);
        self.notify_receiver = Some(receiver);
    }

    fn deregister_notify_message(&mut self) {
        SHARED_STATE.remove_sub_notify(&self.room, &self.user);
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
        let created = self.created.clone();
        let room = self.room.clone();
        let user = self.user.clone();
        Box::new(move |s: RTCPeerConnectionState| {
            let _enter = span.enter();  // populate user & room info in following logs
            info!("PeerConnection State has changed: {}", s);
            match s {
                RTCPeerConnectionState::Connected => {
                    let now = std::time::SystemTime::now();
                    let duration = match now.duration_since(created) {
                        Ok(d) => d,
                        Err(e) => {
                            error!("system time error: {}", e);
                            Duration::from_secs(42) // fake one for now
                        },
                    }.as_millis();

                    info!("Peer Connection connected! spent {} ms from created", duration);

                    let room = room.clone();
                    let user = user.clone();
                    return Box::pin(async move {
                        catch(SHARED_STATE.add_subscriber(&room, &user)).await;
                    }.instrument(tracing::Span::current()));
                },
                RTCPeerConnectionState::Failed |
                RTCPeerConnectionState::Disconnected |
                RTCPeerConnectionState::Closed => {
                    // NOTE:
                    // In disconnected state, PeerConnection may still come back, e.g. reconnect using an ICE Restart.
                    // But let's cleanup everything for now.
                    info!("send close notification");
                    notify_close.notify_waiters();

                    let room = room.clone();
                    let user = user.clone();
                    return Box::pin(async move {
                        catch(SHARED_STATE.remove_subscriber(&room, &user)).await;
                    }.instrument(tracing::Span::current()));
                },
                _ => {}
            }

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

        let wpc = self.pc_downgrade();
        let tracks = self.tracks.clone();
        let rtp_senders = self.rtp_senders.clone();
        let notify_message = Arc::new(tokio::sync::Mutex::new(self.notify_receiver.take().unwrap()));
        let user_media_to_tracks = self.user_media_to_tracks.clone();
        let user_media_to_senders = self.user_media_to_senders.clone();
        let room = self.room.clone();
        let user = self.user.clone();

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
                user.clone(),
                wpc.clone(),
                dc,
                dc_label,
                dc_id.to_string(),
                tracks.clone(),
                rtp_senders.clone(),
                notify_message.clone(),
                user_media_to_tracks.clone(),
                user_media_to_senders.clone())
        })
    }

    fn on_data_channel_open(
            room: String,
            user: String,
            wpc: WeakPeerConnection,
            dc: Arc<RTCDataChannel>,
            dc_label: String,
            dc_id: String,
            tracks: Arc<RwLock<HashMap<(String, String), Arc<TrackLocalStaticRTP>>>>,
            rtp_senders: Arc<RwLock<HashMap<(String, String), Arc<RTCRtpSender>>>>,
            notify_message: Arc<tokio::sync::Mutex<mpsc::Receiver<Command>>>,
            user_media_to_tracks: Arc<RwLock<HashMap<(String, String), Arc<TrackLocalStaticRTP>>>>,
            user_media_to_senders: Arc<RwLock<HashMap<(String, String), Arc<RTCRtpSender>>>>)
        -> Pin<Box<tracing::instrument::Instrumented<impl std::future::Future<Output = ()>>>> {

        Box::pin(async move {
            dc.on_open(Self::on_data_channel_sender(
                    room,
                    user,
                    wpc.clone(),
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
                    wpc,
                    dc.clone(),
                    dc_label))
                .instrument(tracing::Span::current()).await;
        }.instrument(tracing::Span::current()))
    }

    fn on_data_channel_sender(room: String,
                              user: String,
                              wpc: WeakPeerConnection,
                              dc: Arc<RTCDataChannel>,
                              dc_label: String,
                              dc_id: String,
                              tracks: Arc<RwLock<HashMap<(String, String), Arc<TrackLocalStaticRTP>>>>,
                              rtp_senders: Arc<RwLock<HashMap<(String, String), Arc<RTCRtpSender>>>>,
                              notify_message: Arc<tokio::sync::Mutex<mpsc::Receiver<Command>>>,
                              user_media_to_tracks: Arc<RwLock<HashMap<(String, String), Arc<TrackLocalStaticRTP>>>>,
                              user_media_to_senders: Arc<RwLock<HashMap<(String, String), Arc<RTCRtpSender>>>>) -> OnOpenHdlrFn {
        let span = tracing::Span::current();
        Box::new(move || {
            let _enter = span.enter();  // populate user & room info in following logs
            info!("Data channel '{}'-'{}' open", dc_label, dc_id);

            Box::pin(async move {
                let mut notify_message = notify_message.lock().await;  // TODO: avoid this?
                let max_time = Duration::from_secs(30);

                // TODO: move this into the loop?
                let pc = match wpc.upgrade() {
                    None => return,
                    Some(pc) => pc,
                };

                // ask for renegotiation immediately after datachannel is connected
                // TODO: detect if there is media?
                let mut result = Self::send_data_renegotiation(dc.clone(), pc.clone()).await;

                while result.is_ok() {
                    // use a timeout to make sure we have chance to leave the waiting task even it's closed
                    // TODO: use notify_close directly
                    let msg = timeout(max_time, notify_message.recv()).await;
                    if let Ok(msg) = msg {
                        // we get data before timeout
                        let media = SHARED_STATE.get_user_track_to_mime(&room).await.unwrap();

                        let cmd = msg.unwrap();
                        info!("cmd from internal sender: {:?}", cmd);
                        let msg = cmd.to_user_msg();

                        match cmd {
                            Command::PubJoin(join_user) => {
                                Self::on_pub_join(
                                    room.clone(),
                                    join_user.to_string(),
                                    user.clone(),
                                    pc.clone(),
                                    media.clone(),
                                    tracks.clone(),
                                    rtp_senders.clone(),
                                    user_media_to_tracks.clone(),
                                    user_media_to_senders.clone(),
                                ).await;

                                Self::send_data(dc.clone(), msg).await.unwrap();
                                result = Self::send_data_renegotiation(dc.clone(), pc.clone()).await;
                            },
                            Command::PubLeft(_user) => {
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
                            },
                        }
                    } else {
                        result = Self::send_data(dc.clone(), "PING".to_string()).await;
                    };
                }

                info!("leaving data channel loop for '{}'-'{}'", dc_label, dc_id);
            }.instrument(span.clone()))
        })
    }

    fn on_data_channel_msg(wpc: WeakPeerConnection, dc: Arc<RTCDataChannel>, dc_label: String) -> OnMessageHdlrFn {
        let span = tracing::Span::current();
        Box::new(move |msg: DataChannelMessage| {
            let _enter = span.enter();  // populate user & room info in following logs

            let pc = match wpc.upgrade() {
                None => return Box::pin(async {}),
                Some(pc) => pc,
            };

            let dc = dc.clone();

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
            } else if msg_str == "STOP" {
                info!("actively close peer connection");
                return Box::pin(async move {
                    let _ = pc.close().await;
                }.instrument(span.clone()));
            }

            Box::pin(async {})
        })
    }

    async fn on_pub_join(
            _room: String,
            join_user: String,
            sub_user: String,
            pc: Arc<RTCPeerConnection>,
            media: HashMap<(String, String), String>,
            tracks: Arc<RwLock<HashMap<(String, String), Arc<TrackLocalStaticRTP>>>>,
            rtp_senders: Arc<RwLock<HashMap<(String, String), Arc<RTCRtpSender>>>>,
            user_media_to_tracks: Arc<RwLock<HashMap<(String, String), Arc<TrackLocalStaticRTP>>>>,
            user_media_to_senders: Arc<RwLock<HashMap<(String, String), Arc<RTCRtpSender>>>>) {

        info!("pub join: {}", join_user);

        // skip it if publisher is the same as subscriber
        // so we won't pull back our own media streams and cause echo effect
        // TODO: remove the hardcode "-screen" case
        if join_user == sub_user || join_user == format!("{}-screen", sub_user){
            return;
        }

        let mut video_count = 0;
        let mut audio_count = 0;

        // add new track for PUB_JOIN
        // TODO: use better mechanism replace "(user, track) -> mime"
        for ((pub_user, track_id), mime) in media {
            // skip it if publisher is the same as subscriber
            // so we won't pull back our own media streams and cause echo effect
            if pub_user == sub_user || pub_user == format!("{}-screen", sub_user) {
                continue;
            }

            if tracks.read().unwrap().contains_key(&(pub_user.clone(), track_id.clone())) {
                continue;
            }

            // hardcode this for now
            // we may need to change it to support like screen sharing later
            let app_id = match mime.as_ref() {
                "video" => {
                    let result = format!("video{}", video_count);
                    video_count += 1;
                    result
                },
                "audio" => {
                    let result = format!("audio{}", audio_count);
                    audio_count += 1;
                    result
                },
                _ => unreachable!(),
            };

            let mime = match mime.as_ref() {
                "video" => MIME_TYPE_VP8,
                "audio" => MIME_TYPE_OPUS,
                _ => unreachable!(),
            };

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

                // NOTE: make sure the track is added before leaving this function
                let rtp_sender = pc
                    .add_track(Arc::clone(&track) as Arc<dyn TrackLocal + Send + Sync>)
                    .await.unwrap();

                // Read incoming RTCP packets
                // Before these packets are returned they are processed by interceptors. For things
                // like NACK this needs to be called.
                tokio::spawn(async move {
                    info!("create new rtp sender for {} {}", pub_user, track_id);
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

    #[allow(dead_code)]
    async fn on_pub_leave(
            left_user: String,
            _room: String,
            pc: Arc<RTCPeerConnection>,
            _media: HashMap<(String, String), String>,
            tracks: Arc<RwLock<HashMap<(String, String), Arc<TrackLocalStaticRTP>>>>,
            rtp_senders: Arc<RwLock<HashMap<(String, String), Arc<RTCRtpSender>>>>,
            _user_media_to_tracks: Arc<RwLock<HashMap<(String, String), Arc<TrackLocalStaticRTP>>>>,
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
            // let rtp_sender = {
            //     let rtp_senders = rtp_senders.read().unwrap();
            //     rtp_senders.get(&target).unwrap().clone()
            // };
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

        let mut user_media_to_senders = user_media_to_senders.write().unwrap();
        user_media_to_senders.remove(&(left_user.clone(), "video0".to_string()));
        user_media_to_senders.remove(&(left_user.clone(), "audio0".to_string()));
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
        // NOTE: busy loop
        self.tokio_tasks.write().unwrap().push(tokio::spawn(async move {
            use webrtc::Error;
            while let Some(msg) = sub.next().await {
                let raw_rtp = msg.data;

                // TODO: leave the loop when subscriber leaves

                // TODO: real dyanmic dispatch for RTP
                // subject sample: "rtc.1234.user1.video.video1" "rtc.1234.user1.audio.audio1"
                let mut it = msg.subject.rsplitn(4, ".").take(3);
                let track_id = it.next().unwrap().to_string();
                let user = it.skip(1).next().unwrap().to_string();
                let track = {
                    let tracks = tracks.read().unwrap();
                    let track = tracks.get(&(user, track_id));
                    // if we don't have corresponding track
                    // either we are not ready for this
                    // or the RTP is from ourself (publisher == subscriber)
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
        }));

        Ok(())
    }
}


/// Pull RTP streams from NATS, and send it to WebRTC
///
// based on [rtp-to-webrtc](https://github.com/webrtc-rs/webrtc/tree/master/examples/rtp-to-webrtc)
#[tracing::instrument(name = "sub", skip(cli, offer, answer_tx), level = "info")]  // following log will have "sub{room=..., user=...}" in INFO level
pub async fn nats_to_webrtc(cli: cli::CliOptions, room: String, user: String, offer: String, answer_tx: oneshot::Sender<String>, tid: u16) -> Result<()> {
    // build SDP Offer type
    let mut sdp = RTCSessionDescription::default();
    sdp.sdp_type = RTCSdpType::Offer;
    sdp.sdp = offer;
    let offer = sdp;

    // NATS
    info!("getting NATS");
    let nc = SHARED_STATE.get_nats().context("get NATS client failed")?;
    let peer_connection = Arc::new(SubscriberDetails::create_pc(
            cli.stun,
            cli.turn,
            cli.turn_username,
            cli.turn_password,
            cli.public_ip,
    ).await?);

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
        created: std::time::SystemTime::now(),
        tokio_tasks: Default::default(),
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
        answer_tx.send(local_desc.sdp).map_err(|s| anyhow!(s).context("SDP answer send error"))?;
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
    subscriber.deregister_notify_message();
    // remove all spawned tasks
    for task in subscriber.tokio_tasks.read().unwrap().iter() {
        task.abort();
    }
    info!("leaving subscriber main");

    Ok(())
}
