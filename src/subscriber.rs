//! Run as publisher in the room.
//! We will extract RTP from NATS and send via WebRTC streams.

use crate::cli;
use crate::helper::catch;
use crate::state::{Command, SharedState, SHARED_STATE};
use anyhow::{anyhow, bail, Context, Result};
use log::{debug, error, info, warn};
use std::collections::HashMap;
use std::pin::Pin;
use std::sync::{Arc, RwLock, Weak};
use tokio::sync::mpsc;
use tokio::sync::oneshot;
use tokio::task::JoinHandle;
use tokio::time::{timeout, Duration};
use tracing::Instrument;
use webrtc::{
    api::{
        interceptor_registry::register_default_interceptors,
        media_engine::{MediaEngine, MIME_TYPE_OPUS, MIME_TYPE_VP8},
        setting_engine::SettingEngine,
        APIBuilder,
    },
    data_channel::{
        data_channel_message::DataChannelMessage, OnMessageHdlrFn, OnOpenHdlrFn, RTCDataChannel,
    },
    ice_transport::{
        ice_candidate_type::RTCIceCandidateType, ice_connection_state::RTCIceConnectionState,
        ice_server::RTCIceServer,
    },
    interceptor::registry::Registry,
    peer_connection::{
        configuration::RTCConfiguration, peer_connection_state::RTCPeerConnectionState,
        sdp::session_description::RTCSessionDescription, OnDataChannelHdlrFn,
        OnICEConnectionStateChangeHdlrFn, OnNegotiationNeededHdlrFn,
        OnPeerConnectionStateChangeHdlrFn, RTCPeerConnection,
    },
    rtp_transceiver::{
        rtp_codec::{RTCRtpCodecCapability, RTCRtpCodecParameters, RTPCodecType},
        rtp_sender::RTCRtpSender,
    },
    track::{
        track_local::track_local_static_rtp::TrackLocalStaticRTP,
        track_local::{TrackLocal, TrackLocalWriter},
    },
};

//////////////
// Subscriber
//////////////

struct SubscriberDetails {
    user: String,
    room: String,
    pc: Arc<RTCPeerConnection>,
    nats: nats::asynk::Connection,
    notify_close: Arc<tokio::sync::Notify>,
    rtp_tracks: Arc<RwLock<HashMap<(String, String), Arc<TrackLocalStaticRTP>>>>,
    rtp_senders: Arc<RwLock<HashMap<(String, String), Arc<RTCRtpSender>>>>,
    rtp_forward_tasks: Arc<RwLock<HashMap<(String, String), JoinHandle<()>>>>,
    notify_sender: RwLock<Option<mpsc::Sender<Command>>>,
    notify_receiver: RwLock<Option<mpsc::Receiver<Command>>>,
    created: std::time::SystemTime,
    tokio_tasks: Arc<RwLock<Vec<JoinHandle<()>>>>,
    // guard for WebRTC flow
    // so we will handle only one renegotiation at once
    // in case there is many publisher come and go in very short interval
    is_doing_renegotiation: Arc<tokio::sync::Mutex<bool>>,
    need_another_renegotiation: Arc<tokio::sync::Mutex<bool>>,
}

struct Subscriber(Arc<SubscriberDetails>);
struct SubscriberWeak(Weak<SubscriberDetails>);

impl SubscriberWeak {
    // Try upgrading a weak reference to a strong one
    fn upgrade(&self) -> Option<Subscriber> {
        self.0.upgrade().map(Subscriber)
    }
}

// To be able to access the internal fields directly
// We wrap the real things with reference counting and new type,
// this will let us use the wrapping type like there is no wrap.
impl std::ops::Deref for Subscriber {
    type Target = SubscriberDetails;

    fn deref(&self) -> &SubscriberDetails {
        &self.0
    }
}

// for logging only
impl std::ops::Drop for SubscriberDetails {
    fn drop(&mut self) {
        info!(
            "dropping SubscriberDetails for room {} user {}",
            self.room, self.user
        );
    }
}

impl Subscriber {
    // Downgrade the strong reference to a weak reference
    fn downgrade(&self) -> SubscriberWeak {
        SubscriberWeak(Arc::downgrade(&self.0))
    }

    fn get_nats_subect(&self, user: &str, mime: &str, app_id: &str) -> String {
        format!("rtc.{}.{}.{}.{}", self.room, user, mime, app_id)
    }

    async fn create_pc(
        _stun: String,
        turn: Option<String>,
        turn_username: Option<String>,
        turn_password: Option<String>,
        public_ip: Option<String>,
    ) -> Result<RTCPeerConnection> {
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

        let mut setting = SettingEngine::default();
        setting.set_ice_timeouts(
            Some(Duration::from_secs(3)), // disconnected timeout
            Some(Duration::from_secs(6)), // failed timeout
            Some(Duration::from_secs(1)), // keep alive interval
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
            servers.push(RTCIceServer {
                urls: vec![turn],
                username,
                credential: password,
                ..Default::default()
            });
        }
        let config = RTCConfiguration {
            ice_servers: servers,
            ..Default::default()
        };

        info!("creating PeerConnection");
        // Create a new RTCPeerConnection
        api.new_peer_connection(config)
            .await
            .map_err(|e| anyhow!(e))
    }

    /// Add transceiver based on room metadata from Redis
    async fn update_transceivers_based_on_room(&self) -> Result<()> {
        // this function should handle follow cases:
        // case1: add user
        // case2: remove user
        // case3: (not cover yet) existing user add media
        // case4: (not cover yet) existing user remove media

        let room = &self.room;
        let sub_user = &self.user;
        let pc = &self.pc;
        let rtp_tracks = &self.rtp_tracks;
        let rtp_senders = &self.rtp_senders;
        let rtp_forward_tasks = &self.rtp_forward_tasks;
        let sub = self;

        let media = SHARED_STATE.get_users_media_count(room).await?;

        let sub_id = sub_user.splitn(2, '+').take(1).next().unwrap_or(""); // "ID+RANDOM" -> "ID"

        let mut remove_targets = vec![];

        // case2: remove user
        for ((pub_user, track_id), rtp_sender) in rtp_senders
            .read()
            .map_err(|e| anyhow!("get rtp_tracks as reader failed: {}", e))?
            .iter()
        {
            let (mime, _count) = track_id.split_at(5);
            if media
                .get(&(pub_user.to_string(), mime.to_string()))
                .is_some()
            {
                continue;
            }
            // current (pub_user, mime) pair does not show up in room shared media metadata
            // pub_user might be left or the media is removed
            remove_targets.push((pub_user.clone(), track_id.clone(), rtp_sender.clone()));
        }

        for (pub_user, track_id, rtp_sender) in remove_targets {
            info!("removing user {} track {}", pub_user, track_id);
            if let Err(err) = pc.remove_track(&rtp_sender).await {
                error!("PeerConnection remove_track error: {}", err);
            }
            let mut rtp_tracks = rtp_tracks
                .write()
                .map_err(|e| anyhow!("get rtp_tracks as writer failed: {}", e))?;
            let mut rtp_senders = rtp_senders
                .write()
                .map_err(|e| anyhow!("get rtp_senders as writer failed: {}", e))?;
            let mut rtp_forward_tasks = rtp_forward_tasks
                .write()
                .map_err(|e| anyhow!("get rtp_forward_tasks as writer failed: {}", e))?;
            rtp_tracks.remove(&(pub_user.clone(), track_id.clone()));
            rtp_senders.remove(&(pub_user.clone(), track_id.clone()));
            if let Some(task) = rtp_forward_tasks.remove(&(pub_user, track_id)) {
                task.abort();
            };
        }

        // case1: add user
        for ((pub_user, raw_mime), count) in media {
            // skip it if publisher is the same as subscriber
            // so we won't pull back our own media streams and cause echo effect
            // TODO: remove the hardcode "-screen" case
            let pub_id = pub_user.splitn(2, '+').take(1).next().unwrap_or(""); // "ID+RANDOM" -> "ID"
            if pub_id == sub_id || pub_id == format!("{}-screen", sub_id) {
                continue;
            }

            for index in 0..count {
                // FIXME: refactor related stuffs
                let track_id = format!("{}{}", raw_mime, index);

                if rtp_tracks
                    .read()
                    .map_err(|e| anyhow!("get rtp_tracks as reader failed: {}", e))?
                    .contains_key(&(pub_user.clone(), track_id.clone()))
                {
                    continue;
                }

                let mime = match raw_mime.as_ref() {
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
                    track_id.clone(), // msid, application id part
                    // stream_id is the group this track belongs too.
                    // This must be unique.
                    pub_user.to_string(), // msid, group id part
                ));

                // for later dyanmic RTP dispatch from NATS
                rtp_tracks
                    .write()
                    .map_err(|e| anyhow!("get rtp_tracks as writer failed: {}", e))?
                    .entry((pub_user.to_string(), track_id.to_string()))
                    .and_modify(|e| *e = track.clone())
                    .or_insert_with(|| track.clone());

                // NOTE: make sure the track is added before leaving this function
                let rtp_sender = pc
                    .add_track(Arc::clone(&track) as Arc<dyn TrackLocal + Send + Sync>)
                    .await
                    .context("add track to PeerConnection failed")?;

                sub.spawn_rtp_foward_task(track, &pub_user, &raw_mime, &track_id)
                    .await?;

                rtp_senders
                    .write()
                    .map_err(|e| anyhow!("get rtp_senders as writer failed: {}", e))?
                    .entry((pub_user.clone(), track_id.clone()))
                    .and_modify(|e| *e = rtp_sender.clone())
                    .or_insert_with(|| rtp_sender.clone());

                // Read incoming RTCP packets
                // Before these packets are returned they are processed by interceptors. For things
                // like NACK this needs to be called.
                // NOTE: disable RTCP reader for now
                // self.spawn_rtcp_reader(rtp_sender);
            }
        }

        Ok(())
    }

    /// Read incoming RTCP packets
    /// Before these packets are returned they are processed by interceptors.
    /// For things like NACK this needs to be called.
    fn spawn_rtcp_reader(&self, rtp_sender: Arc<RTCRtpSender>) -> Result<()> {
        // NOTE: busy loop
        let task = tokio::spawn(
            async move {
                let mut rtcp_buf = vec![0u8; 1500];
                info!("running RTP sender read");
                while let Ok((_, _)) = rtp_sender.read(&mut rtcp_buf).await {}
                info!("leaving RTP sender read");
            }
            .instrument(tracing::Span::current()),
        );

        self.tokio_tasks
            .write()
            .map_err(|e| anyhow!("get tokio_tasks as writer failed: {}", e))?
            .push(task);

        Ok(())
    }

    fn register_notify_message(&mut self) {
        // set notify
        let (sender, receiver) = mpsc::channel(10);
        let result = SHARED_STATE.add_sub_notify(&self.room, &self.user, sender.clone());
        if let Err(err) = result {
            error!("add_sub_notify failed: {:?}", err);
        }
        *self.notify_sender.write().unwrap() = Some(sender);
        *self.notify_receiver.write().unwrap() = Some(receiver);
    }

    fn deregister_notify_message(&mut self) {
        let result = SHARED_STATE.remove_sub_notify(&self.room, &self.user);
        if let Err(err) = result {
            error!("remove_sub_notify failed: {:?}", err);
        }
    }

    fn on_ice_connection_state_change(&self) -> OnICEConnectionStateChangeHdlrFn {
        let span = tracing::Span::current();
        Box::new(move |connection_state: RTCIceConnectionState| {
            let _enter = span.enter(); // populate user & room info in following logs
            info!("ICE Connection State has changed: {}", connection_state);
            Box::pin(async {})
        })
    }

    fn on_peer_connection_state_change(&self) -> OnPeerConnectionStateChangeHdlrFn {
        let span = tracing::Span::current();
        let weak = self.downgrade();
        Box::new(move |s: RTCPeerConnectionState| {
            let _enter = span.enter(); // populate user & room info in following logs
            info!("PeerConnection State has changed: {}", s);

            let sub = match weak.upgrade() {
                Some(sub) => sub,
                _ => return Box::pin(async {}),
            };

            match s {
                RTCPeerConnectionState::Connected => {
                    let now = std::time::SystemTime::now();
                    let duration = match now.duration_since(sub.created) {
                        Ok(d) => d,
                        Err(e) => {
                            error!("system time error: {}", e);
                            Duration::from_secs(42) // fake one for now
                        }
                    }
                    .as_millis();

                    info!(
                        "Peer Connection connected! spent {} ms from created",
                        duration
                    );

                    return Box::pin(
                        async move {
                            catch(SHARED_STATE.add_subscriber(&sub.room, &sub.user)).await;
                        }
                        .instrument(tracing::Span::current()),
                    );
                }
                RTCPeerConnectionState::Failed
                | RTCPeerConnectionState::Disconnected
                | RTCPeerConnectionState::Closed => {
                    // NOTE:
                    // In disconnected state, PeerConnection may still come back, e.g. reconnect using an ICE Restart.
                    // But let's cleanup everything for now.
                    info!("send close notification");
                    sub.notify_close.notify_waiters();

                    return Box::pin(
                        async move {
                            catch(SHARED_STATE.remove_subscriber(&sub.room, &sub.user)).await;
                        }
                        .instrument(tracing::Span::current()),
                    );
                }
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
        let weak = self.downgrade();

        Box::new(move |dc: Arc<RTCDataChannel>| {
            let _enter = span.enter(); // populate user & room info in following logs

            let dc_label = dc.label().to_owned();
            // only accept data channel with label "control"
            if dc_label != "control" {
                return Box::pin(async {});
            }
            let dc_id = dc.id();
            info!("New DataChannel {} {}", dc_label, dc_id);

            let sub = match weak.upgrade() {
                Some(sub) => sub,
                _ => return Box::pin(async {}),
            };

            // channel open handling
            sub.on_data_channel_open(dc, dc_label, dc_id.to_string())
        })
    }

    fn on_data_channel_open(
        &self,
        dc: Arc<RTCDataChannel>,
        dc_label: String,
        dc_id: String,
    ) -> Pin<Box<tracing::instrument::Instrumented<impl std::future::Future<Output = ()>>>> {
        let weak = self.downgrade();

        Box::pin(
            async move {
                let sub = match weak.upgrade() {
                    Some(sub) => sub,
                    _ => return,
                };

                dc.on_open(sub.on_data_channel_sender(dc.clone(), dc_label.clone(), dc_id));

                // Register text message handling
                dc.on_message(sub.on_data_channel_msg(dc.clone(), dc_label));
            }
            .instrument(tracing::Span::current()),
        )
    }

    fn on_data_channel_sender(
        &self,
        dc: Arc<RTCDataChannel>,
        dc_label: String,
        dc_id: String,
    ) -> OnOpenHdlrFn {
        let span = tracing::Span::current();
        let weak = self.downgrade();
        Box::new(move || {
            let _enter = span.enter(); // populate user & room info in following logs
            info!("Data channel '{}'-'{}' open", dc_label, dc_id);

            Box::pin(async move {
                let sub = match weak.upgrade() {
                    Some(sub) => sub,
                    _ => return,
                };

                let pc = &sub.pc;
                let sub_id = sub.user.splitn(2, '+').take(1).next().unwrap_or("");  // "ID+RANDOM" -> "ID"
                let is_doing_renegotiation = &sub.is_doing_renegotiation;
                let need_another_renegotiation = &sub.need_another_renegotiation;

                // wrapping for increase lifetime, cause we will have await later
                let notify_message = sub.notify_receiver.write().unwrap().take().unwrap();
                let notify_message = Arc::new(tokio::sync::Mutex::new(notify_message));
                let mut notify_message = notify_message.lock().await;  // TODO: avoid this?

                let notify_close = sub.notify_close.clone();

                // ask for renegotiation immediately after datachannel is connected
                // TODO: detect if there is media?
                {
                    let mut is_doing_renegotiation = is_doing_renegotiation.lock().await;
                    *is_doing_renegotiation = true;
                    catch(sub.update_transceivers_based_on_room()).await;
                    catch(Self::send_data_sdp_offer(dc.clone(), pc.clone())).await;
                }

                let mut result = Ok(0);
                while result.is_ok() {
                    // use a timeout to make sure we have chance to leave the waiting task even it's closed
                    tokio::select! {
                        _ = notify_close.notified() => {
                            info!("notified closed, leaving data channel");
                            break
                        },
                        msg = notify_message.recv() => {
                            // we get data before timeout
                            let cmd = match msg {
                                Some(cmd) => cmd,
                                _ => break,     // e.g. alread closed
                            };

                            info!("cmd from internal sender: {:?}", cmd);
                            let msg = cmd.to_user_msg();

                            match cmd {
                                Command::PubJoin(pub_user) | Command::PubLeft(pub_user) => {
                                    let pub_id = pub_user.splitn(2, '+').take(1).next().unwrap_or("");  // "ID+RANDOM" -> "ID"
                                    // don't send PUB_JOIN/PUB_LEFT if current subscriber is the publisher
                                    if pub_id == sub_id {
                                        continue;
                                    }
                                    result = Self::send_data(dc.clone(), msg).await;
                                    // don't send SDP_OFFER if we are not done with previous round
                                    // it means frotend is still on going another renegotiation
                                    let mut is_doing_renegotiation = is_doing_renegotiation.lock().await;
                                    if !*is_doing_renegotiation {
                                        info!("trigger renegotiation");
                                        *is_doing_renegotiation = true;
                                        catch(sub.update_transceivers_based_on_room()).await;
                                        catch(Self::send_data_sdp_offer(dc.clone(), pc.clone())).await;
                                    } else {
                                        info!("mark as need renegotiation");
                                        let mut need_another_renegotiation = need_another_renegotiation.lock().await;
                                        *need_another_renegotiation = true;
                                    }
                                },
                            }

                            info!("cmd from internal sender handle done");
                        }
                    }
                }

                if let Err(err) = result {
                    error!("data channel error: {}", err);
                }

                info!("leaving data channel loop for '{}'-'{}'", dc_label, dc_id);
            }.instrument(span.clone()))
        })
    }

    fn on_data_channel_msg(&self, dc: Arc<RTCDataChannel>, dc_label: String) -> OnMessageHdlrFn {
        let span = tracing::Span::current();
        let weak = self.downgrade();
        Box::new(move |msg: DataChannelMessage| {
            let _enter = span.enter(); // populate user & room info in following logs

            let sub = match weak.upgrade() {
                Some(sub) => sub,
                _ => return Box::pin(async {}),
            };

            let pc = sub.pc.clone();

            let dc = dc.clone();

            let msg_str = String::from_utf8(msg.data.to_vec()).unwrap_or_default();
            info!("Message from DataChannel '{}': '{:.20}'", dc_label, msg_str);

            if msg_str.starts_with("SDP_ANSWER ") {
                let answer = match msg_str.split_once(' ').map(|x| x.1) {
                    Some(o) => o,
                    _ => return Box::pin(async {}),
                };
                debug!("got new SDP answer: {}", answer);
                // build SDP Offer type
                let answer = RTCSessionDescription::answer(answer.to_string()).unwrap();
                let need_another_renegotiation = sub.need_another_renegotiation.clone();
                let is_doing_renegotiation = sub.is_doing_renegotiation.clone();
                return Box::pin(
                    async move {
                        let dc = dc.clone();
                        if let Err(err) = pc.set_remote_description(answer).await {
                            error!("SDP_ANSWER set error: {}", err);
                            return;
                        }
                        info!("updated new SDP offer");
                        // check if we have another renegotiation on hold
                        let mut need_another_renegotiation =
                            need_another_renegotiation.lock().await;
                        if *need_another_renegotiation {
                            info!("trigger another round of renegotiation");
                            catch(sub.update_transceivers_based_on_room()).await; // update the transceivers now
                            *need_another_renegotiation = false;
                            catch(Self::send_data_sdp_offer(dc.clone(), pc.clone())).await;
                        } else {
                            info!("mark renegotiation as done");
                            let mut is_doing_renegotiation = is_doing_renegotiation.lock().await;
                            *is_doing_renegotiation = false;
                        }
                    }
                    .instrument(span.clone()),
                );
            } else if msg_str == "STOP" {
                info!("actively close peer connection");
                return Box::pin(
                    async move {
                        let _ = pc.close().await;
                    }
                    .instrument(span.clone()),
                );
            }

            info!(
                "DataChannel {} message handle done: '{:.20}'",
                dc_label, msg_str
            );

            // still send something back even if we don't do special things
            // so browser knows server received the messages
            Box::pin(
                async move {
                    if let Err(err) = dc.send_text("OK".to_string()).await {
                        error!("send OK to data channel error: {}", err);
                    };
                }
                .instrument(span.clone()),
            )
        })
    }

    async fn send_data(dc: Arc<RTCDataChannel>, msg: String) -> Result<usize, webrtc::Error> {
        info!("data channel sending: {}", &msg);
        let result = dc.send_text(msg).await;
        info!("data channel sent done");
        result
    }

    async fn send_data_sdp_offer(
        dc: Arc<RTCDataChannel>,
        pc: Arc<RTCPeerConnection>,
    ) -> Result<()> {
        info!("making SDP offer");
        let offer = match pc.create_offer(None).await {
            Ok(offer) => offer,
            Err(err) => {
                bail!("recreate offer error: {}", err);
            }
        };
        if let Err(err) = pc.set_local_description(offer.clone()).await {
            bail!("set local SDP error: {}", err);
        };
        if let Some(offer) = pc.local_description().await {
            info!("sent new SDP offer");
            if let Err(err) = dc.send_text(format!("SDP_OFFER {}", offer.sdp)).await {
                bail!("send SDP_OFFER to data channel error: {}", err);
            };
        } else {
            bail!("somehow didn't get local SDP?!");
        }
        Ok(())
    }

    async fn spawn_rtp_foward_task(
        &self,
        track: Arc<TrackLocalStaticRTP>,
        user: &str,
        mime: &str,
        app_id: &str,
    ) -> Result<()> {
        // get RTP from NATS
        let subject = self.get_nats_subect(user, mime, app_id);
        info!("subscribe NATS: {}", subject);
        let sub = self
            .nats
            .subscribe(&subject)
            .await
            .map_err(|_| anyhow!("can't subscribe for RTP forward"))?;

        // Read RTP packets forever and send them to the WebRTC Client
        // NOTE: busy loop
        let task = tokio::spawn(
            async move {
                use webrtc::Error;
                while let Some(msg) = sub.next().await {
                    // TODO: make sure we leave the loop when subscriber/publisher leave
                    let raw_rtp = msg.data;
                    if let Err(err) = track.write(&raw_rtp).await {
                        error!("nats forward err: {:?}", err);
                        if Error::ErrClosedPipe == err {
                            // The peerConnection has been closed.
                            return;
                        } else {
                            error!("track write err: {}", err);
                            // TODO: cleanup?
                        }
                    }
                }
                info!("leaving NATS to RTP pull: {}", subject);
            }
            .instrument(tracing::Span::current()),
        );

        let _ = self
            .rtp_forward_tasks
            .write()
            .map_err(|e| anyhow!("get rtp_forward_tasks as writer failed: {}", e))?
            .insert((user.to_string(), app_id.to_string()), task);

        Ok(())
    }
}

/// Pull RTP streams from NATS, and send it to WebRTC
///
// based on [rtp-to-webrtc](https://github.com/webrtc-rs/webrtc/tree/master/examples/rtp-to-webrtc)
#[tracing::instrument(name = "sub", skip(cli, offer, answer_tx), level = "info")] // following log will have "sub{room=..., user=...}" in INFO level
pub async fn nats_to_webrtc(
    cli: cli::CliOptions,
    room: String,
    user: String,
    offer: String,
    answer_tx: oneshot::Sender<String>,
    tid: u16,
) -> Result<()> {
    // build SDP Offer type
    let offer = RTCSessionDescription::offer(offer.to_string()).unwrap();

    // NATS
    info!("getting NATS");
    let nc = SHARED_STATE.get_nats().context("get NATS client failed")?;
    let peer_connection = Arc::new(
        Subscriber::create_pc(
            cli.stun,
            cli.turn,
            cli.turn_username,
            cli.turn_password,
            cli.public_ip,
        )
        .await?,
    );

    let mut subscriber = Subscriber(Arc::new(SubscriberDetails {
        user: user.clone(),
        room: room.clone(),
        nats: nc.clone(),
        pc: peer_connection.clone(),
        notify_close: Default::default(),
        rtp_tracks: Default::default(),
        rtp_senders: Default::default(),
        rtp_forward_tasks: Default::default(),
        notify_sender: Default::default(),
        notify_receiver: Default::default(),
        created: std::time::SystemTime::now(),
        tokio_tasks: Default::default(),
        is_doing_renegotiation: Default::default(),
        need_another_renegotiation: Default::default(),
    }));

    subscriber.register_notify_message();

    // Set the handler for ICE connection state
    // This will notify you when the peer has connected/disconnected
    peer_connection.on_ice_connection_state_change(subscriber.on_ice_connection_state_change());

    // Register data channel creation handling
    peer_connection.on_data_channel(subscriber.on_data_channel());

    // Set the handler for Peer connection state
    // This will notify you when the peer has connected/disconnected
    peer_connection.on_peer_connection_state_change(subscriber.on_peer_connection_state_change());

    peer_connection.on_negotiation_needed(subscriber.on_negotiation_needed());

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
        answer_tx
            .send(local_desc.sdp)
            .map_err(|s| anyhow!(s).context("SDP answer send error"))?;
    } else {
        // TODO: when will this happen?
        warn!("generate local_description failed!");
    }

    // limit a subscriber to 24 hours for now
    // after 24 hours, we close the connection
    let max_time = Duration::from_secs(24 * 60 * 60);
    timeout(max_time, subscriber.notify_close.notified()).await?;
    peer_connection.close().await?;
    subscriber.deregister_notify_message();
    // remove all spawned tasks
    for task in subscriber
        .rtp_forward_tasks
        .read()
        .map_err(|e| anyhow!("get rtp_forward_tasks as reader failed: {}", e))?
        .values()
    {
        task.abort();
    }
    for task in subscriber
        .tokio_tasks
        .read()
        .map_err(|e| anyhow!("get tokio_tasks as reader failed: {}", e))?
        .iter()
    {
        task.abort();
    }
    info!("leaving subscriber main");

    Ok(())
}
