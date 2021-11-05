//! Sharing state for whole program.
//! Currently, we share states across instances via Redis.
//! We can implement multiple state mechanism as long as it fit the SharedState trait.

use crate::helper::catch;
use anyhow::{Result, Context};
use async_trait::async_trait;
use log::info;
use tokio::sync::mpsc;
use once_cell::sync::Lazy;
use redis::aio::MultiplexedConnection;
use redis::AsyncCommands;
use std::sync::{Arc, RwLock};
use std::collections::{HashMap, HashSet};


#[derive(Default)]
pub struct InternalState {
    rooms: HashMap<String, Room>,
    nats: Option<nats::asynk::Connection>,
    redis: Option<MultiplexedConnection>,
}

#[derive(Debug, Default)]
struct Room {
    name: String,
    /// (user, track id) -> mime type
    ///
    /// usage for this:
    /// 1. know how many media in a room
    /// 2. know how many media for each publisher
    user_track_to_mime: HashMap<(String, String), String>,
    sub_peers: HashMap<String, PeerConnetionInfo>,
    /// user -> token
    pub_tokens: HashMap<String, String>,
    sub_tokens: HashMap<String, String>,
}

#[derive(Debug, Default)]
struct PeerConnetionInfo {
    name: String,
    notify_message: Option<Arc<mpsc::Sender<String>>>,  // TODO: special enum for all the cases
}

pub type State = Lazy<RwLock<InternalState>>;
pub static SHARED_STATE: State = Lazy::new(|| Default::default());

// TODO: redesign the tracking data structure
#[async_trait]
pub trait SharedState {
    fn set_redis(&self, conn: MultiplexedConnection);
    fn get_redis(&self) -> Result<MultiplexedConnection>;
    fn set_nats(&self, nats: nats::asynk::Connection);
    fn get_nats(&self) -> Result<nats::asynk::Connection>;
    async fn listen_on_commands(&self) -> Result<()>;
    async fn set_pub_token(&self, room: String, user: String, token: String) -> Result<()>;
    async fn set_sub_token(&self, room: String, user: String, token: String) -> Result<()>;
    async fn get_pub_token(&self, room: &str, user: &str) -> Result<String>;
    async fn get_sub_token(&self, room: &str, user: &str) -> Result<String>;
    async fn add_user_track_to_mime(&self, room: String, user: String, track: String, mime: String) -> Result<()>;
    async fn get_user_track_to_mime(&self, room: &str) -> Result<HashMap<(String, String), String>>;
    async fn remove_user_track_to_mime(&self, room: &str, user: &str) -> Result<()>;
    async fn add_publisher(&self, room: &str, user: &str) -> Result<()>;
    async fn remove_publisher(&self, room: &str, user: &str) -> Result<()>;
    async fn list_publishers(&self, room: &str) -> Result<HashSet<String>>;
    async fn exist_publisher(&self, room: &str, user: &str) -> Result<bool>;
    async fn add_subscriber(&self, room: &str, user: &str) -> Result<()>;
    async fn remove_subscriber(&self, room: &str, user: &str) -> Result<()>;
    async fn list_subscribers(&self, room: &str) -> Result<HashSet<String>>;
    async fn exist_subscriber(&self, room: &str, user: &str) -> Result<bool>;
    fn set_sub_notify(&self, room: &str, user: &str, sender: mpsc::Sender<String>);

    async fn send_pub_join(&self, room: &str, pub_user: &str) -> Result<()>;
    async fn on_pub_join(&self, room: &str, pub_user: &str) -> Result<()> ;
    async fn send_pub_leave(&self, room: &str, pub_user: &str) -> Result<()>;
    async fn on_pub_leave(&self, room: &str, pub_user: &str) -> Result<()> ;

    /// Ask all subscribers to do renegotiation
    async fn send_all_subs_renegotiation(&self, room: &str) -> Result<()>;
    async fn on_all_subs_renegotiation(&self, room: &str) -> Result<()>;

    async fn send_pub_media_add(&self, room: &str, pub_user: &str, mime: &str, app_id: &str) -> Result<()>;
    async fn on_pub_media_add(&self, room: &str, msg: &str) -> Result<()>;

    // TODO: generic msg forward method for all subscribers

    // fn send_sub_join();
    // fn on_sub_join();
    // fn send_sub_leave();
    // fn on_sub_leave();
}

#[async_trait]
impl SharedState for State {
    fn set_redis(&self, conn: MultiplexedConnection) {
        let mut state = self.write().unwrap();
        state.redis = Some(conn);
    }

    fn get_redis(&self) -> Result<MultiplexedConnection> {
        let state = self.read().unwrap();
        Ok(state.redis.as_ref().context("get Redis client failed")?.clone())
    }

    fn set_nats(&self, nats: nats::asynk::Connection) {
        let mut state = self.write().unwrap();
        state.nats = Some(nats);
    }

    fn get_nats(&self) -> Result<nats::asynk::Connection> {
        let state = self.read().unwrap();
        Ok(state.nats.as_ref().context("get NATS client failed")?.clone())
    }

    async fn listen_on_commands(&self) -> Result<()> {
        let nats = self.get_nats().context("get NATS client failed")?;
        // cmd.ROOM
        // e.g. cmd.1234
        let subject = "cmd.*";
        let sub = nats.subscribe(subject).await?;

        async fn process(msg: nats::asynk::Message) -> Result<()> {
            let room = msg.subject.splitn(2, ".").skip(1).next().unwrap();
            let msg = String::from_utf8(msg.data.to_vec()).unwrap();
            info!("internal NATS cmd, room {} msg '{}'", room, msg);
            if msg.starts_with("PUB_LEFT ") {
                let user = msg.splitn(2, " ").skip(1).next().unwrap();
                catch(SHARED_STATE.on_pub_leave(room, user)).await;
            } else if msg.starts_with("PUB_JOIN ") {
                let user = msg.splitn(2, " ").skip(1).next().unwrap();
                catch(SHARED_STATE.on_pub_join(room, user)).await;
            } else if msg.starts_with("PUB_MEDIA_ADD ") {
                catch(SHARED_STATE.on_pub_media_add(room, &msg)).await;
            } else if msg.starts_with("ALL_SUBS_RENEGOTIATION") {
                catch(SHARED_STATE.on_all_subs_renegotiation(room)).await;
            }
            Ok(())
        }

        tokio::spawn(async move {
            // TODO: will we exit this loop when disconnect?
            while let Some(msg) = sub.next().await {
                tokio::spawn(catch(process(msg)));
            }
        });

        Ok(())
    }

    async fn set_pub_token(&self, room: String, user: String, token: String) -> Result<()> {
        // local version:
        // let mut state = self.lock().unwrap();
        // let room = state.rooms.entry(room).or_default();
        // let pub_token = room.pub_tokens.entry(user).or_default();
        // *pub_token = token;

        // redis version:
        let mut conn = self.get_redis()?;
        let key = format!("token#pub#{}#{}", room, user);
        let _: Option<()> = conn.set(key, token).await.context("Redis set failed")?;
        Ok(())
    }

    async fn set_sub_token(&self, room: String, user: String, token: String) -> Result<()> {
        // local version:
        // let mut state = self.lock().unwrap();
        // let room = state.rooms.entry(room).or_default();
        // let sub_token = room.sub_tokens.entry(user).or_default();
        // *sub_token = token;

        // redis version:
        let mut conn = self.get_redis()?;
        let key = format!("token#sub#{}#{}", room, user);
        let _: Option<()> = conn.set(key, token).await.context("Redis set failed")?;
        Ok(())
    }

    async fn get_pub_token(&self, room: &str, user: &str) -> Result<String> {
        // local version:
        // let mut state = self.lock().unwrap();
        // let room = state.rooms.entry(room.to_string()).or_default();
        // room.pub_tokens.get(user).cloned()

        // redis version:
        let mut conn = self.get_redis()?;
        let key = format!("token#pub#{}#{}", room, user);
        Ok(conn.get(&key).await.with_context(|| format!("can't get {} from Redis", key))?)
    }

    async fn get_sub_token(&self, room: &str, user: &str) -> Result<String> {
        // local version:
        // let mut state = self.lock().unwrap();
        // let room = state.rooms.entry(room.to_string()).or_default();
        // room.sub_tokens.get(user).cloned()

        // redis version:
        let mut conn = self.get_redis()?;
        let key = format!("token#sub#{}#{}", room, user);
        Ok(conn.get(&key).await.with_context(|| format!("can't get {} from Redis", key))?)
    }

    async fn add_user_track_to_mime(&self, room: String, user: String, track: String, mime: String) -> Result<()> {
        // local version:
        // let room_info = self.lock();
        // let mut room_info = room_info.unwrap();
        // let room_info = room_info.rooms.entry(room).or_default();
        // room_info.user_track_to_mime.insert((user, track), mime);

        info!("add global cache ({}, {}) -> {}", user, track, mime);

        // redis version:
        let mut conn = self.get_redis()?;
        let redis_key = format!("utm#{}", room);
        let hash_key = format!("{}#{}", user, track);
        let _: Option<()> = conn.hset(redis_key, hash_key, mime).await.context("Redis hset failed")?;
        Ok(())
    }

    async fn get_user_track_to_mime(&self, room: &str) -> Result<HashMap<(String, String), String>> {
        // local version:
        // self.lock().unwrap().rooms.get(room).unwrap().user_track_to_mime.clone()  // TODO: avoid this clone?

        // redis version:
        let mut conn = self.get_redis()?;
        let redis_key = format!("utm#{}", room);
        let utm: HashMap<String, String> = conn.hgetall(redis_key).await.context("Redis hgetall failed")?;
        let mut result = HashMap::new();
        for (k, v) in utm.into_iter() {
            let mut it = k.splitn(2, "#");
            let user = it.next().unwrap();
            let track = it.next().unwrap();
            result.insert((user.to_string(), track.to_string()), v.to_string());
        }
        Ok(result)
    }

    async fn remove_user_track_to_mime(&self, room: &str, user: &str) -> Result<()> {
        // local version:
        // let mut tracks = vec![];
        // let mut state = self.lock().unwrap();
        // let room_obj = state.rooms.get(&room.to_string()).unwrap();
        // for ((pub_user, track_id), _) in room_obj.user_track_to_mime.iter() {
        //     if pub_user == &user {
        //         tracks.push(track_id.to_string());
        //     }
        // }
        // let user_track_to_mime = &mut state.rooms.get_mut(&room.to_string()).unwrap().user_track_to_mime;
        // for track in tracks {
        //     user_track_to_mime.remove(&(user.to_string(), track.to_string()));
        // }

        // redis version:
        let mut conn = self.get_redis()?;
        let redis_key = &format!("utm#{}", room);
        let user_track_to_mime = self.get_user_track_to_mime(room).await.context("get user track to mime failed")?;
        for ((pub_user, track_id), _) in user_track_to_mime.iter() {
            if pub_user == &user {
                let hash_key = format!("{}#{}", user, track_id);
                let _: Option<()> = conn.hdel(redis_key, hash_key).await.context("Redis hdel failed")?;
            }
        }
        Ok(())
    }

    async fn add_publisher(&self, room: &str, user: &str) -> Result<()> {
        let mut conn = self.get_redis()?;
        let redis_key = format!("room#{}#pub_list", room);
        let _: Option<()> = conn.sadd(redis_key, user).await.context("Redis sadd failed")?;
        Ok(())
    }

    async fn remove_publisher(&self, room: &str, user: &str) -> Result<()> {
        let mut conn = self.get_redis()?;
        let redis_key = format!("room#{}#pub_list", room);
        let _: Option<()> = conn.srem(redis_key, user).await.context("Redis sadd failed")?;
        Ok(())
    }

    async fn list_publishers(&self, room: &str) -> Result<HashSet<String>> {
        let mut conn = self.get_redis()?;
        let redis_key = format!("room#{}#pub_list", room);
        let result: HashSet<String> = conn.smembers(redis_key).await.context("Redis smembers failed")?;
        Ok(result)
    }

    async fn exist_publisher(&self, room: &str, user: &str) -> Result<bool> {
        let mut conn = self.get_redis()?;
        let redis_key = format!("room#{}#pub_list", room);
        let result: bool = conn.sismember(redis_key, user).await.context("Redis sismember failed")?;
        Ok(result)
    }

    async fn add_subscriber(&self, room: &str, user: &str) -> Result<()> {
        let mut conn = self.get_redis()?;
        let redis_key = format!("room#{}#sub_list", room);
        let _: Option<()> = conn.sadd(redis_key, user).await.context("Redis sadd failed")?;
        Ok(())
    }

    async fn remove_subscriber(&self, room: &str, user: &str) -> Result<()> {
        let mut conn = self.get_redis()?;
        let redis_key = format!("room#{}#sub_list", room);
        let _: Option<()> = conn.srem(redis_key, user).await.context("Redis sadd failed")?;
        Ok(())
    }

    async fn list_subscribers(&self, room: &str) -> Result<HashSet<String>> {
        let mut conn = self.get_redis()?;
        let redis_key = format!("room#{}#sub_list", room);
        let result: HashSet<String> = conn.smembers(redis_key).await.context("Redis smembers failed")?;
        Ok(result)
    }

    async fn exist_subscriber(&self, room: &str, user: &str) -> Result<bool> {
        let mut conn = self.get_redis()?;
        let redis_key = format!("room#{}#sub_list", room);
        let result: bool = conn.sismember(redis_key, user).await.context("Redis sismember failed")?;
        Ok(result)
    }

    fn set_sub_notify(&self, room: &str, user: &str, sender: mpsc::Sender<String>) {
        // TODO: convert this write lock to read lock, use field interior mutability
        let mut state = self.write().unwrap();
        let room = state.rooms.entry(room.to_string()).or_default();
        let user = room.sub_peers.entry(user.to_string()).or_default();
        user.notify_message = Some(Arc::new(sender.clone()));
    }

    async fn send_pub_join(&self, room: &str, pub_user: &str) -> Result<()> {
        let subject = format!("cmd.{}", room);
        let msg = format!("PUB_JOIN {}", pub_user);
        let nats = self.get_nats().context("get NATS client failed")?;
        nats.publish(&subject, msg).await.context("publish PUB_JOIN to NATS failed")?;
        Ok(())
    }

    async fn on_pub_join(&self, room: &str, pub_user: &str) -> Result<()> {
        let subs = {
            let state = self.read().unwrap();
            let room = match state.rooms.get(room) {
                Some(room) => room,
                None => return Ok(()),
            };
            room.sub_peers.iter().map(|(_, sub)| sub.notify_message.as_ref().unwrap().clone()).collect::<Vec<_>>()
        };
        for sub in subs {
            // TODO: special enum for all the cases
            sub.send(format!("PUB_JOIN {}", pub_user)).await.context("send PUB_JOIN to mpsc Sender failed")?;
        }
        Ok(())
    }

    async fn send_pub_leave(&self, room: &str, pub_user: &str) -> Result<()> {
        let subject = format!("cmd.{}", room);
        let msg = format!("PUB_LEFT {}", pub_user);
        let nats = self.get_nats().context("get NATS client failed")?;
        nats.publish(&subject, msg).await.context("publish PUB_LEFT to NATS failed")?;
        Ok(())
    }

    async fn on_pub_leave(&self, room: &str, pub_user: &str) -> Result<()> {
        let subs = {
            let state = self.read().unwrap();
            let room = match state.rooms.get(room) {
                Some(room) => room,
                None => return Ok(()),
            };
            room.sub_peers.iter().map(|(_, sub)| sub.notify_message.as_ref().unwrap().clone()).collect::<Vec<_>>()
        };
        for sub in subs {
            // TODO: special enum for all the cases
            sub.send(format!("PUB_LEFT {}", pub_user)).await.context("send PUB_LEFT to mpsc Sender failed")?;
        }
        Ok(())
    }

    async fn send_all_subs_renegotiation(&self, room: &str) -> Result<()> {
        let subject = format!("cmd.{}", room);
        let msg = "ALL_SUBS_RENEGOTIATION".to_string();
        let nats = self.get_nats().context("get NATS client failed")?;
        nats.publish(&subject, msg).await.context("publish ALL_SUBS_RENEGOTIATION to NATS failed")?;
        Ok(())
    }

    async fn on_all_subs_renegotiation(&self, room: &str) -> Result<()> {
        let subs = {
            let state = self.read().unwrap();
            let room = match state.rooms.get(room) {
                Some(room) => room,
                None => return Ok(()),
            };
            room.sub_peers.iter().map(|(_, sub)| sub.notify_message.as_ref().unwrap().clone()).collect::<Vec<_>>()
        };
        for sub in subs {
            // TODO: special enum for all the cases
            sub.send("RENEGOTIATION".to_string()).await.context("send RENEGOTIATION to mpsc Sender failed")?;
        }
        Ok(())
    }

    async fn send_pub_media_add(&self, room: &str, pub_user: &str, mime: &str, app_id: &str) -> Result<()> {
        let subject = format!("cmd.{}", room);
        let msg = format!("PUB_MEDIA_ADD {} {} {} {}", room, pub_user, mime, app_id);
        let nats = self.get_nats().context("get NATS client failed")?;
        nats.publish(&subject, msg).await.context("publish PUB_MEDIA_ADD to NATS failed")?;
        Ok(())
    }

    async fn on_pub_media_add(&self, room: &str, msg: &str) -> Result<()> {
        let subs = {
            let state = self.read().unwrap();
            let room = match state.rooms.get(room) {
                Some(room) => room,
                None => return Ok(()),
            };
            room.sub_peers.iter().map(|(_, sub)| sub.notify_message.as_ref().unwrap().clone()).collect::<Vec<_>>()
        };
        for sub in subs {
            // TODO: special enum for all the cases
            sub.send(msg.to_string()).await.context("send PUB_MEDIA_ADD to mpsc Sender failed")?;
        }
        Ok(())
    }
}
