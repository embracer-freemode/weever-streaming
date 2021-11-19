//! Sharing state for whole program.
//!
//! The publisher metadata will share across instances (via Redis).
//! The subscribe metadata will be per instance local only.
//!
//! We can implement multiple state mechanism as long as it fit the SharedState trait.

use crate::helper::catch;
use anyhow::{Result, Context, anyhow};
use async_trait::async_trait;
use log::{info, error};
use tokio::sync::mpsc;
use once_cell::sync::Lazy;
use redis::aio::MultiplexedConnection;
use redis::AsyncCommands;
use bincode::{Decode, Encode, config::Configuration};
use std::sync::{Arc, RwLock};
use std::collections::{HashMap, HashSet};


/// Command that we will send across instances.
/// And we will send some of them to frontend after serialization.
#[derive(Decode, Encode)]
#[derive(Debug, Clone)]
pub enum Command {
    PubJoin(String),
    PubLeft(String),
}

impl Command {
    /// convert into messages that we forward to frontend
    pub fn to_user_msg(&self) -> String {
        match &self {
            Self::PubJoin(user) => format!("PUB_JOIN {}", user),
            Self::PubLeft(user) => format!("PUB_LEFT {}", user),
        }
    }
}


/// Global state for whole program
/// It holds NATS & Redis connection,
/// and per room metadata
#[derive(Default)]
pub struct InternalState {
    rooms: HashMap<String, Room>,
    nats: Option<nats::asynk::Connection>,
    redis: Option<MultiplexedConnection>,
}

/// Room metadata
#[derive(Debug, Default)]
struct Room {
    name: String,
    subs: HashMap<String, PeerConnetionInfo>,
}

#[derive(Debug, Default)]
struct PeerConnetionInfo {
    name: String,
    notify_message: Option<Arc<mpsc::Sender<Command>>>,
}

pub type State = Lazy<RwLock<InternalState>>;
pub static SHARED_STATE: State = Lazy::new(|| Default::default());

/// Interface for global state
///
/// NOTE:
/// Current implementation use NATS & Redis.
/// But we can have different implementation.
#[async_trait]
pub trait SharedState {
    /// Set Redis connection
    fn set_redis(&self, conn: MultiplexedConnection) -> Result<()>;
    /// Get Redis connection
    fn get_redis(&self) -> Result<MultiplexedConnection>;
    /// Set NATS connection
    fn set_nats(&self, nats: nats::asynk::Connection) -> Result<()>;
    /// Get NATS connection
    fn get_nats(&self) -> Result<nats::asynk::Connection>;

    /// Set publisher authorization token
    async fn set_pub_token(&self, room: String, user: String, token: String) -> Result<()>;
    /// Set subscriber authorization token
    async fn set_sub_token(&self, room: String, user: String, token: String) -> Result<()>;
    /// Get publisher authorization token
    async fn get_pub_token(&self, room: &str, user: &str) -> Result<String>;
    /// Get subscriber authorization token
    async fn get_sub_token(&self, room: &str, user: &str) -> Result<String>;

    async fn add_user_track_to_mime(&self, room: String, user: String, track: String, mime: String) -> Result<()>;
    async fn get_user_track_to_mime(&self, room: &str) -> Result<HashMap<(String, String), String>>;
    async fn remove_user_track_to_mime(&self, room: &str, user: &str) -> Result<()>;

    /// Add publisher to room publishers list
    async fn add_publisher(&self, room: &str, user: &str) -> Result<()>;
    /// Remove publisher from room publishers list
    async fn remove_publisher(&self, room: &str, user: &str) -> Result<()>;
    /// Fetch room publishers list
    async fn list_publishers(&self, room: &str) -> Result<HashSet<String>>;
    /// Check if specific publisher is in room publishers list
    async fn exist_publisher(&self, room: &str, user: &str) -> Result<bool>;

    /// Add subscriber to room subscribers list
    async fn add_subscriber(&self, room: &str, user: &str) -> Result<()>;
    /// remove subscriber from room subscribers list
    async fn remove_subscriber(&self, room: &str, user: &str) -> Result<()>;
    /// Fetch room subscribers list
    async fn list_subscribers(&self, room: &str) -> Result<HashSet<String>>;
    /// Check if specific subscriber is in room subscribers list
    async fn exist_subscriber(&self, room: &str, user: &str) -> Result<bool>;

    /// Set Sender (for other people to talk to specific subscriber)
    fn add_sub_notify(&self, room: &str, user: &str, sender: mpsc::Sender<Command>) -> Result<()>;
    /// Remove Sender (for other people to talk to specific subscriber)
    fn remove_sub_notify(&self, room: &str, user: &str) -> Result<()>;

    /// send cross instances commands, e.g. via NATS
    async fn send_command(&self, room: &str, cmd: Command) -> Result<()>;
    /// handling cross instances commands, e.g. via NATS
    async fn on_command(&self, room: &str, cmd: &[u8]) -> Result<()>;
    /// listen on cross instances commands, e.g. via NATS
    async fn listen_on_commands(&self) -> Result<()>;
    /// forward commands to each subscriber handler
    async fn forward_to_all_subs(&self, room: &str, msg: Command) -> Result<()>;
}

/// implement cross instances communication for NATS & Redis
#[async_trait]
impl SharedState for State {
    fn set_redis(&self, conn: MultiplexedConnection) -> Result<()> {
        let mut state = self.write().map_err(|e| anyhow!("Get global state as write failed: {}", e))?;
        state.redis = Some(conn);
        Ok(())
    }

    fn get_redis(&self) -> Result<MultiplexedConnection> {
        let state = self.read().map_err(|e| anyhow!("Get global state as read failed: {}", e))?;
        Ok(state.redis.as_ref().context("get Redis client failed")?.clone())
    }

    fn set_nats(&self, nats: nats::asynk::Connection) -> Result<()> {
        let mut state = self.write().map_err(|e| anyhow!("Get global state as write failed: {}", e))?;
        state.nats = Some(nats);
        Ok(())
    }

    fn get_nats(&self) -> Result<nats::asynk::Connection> {
        let state = self.read().map_err(|e| anyhow!("Get global state as read failed: {}", e))?;
        Ok(state.nats.as_ref().context("get NATS client failed")?.clone())
    }

    async fn set_pub_token(&self, room: String, user: String, token: String) -> Result<()> {
        // redis version:
        let mut conn = self.get_redis()?;
        let key = format!("token#pub#{}#{}", room, user);
        let _: Option<()> = conn.set(key.clone(), token).await.context("Redis set failed")?;
        // set Redis key TTL to 1 day
        let _: Option<()> = conn.expire(key, 24 * 60 * 60).await.context("Redis expire failed")?;
        Ok(())
    }

    async fn set_sub_token(&self, room: String, user: String, token: String) -> Result<()> {
        // redis version:
        let mut conn = self.get_redis()?;
        let key = format!("token#sub#{}#{}", room, user);
        let _: Option<()> = conn.set(key.clone(), token).await.context("Redis set failed")?;
        // set Redis key TTL to 1 day
        let _: Option<()> = conn.expire(key, 24 * 60 * 60).await.context("Redis expire failed")?;
        Ok(())
    }

    async fn get_pub_token(&self, room: &str, user: &str) -> Result<String> {
        // redis version:
        let mut conn = self.get_redis()?;
        let key = format!("token#pub#{}#{}", room, user);
        Ok(conn.get(&key).await.with_context(|| format!("can't get {} from Redis", key))?)
    }

    async fn get_sub_token(&self, room: &str, user: &str) -> Result<String> {
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
        let _: Option<()> = conn.hset(redis_key.clone(), hash_key, mime).await.context("Redis hset failed")?;
        // set Redis key TTL to 1 day
        let _: Option<()> = conn.expire(redis_key, 24 * 60 * 60).await.context("Redis expire failed")?;
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
            if let Some((user, track)) = it.next().zip(it.next()) {
                result.insert((user.to_string(), track.to_string()), v.to_string());
            }
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
        let _: Option<()> = conn.sadd(redis_key.clone(), user).await.context("Redis sadd failed")?;
        // set Redis key TTL to 1 day
        let _: Option<()> = conn.expire(redis_key, 24 * 60 * 60).await.context("Redis expire failed")?;
        Ok(())
    }

    async fn remove_publisher(&self, room: &str, user: &str) -> Result<()> {
        let mut conn = self.get_redis()?;
        let redis_key = format!("room#{}#pub_list", room);
        let _: Option<()> = conn.srem(redis_key.clone(), user).await.context("Redis sadd failed")?;
        // set Redis key TTL to 1 day
        let _: Option<()> = conn.expire(redis_key, 24 * 60 * 60).await.context("Redis expire failed")?;
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
        let _: Option<()> = conn.sadd(redis_key.clone(), user).await.context("Redis sadd failed")?;
        // set Redis key TTL to 1 day
        let _: Option<()> = conn.expire(redis_key, 24 * 60 * 60).await.context("Redis expire failed")?;
        Ok(())
    }

    async fn remove_subscriber(&self, room: &str, user: &str) -> Result<()> {
        let mut conn = self.get_redis()?;
        let redis_key = format!("room#{}#sub_list", room);
        let _: Option<()> = conn.srem(redis_key.clone(), user).await.context("Redis sadd failed")?;
        // set Redis key TTL to 1 day
        let _: Option<()> = conn.expire(redis_key, 24 * 60 * 60).await.context("Redis expire failed")?;
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

    fn add_sub_notify(&self, room: &str, user: &str, sender: mpsc::Sender<Command>) -> Result<()> {
        // TODO: convert this write lock to read lock, use field interior mutability
        let mut state = self.write().map_err(|e| anyhow!("Get global state as write failed: {}", e))?;
        let room = state.rooms.entry(room.to_string()).or_default();
        let user = room.subs.entry(user.to_string()).or_default();
        user.notify_message = Some(Arc::new(sender.clone()));
        Ok(())
    }

    fn remove_sub_notify(&self, room: &str, user: &str) -> Result<()> {
        // TODO: convert this write lock to read lock, use field interior mutability
        let mut state = self.write().map_err(|e| anyhow!("Get global state as write failed: {}", e))?;
        let room_obj = state.rooms.entry(room.to_string()).or_default();
        let _ = room_obj.subs.remove(user);
        // if there is no subscribers for this room
        // clean up the up layer hashmap too
        if room_obj.subs.is_empty() {
            let _ = state.rooms.remove(room);
        }
        Ok(())
    }

    async fn send_command(&self, room: &str, cmd: Command) -> Result<()> {
        let subject = format!("cmd.{}", room);
        let mut slice = [0u8; 64];
        let length = bincode::encode_into_slice(&cmd, &mut slice, Configuration::standard())
            .with_context(|| format!("encode command error: {:?}", cmd))?;
        let payload = &slice[..length];
        let nats = self.get_nats().context("get NATS client failed")?;
        nats.publish(&subject, payload).await.context("publish PUB_JOIN to NATS failed")?;
        Ok(())
    }

    async fn on_command(&self, room: &str, cmd: &[u8]) -> Result<()> {
        let cmd: Command = bincode::decode_from_slice(&cmd, Configuration::standard()).context("decode command error")?;
        info!("on cmd, room {} msg '{:?}'", room, cmd);
        match cmd {
            Command::PubJoin(_) => {
                self.forward_to_all_subs(room, cmd).await?;
            },
            Command::PubLeft(_) => {
                self.forward_to_all_subs(room, cmd).await?;
            },
        }
        Ok(())
    }

    async fn listen_on_commands(&self) -> Result<()> {
        let nats = self.get_nats().context("get NATS client failed")?;
        // cmd.ROOM
        // e.g. cmd.1234
        let subject = "cmd.*";
        let sub = nats.subscribe(subject).await.context("NATS subscribe for commands failed")?;

        async fn process(msg: nats::asynk::Message) -> Result<()> {
            let room = msg.subject.splitn(2, ".").skip(1).next()
                .context("extract room from NATS subject failed")?;
            SHARED_STATE.on_command(room, &msg.data).await?;
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

    async fn forward_to_all_subs(&self, room: &str, cmd: Command) -> Result<()> {
        let subs = {
            let state = self.read().map_err(|e| anyhow!("Get global state as read failed: {}", e))?;
            let room = match state.rooms.get(room) {
                Some(room) => room,
                None => return Ok(()),
            };
            room.subs
                .iter()
                .filter_map(|(_, sub)| sub.notify_message.clone())
                .collect::<Vec<_>>()
        };
        for sub in subs {
            let result = sub.send(cmd.clone()).await.with_context(|| format!("send {:?} to mpsc Sender failed", cmd));
            if let Err(err) = result {
                error!("{:?}", err);
            }
        }
        Ok(())
    }
}
