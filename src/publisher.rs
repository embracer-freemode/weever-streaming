use crate::state::{SHARED_STATE, SharedState};
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
        OnTrackHdlrFn,
        OnICEConnectionStateChangeHdlrFn,
        OnPeerConnectionStateChangeHdlrFn,
        OnDataChannelHdlrFn,
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
        rtp_receiver::RTCRtpReceiver,
    },
    track::track_remote::TrackRemote,
    data_channel::{
        data_channel_message::DataChannelMessage,
        RTCDataChannel,
        OnMessageHdlrFn,
    },
    rtcp::payload_feedbacks::picture_loss_indication::PictureLossIndication,
};
use tokio::time::{Duration, timeout};
use tokio::sync::oneshot;
use tracing::{Instrument, info_span};
use std::collections::HashMap;
use std::pin::Pin;
use std::sync::{Arc, Weak, RwLock};


struct Publisher(Arc<Publisher>);
struct PublisherWeak(Weak<Publisher>);

impl PublisherWeak {
    // Try upgrading a weak reference to a strong one
    fn upgrade(&self) -> Option<Publisher> {
        self.0.upgrade().map(Publisher)
    }
}

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

/////////////
// Publisher
/////////////

struct PublisherDetails {
    user: String,
    room: String,
    pc: Arc<RTCPeerConnection>,
    nats: nats::asynk::Connection,
    notify_close: Arc<tokio::sync::Notify>,
    created: std::time::SystemTime,
    video_count: u8,
    audio_count: u8,
    gen_id_to_app_id: Arc<RwLock<HashMap<String, String>>>,
}

// for logging only
impl std::ops::Drop for PublisherDetails {
    fn drop(&mut self) {
        info!("dropping PublisherDetails for room {} user {}", self.room, self.user);
    }
}

// To be able to access the internal fields directly
// We wrap the real things with reference counting and new type,
// this will let us use the wrapping type like there is no wrap.
impl std::ops::Deref for Publisher {
    type Target = PublisherDetails;

    fn deref(&self) -> &PublisherDetails {
        &self.0
    }
}

impl PublisherDetails {
    // Downgrade the strong reference to a weak reference
    fn pc_downgrade(&self) -> WeakPeerConnection {
        WeakPeerConnection(Arc::downgrade(&self.pc))
    }

    fn get_nats_subect(&self) -> String {
        format!("rtc.{}.{}", self.room, self.user)
    }

    async fn create_pc(stun: String,
                       turn: Option<String>,
                       turn_username: Option<String>,
                       turn_password: Option<String>,
                       public_ip: Option<String>) -> Result<RTCPeerConnection> {
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

        // Prepare the configuration
        info!("preparing RTCConfiguration");
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

    async fn add_transceivers_based_on_sdp(&mut self, offer: &str) -> Result<()> {
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
                let app_id = format!("video{}", self.video_count);
                {
                    let gen_id_to_app_id = self.gen_id_to_app_id.read().unwrap();
                    let maybe = gen_id_to_app_id.get(id);
                    if maybe.is_some() {
                        continue;
                    }
                }
                self.gen_id_to_app_id.write().unwrap().insert(id.to_string(), app_id.clone());
                self.video_count += 1;
                // catch(SHARED_STATE.add_user_track_to_mime(self.room.clone(), self.user.clone(), id.to_string(), "video".to_string())).await;
                catch(SHARED_STATE.add_user_track_to_mime(self.room.clone(), self.user.clone(), app_id, "video".to_string())).await;
                self.pc
                    .add_transceiver_from_kind(RTPCodecType::Video, &[])
                    .await?;
            } else if sdp_media.starts_with("audio") {
                let app_id = format!("audio{}", self.audio_count);
                {
                    let gen_id_to_app_id = self.gen_id_to_app_id.read().unwrap();
                    let maybe = gen_id_to_app_id.get(id);
                    if maybe.is_some() {
                        continue;
                    }
                }
                self.gen_id_to_app_id.write().unwrap().insert(id.to_string(), app_id.clone());
                self.audio_count += 1;
                // catch(SHARED_STATE.add_user_track_to_mime(self.room.clone(), self.user.clone(), id.to_string(), "audio".to_string())).await;
                catch(SHARED_STATE.add_user_track_to_mime(self.room.clone(), self.user.clone(), app_id, "audio".to_string())).await;
                self.pc
                    .add_transceiver_from_kind(RTPCodecType::Audio, &[])
                    .await?;
            }
        }

        // FIXME: remove this
        // hardcode an extra video for now
        // {
        //     let app_id = format!("video{}", self.video_count);
        //     // self.gen_id_to_app_id.write().unwrap().insert(id.to_string(), app_id.clone());
        //     self.video_count += 1;
        //     // catch(SHARED_STATE.add_user_track_to_mime(self.room.clone(), self.user.clone(), id.to_string(), "video".to_string())).await;
        //     catch(SHARED_STATE.add_user_track_to_mime(self.room.clone(), self.user.clone(), app_id, "video".to_string())).await;
        //     self.pc
        //         .add_transceiver_from_kind(RTPCodecType::Video, &[])
        //         .await?;
        // }

        Ok(())
    }

    /// Handler for incoming streams
    fn on_track(&self) -> OnTrackHdlrFn {
        let span = tracing::Span::current();

        let nc = self.nats.clone();
        let wpc = self.pc_downgrade();
        let subject = self.get_nats_subect();
        let gen_id_to_app_id = self.gen_id_to_app_id.clone();

        Box::new(move |track: Option<Arc<TrackRemote>>, _receiver: Option<Arc<RTCRtpReceiver>>| {
            let _enter = span.enter();  // populate user & room info in following logs

            info!("getting new track");

            if let Some(track) = track {
                // Send a PLI on an interval so that the publisher is pushing a keyframe every rtcpPLIInterval
                let media_ssrc = track.ssrc();
                Self::spawn_periodic_pli(wpc.clone(), media_ssrc);

                let gen_id_to_app_id = gen_id_to_app_id.clone();
                let nc = nc.clone();
                let subject = subject.clone();
                return Box::pin(async move {
                    let id = &track.id().await;
                    let gen_id_to_app_id = gen_id_to_app_id.read().unwrap();
                    let app_id = gen_id_to_app_id.get(id.as_str()).unwrap();

                    // push RTP to NATS
                    Self::spawn_rtp_to_nats(subject.clone(), app_id.to_string(), track, nc.clone());
                });
            }

            Box::pin(async {})
        })
    }

    /// Send a PLI on an interval so that the publisher is pushing a keyframe every rtcpPLIInterval
    fn spawn_periodic_pli(wpc: WeakPeerConnection, media_ssrc: u32) {
        tokio::spawn(async move {
            let mut result = Ok(0);
            while result.is_ok() {
                let timeout = tokio::time::sleep(Duration::from_secs(3));
                tokio::pin!(timeout);

                tokio::select! {
                    _ = timeout.as_mut() => {
                        let pc = match wpc.upgrade() {
                            None => break,
                            Some(pc) => pc,
                        };

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

    fn spawn_rtp_to_nats(subject: String, app_id: String, track: Arc<TrackRemote>, nats: nats::asynk::Connection) {
        // push RTP to NATS
        // use ID to disquish streams from same publisher
        // TODO: can we use SSRC?
        tokio::spawn(async move {
            // FIXME: the id here generated from browser might be "{...}"
            let kind = match track.kind() {
                RTPCodecType::Video => "v",
                RTPCodecType::Audio => "a",
                RTPCodecType::Unspecified => "u",
            };
            // TODO: replace "track.id()" with more meaningful app id? if browser does not populate meaningful one
            // let subject = format!("{}.{}.{}", subject, kind, track.id().await);
            let subject = format!("{}.{}.{}", subject, kind, app_id);
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
        let created = self.created.clone();

        Box::new(move |s: RTCPeerConnectionState| {
            let _enter = span.enter();  // populate user & room info in following logs

            info!("PeerConnection State has changed: {}", s);

            if s == RTCPeerConnectionState::Failed {
                // Wait until PeerConnection has had no network activity for 30 seconds or another failure. It may be reconnected using an ICE Restart.
                // Use webrtc.PeerConnectionStateDisconnected if you are interested in detecting faster timeout.
                // Note that the PeerConnection may come back from PeerConnectionStateDisconnected.
                info!("Peer Connection has gone to failed exiting: Done forwarding");

                // also do state cleanup here
                // in case we didn't go through disconnected and become failed directly
                let room = room.clone();
                let user = user.clone();
                return Box::pin(async move {
                    catch(SHARED_STATE.remove_publisher(&room, &user)).await;
                }.instrument(tracing::Span::current()));
            }

            if s == RTCPeerConnectionState::Disconnected {
                // TODO: also remove the media from state

                notify_close.notify_waiters();

                let room = room.clone();
                let user = user.clone();
                return Box::pin(async move {
                    // tell subscribers a new publisher just leave
                    // ask subscribers to renegotiation
                    catch(Self::notify_subs_for_leave(&room, &user)).await;
                    catch(SHARED_STATE.remove_publisher(&room, &user)).await;
                }.instrument(tracing::Span::current()));
                // TODO: make sure we will cleanup related stuffs
            }

            if s == RTCPeerConnectionState::Connected {
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
                    catch(SHARED_STATE.add_publisher(&room, &user)).await;
                }.instrument(tracing::Span::current()));
            }

            Box::pin(async {})
        })
    }

    fn on_data_channel(&self) -> OnDataChannelHdlrFn {
        let span = tracing::Span::current();
        let wpc = self.pc_downgrade();
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
                dc_label)
        })
    }

    fn on_data_channel_open(
            room: String,
            user: String,
            wpc: WeakPeerConnection,
            dc: Arc<RTCDataChannel>,
            dc_label: String)
        -> Pin<Box<tracing::instrument::Instrumented<impl std::future::Future<Output = ()>>>> {

        Box::pin(async move {
            // Register text message handling
            dc.on_message(Self::on_data_channel_msg(
                    room,
                    user,
                    wpc,
                    dc.clone(),
                    dc_label))
                .instrument(tracing::Span::current()).await;
        }.instrument(tracing::Span::current()))
    }

    fn on_data_channel_msg(room: String,
                           user: String,
                           wpc: WeakPeerConnection,
                           dc: Arc<RTCDataChannel>,
                           dc_label: String) -> OnMessageHdlrFn {
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
                let user = user.clone();
                let room = room.clone();
                return Box::pin(async move {
                    // add new track for screen sharing
                    // 4 tranceivers: data x 1, video x 2, audio x 1
                    // TODO: add real condition to check if needed
                    // if pc.get_transceivers().await.len() < 4 {
                        // info!("adding new transceiver for screen sharing");
                        // catch(SHARED_STATE.add_user_track_to_mime(room.clone(), user.clone(), "screen".to_string(), "video".to_string())).await;
                        // pc.add_transceiver_from_kind(RTPCodecType::Video, &[]).await.unwrap();
                    // }

                    let dc = dc.clone();
                    pc.set_remote_description(offer).await.unwrap();
                    info!("updated new SDP offer");
                    let answer = pc.create_answer(None).await.unwrap();
                    pc.set_local_description(answer.clone()).await.unwrap();
                    if let Some(answer) = pc.local_description().await {
                        info!("sent new SDP answer");
                        dc.send_text(format!("SDP_ANSWER {}", answer.sdp)).await.unwrap();
                    }

                    // notify subscribers to pull new track
                    // TODO: add condition to check if needed
                    // FIXME: don't hardcode video & app_id
                    catch(SHARED_STATE.send_pub_media_add(&room, &user, "video", "screen")).await;
                }.instrument(span.clone()));
            }

            Box::pin(async {})
        })
    }

    /// tell subscribers a new publisher just join
    /// ask subscribers to renegotiation
    async fn notify_subs_for_join(&self) {
        info!("notify subscribers for publisher join");
        catch(SHARED_STATE.send_pub_join(&self.room, &self.user)).await;
    }

    /// tell subscribers a new publisher just leave
    async fn notify_subs_for_leave(room: &str, user: &str) -> Result<()> {
        info!("notify subscribers for publisher leave");
        // remove from global state
        // TODO: better mechanism
        catch(SHARED_STATE.remove_user_track_to_mime(&room, &user)).await;
        catch(SHARED_STATE.send_pub_leave(&room, &user)).await;
        Ok(())
    }
}

/// Extract RTP streams from WebRTC, and send it to NATS
///
/// based on [rtp-forwarder](https://github.com/webrtc-rs/webrtc/tree/master/examples/rtp-forwarder) example
#[tracing::instrument(name = "pub", skip(cli, offer, answer_tx), level = "info")]  // following log will have "pub{room=..., user=...}" in INFO level
pub async fn webrtc_to_nats(cli: cli::CliOptions, room: String, user: String, offer: String, answer_tx: oneshot::Sender<String>, tid: u16) -> Result<()> {
    // NATS
    info!("getting NATS");
    let nc = SHARED_STATE.get_nats().context("get NATS client failed")?;

    let peer_connection = Arc::new(PublisherDetails::create_pc(
            cli.stun,
            cli.turn,
            cli.turn_username,
            cli.turn_password,
            cli.public_ip,
    ).await.context("create PeerConnection failed")?);
    let mut publisher = PublisherDetails {
        user: user.clone(),
        room: room.clone(),
        pc: peer_connection.clone(),
        nats: nc.clone(),
        notify_close: Default::default(),
        created: std::time::SystemTime::now(),
        video_count: 0,
        audio_count: 0,
        gen_id_to_app_id: Default::default(),
    };  // TODO: remove clone

    publisher.add_transceivers_based_on_sdp(&offer).await.context("add tranceivers based on SDP failed")?;

    // // FIXME: remove this hack for screen sharing
    // catch(SHARED_STATE.add_user_track_to_mime(room.clone(), user.clone(), "screen".to_string(), "video".to_string())).await;
    // publisher.pc.add_transceiver_from_kind(RTPCodecType::Video, &[]).await.unwrap();

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

    // Register data channel creation handling
    peer_connection
        .on_data_channel(publisher.on_data_channel())
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
    peer_connection.set_local_description(answer).await.context("set local SDP failed")?;

    // Block until ICE Gathering is complete, disabling trickle ICE
    // we do this because we only can exchange one signaling message
    // in a production application you should exchange ICE Candidates via OnICECandidate
    let _ = gather_complete.recv().await;

    // Send out the SDP answer via Sender
    if let Some(local_desc) = peer_connection.local_description().await {
        info!("PC send local SDP");
        answer_tx.send(local_desc.sdp).map_err(|s| anyhow!(s).context("SDP answer send error"))?;
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
