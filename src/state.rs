//! Sharing state for whole program.
//!
//! The publisher metadata will share across instances (via Redis).
//! The subscribe metadata will be per instance local only.
//!
//! We can implement multiple state mechanism as long as it fit the SharedState trait.

use crate::helper::catch;
use anyhow::{anyhow, Context, Result};
use async_trait::async_trait;
use bincode::{config::standard, Decode, Encode};
use log::{error, info};
use once_cell::sync::Lazy;
use redis::aio::MultiplexedConnection;
use redis::AsyncCommands;
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, RwLock};
use tokio::sync::mpsc;

/// Command that we will send across instances.
/// And we will send some of them to frontend after serialization.
#[derive(Decode, Encode, Debug, Clone)]
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
    pub rooms: HashMap<String, Room>,
    nats: Option<async_nats::Client>,
    redis: Option<MultiplexedConnection>,
}

/// Room metadata
#[derive(Debug, Default)]
pub struct Room {
    pub subs: HashMap<String, PeerConnetionInfo>,
    pub pubs: HashSet<String>, // for local metrics generation
}

#[derive(Debug, Default)]
pub struct PeerConnetionInfo {
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
    fn set_nats(&self, nats: async_nats::Client) -> Result<()>;
    /// Get NATS connection
    fn get_nats(&self) -> Result<async_nats::Client>;

    /// Set publisher authorization token
    async fn set_pub_token(&self, room: String, user: String, token: String) -> Result<()>;
    /// Set subscriber authorization token
    async fn set_sub_token(&self, room: String, user: String, token: String) -> Result<()>;
    /// Get publisher authorization token
    async fn get_pub_token(&self, room: &str, user: &str) -> Result<String>;
    /// Get subscriber authorization token
    async fn get_sub_token(&self, room: &str, user: &str) -> Result<String>;

    async fn add_user_media_count(&self, room: &str, user: &str, mime: &str) -> Result<()>;
    async fn get_users_media_count(&self, room: &str) -> Result<HashMap<(String, String), u8>>;
    async fn remove_user_media_count(&self, room: &str, user: &str) -> Result<()>;

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

    /// check if we still have clients
    async fn has_peers(&self) -> bool;
}

/// implement cross instances communication for NATS & Redis
#[async_trait]
impl SharedState for State {
    fn set_redis(&self, conn: MultiplexedConnection) -> Result<()> {
        let mut state = self
            .write()
            .map_err(|e| anyhow!("Get global state as write failed: {}", e))?;
        state.redis = Some(conn);
        Ok(())
    }

    fn get_redis(&self) -> Result<MultiplexedConnection> {
        let state = self
            .read()
            .map_err(|e| anyhow!("Get global state as read failed: {}", e))?;
        Ok(state
            .redis
            .as_ref()
            .context("get Redis client failed")?
            .clone())
    }

    fn set_nats(&self, nats: async_nats::Client) -> Result<()> {
        let mut state = self
            .write()
            .map_err(|e| anyhow!("Get global state as write failed: {}", e))?;
        state.nats = Some(nats);
        Ok(())
    }

    fn get_nats(&self) -> Result<async_nats::Client> {
        let state = self
            .read()
            .map_err(|e| anyhow!("Get global state as read failed: {}", e))?;
        Ok(state
            .nats
            .as_ref()
            .context("get NATS client failed")?
            .clone())
    }

    async fn set_pub_token(&self, room: String, user: String, token: String) -> Result<()> {
        // redis version:
        let mut conn = self.get_redis()?;
        let key = format!("token#pub#{}#{}", room, user);
        let _: Option<()> = conn
            .set(key.clone(), token)
            .await
            .context("Redis set failed")?;
        // set Redis key TTL to 1 day
        let _: Option<()> = conn
            .expire(key, 24 * 60 * 60)
            .await
            .context("Redis expire failed")?;
        Ok(())
    }

    async fn set_sub_token(&self, room: String, user: String, token: String) -> Result<()> {
        // redis version:
        let mut conn = self.get_redis()?;
        let key = format!("token#sub#{}#{}", room, user);
        let _: Option<()> = conn
            .set(key.clone(), token)
            .await
            .context("Redis set failed")?;
        // set Redis key TTL to 1 day
        let _: Option<()> = conn
            .expire(key, 24 * 60 * 60)
            .await
            .context("Redis expire failed")?;
        Ok(())
    }

    async fn get_pub_token(&self, room: &str, user: &str) -> Result<String> {
        // redis version:
        let mut conn = self.get_redis()?;
        let key = format!("token#pub#{}#{}", room, user);
        Ok(conn
            .get(&key)
            .await
            .with_context(|| format!("can't get {} from Redis", key))?)
    }

    async fn get_sub_token(&self, room: &str, user: &str) -> Result<String> {
        // redis version:
        let mut conn = self.get_redis()?;
        let key = format!("token#sub#{}#{}", room, user);
        Ok(conn
            .get(&key)
            .await
            .with_context(|| format!("can't get {} from Redis", key))?)
    }

    async fn add_user_media_count(&self, room: &str, user: &str, mime: &str) -> Result<()> {
        let mut conn = self.get_redis()?;
        let redis_key = format!("room#{}#media", room);
        let key = format!("{}#{}", user, mime); // e.g. video, audio
        let _: Option<()> = conn
            .hincr(&redis_key, key, 1)
            .await
            .context("Redis hincr failed")?;
        // set Redis key TTL to 1 day
        let _: Option<()> = conn
            .expire(&redis_key, 24 * 60 * 60)
            .await
            .context("Redis expire failed")?;
        Ok(())
    }

    async fn get_users_media_count(&self, room: &str) -> Result<HashMap<(String, String), u8>> {
        let mut conn = self.get_redis()?;
        let redis_key = format!("room#{}#media", room);
        let media: Vec<(String, u8)> = conn
            .hgetall(&redis_key)
            .await
            .context("Redis hgetall failed")?;
        let result = media
            .into_iter()
            .filter_map(|(k, v)| {
                let mut it = k.splitn(2, "#");
                if let Some((user, mime)) = it.next().zip(it.next()) {
                    return Some(((user.to_string(), mime.to_string()), v));
                }
                None
            })
            .collect();
        // set Redis key TTL to 1 day
        let _: Option<()> = conn
            .expire(&redis_key, 24 * 60 * 60)
            .await
            .context("Redis expire failed")?;
        Ok(result)
    }

    async fn remove_user_media_count(&self, room: &str, user: &str) -> Result<()> {
        let mut conn = self.get_redis()?;
        let redis_key = format!("room#{}#media", room);
        let video_key = format!("{}#{}", user, "video");
        let audio_key = format!("{}#{}", user, "audio");
        let _: Option<()> = conn
            .hdel(&redis_key, video_key)
            .await
            .context("Redis hdel failed")?;
        let _: Option<()> = conn
            .hdel(&redis_key, audio_key)
            .await
            .context("Redis hdel failed")?;
        // set Redis key TTL to 1 day
        let _: Option<()> = conn
            .expire(&redis_key, 24 * 60 * 60)
            .await
            .context("Redis expire failed")?;
        Ok(())
    }

    async fn add_publisher(&self, room: &str, user: &str) -> Result<()> {
        let mut conn = self.get_redis()?;
        let redis_key = format!("room#{}#pub_list", room);
        let _: Option<()> = conn
            .sadd(&redis_key, user)
            .await
            .context("Redis sadd failed")?;
        // set Redis key TTL to 1 day
        let _: Option<()> = conn
            .expire(&redis_key, 24 * 60 * 60)
            .await
            .context("Redis expire failed")?;

        // local state (for metrics)
        let mut state = self
            .write()
            .map_err(|e| anyhow!("Get global state as write failed: {}", e))?;
        let room = state.rooms.entry(room.to_string()).or_default();
        room.pubs.insert(user.to_string());

        Ok(())
    }

    async fn remove_publisher(&self, room: &str, user: &str) -> Result<()> {
        let mut conn = self.get_redis()?;
        let redis_key = format!("room#{}#pub_list", room);
        let _: Option<()> = conn
            .srem(&redis_key, user)
            .await
            .context("Redis sadd failed")?;
        // set Redis key TTL to 1 day
        let _: Option<()> = conn
            .expire(&redis_key, 24 * 60 * 60)
            .await
            .context("Redis expire failed")?;

        // local state (for metrics)
        let mut state = self
            .write()
            .map_err(|e| anyhow!("Get global state as write failed: {}", e))?;
        let room_obj = state.rooms.entry(room.to_string()).or_default();
        room_obj.pubs.remove(user);
        // if there is no clients for this room
        // clean up the up layer hashmap too
        if room_obj.pubs.is_empty() & room_obj.subs.is_empty() {
            let _ = state.rooms.remove(room);
        }

        Ok(())
    }

    async fn list_publishers(&self, room: &str) -> Result<HashSet<String>> {
        let mut conn = self.get_redis()?;
        let redis_key = format!("room#{}#pub_list", room);
        let result: HashSet<String> = conn
            .smembers(redis_key)
            .await
            .context("Redis smembers failed")?;
        Ok(result)
    }

    async fn exist_publisher(&self, room: &str, user: &str) -> Result<bool> {
        let mut conn = self.get_redis()?;
        let redis_key = format!("room#{}#pub_list", room);
        let result: bool = conn
            .sismember(redis_key, user)
            .await
            .context("Redis sismember failed")?;
        Ok(result)
    }

    async fn add_subscriber(&self, room: &str, user: &str) -> Result<()> {
        let mut conn = self.get_redis()?;
        let redis_key = format!("room#{}#sub_list", room);
        let _: Option<()> = conn
            .sadd(&redis_key, user)
            .await
            .context("Redis sadd failed")?;
        // set Redis key TTL to 1 day
        let _: Option<()> = conn
            .expire(&redis_key, 24 * 60 * 60)
            .await
            .context("Redis expire failed")?;
        Ok(())
    }

    async fn remove_subscriber(&self, room: &str, user: &str) -> Result<()> {
        let mut conn = self.get_redis()?;
        let redis_key = format!("room#{}#sub_list", room);
        let _: Option<()> = conn
            .srem(&redis_key, user)
            .await
            .context("Redis sadd failed")?;
        // set Redis key TTL to 1 day
        let _: Option<()> = conn
            .expire(&redis_key, 24 * 60 * 60)
            .await
            .context("Redis expire failed")?;
        Ok(())
    }

    async fn list_subscribers(&self, room: &str) -> Result<HashSet<String>> {
        let mut conn = self.get_redis()?;
        let redis_key = format!("room#{}#sub_list", room);
        let result: HashSet<String> = conn
            .smembers(redis_key)
            .await
            .context("Redis smembers failed")?;
        Ok(result)
    }

    async fn exist_subscriber(&self, room: &str, user: &str) -> Result<bool> {
        let mut conn = self.get_redis()?;
        let redis_key = format!("room#{}#sub_list", room);
        let result: bool = conn
            .sismember(redis_key, user)
            .await
            .context("Redis sismember failed")?;
        Ok(result)
    }

    fn add_sub_notify(&self, room: &str, user: &str, sender: mpsc::Sender<Command>) -> Result<()> {
        // TODO: convert this write lock to read lock, use field interior mutability
        let mut state = self
            .write()
            .map_err(|e| anyhow!("Get global state as write failed: {}", e))?;
        let room = state.rooms.entry(room.to_string()).or_default();
        let user = room.subs.entry(user.to_string()).or_default();
        user.notify_message = Some(Arc::new(sender.clone()));
        Ok(())
    }

    fn remove_sub_notify(&self, room: &str, user: &str) -> Result<()> {
        // TODO: convert this write lock to read lock, use field interior mutability
        let mut state = self
            .write()
            .map_err(|e| anyhow!("Get global state as write failed: {}", e))?;
        let room_obj = state.rooms.entry(room.to_string()).or_default();
        let _ = room_obj.subs.remove(user);
        // if there is no clients for this room
        // clean up the up layer hashmap too
        if room_obj.pubs.is_empty() & room_obj.subs.is_empty() {
            let _ = state.rooms.remove(room);
        }
        Ok(())
    }

    async fn send_command(&self, room: &str, cmd: Command) -> Result<()> {
        let subject = format!("cmd.{}", room);
        let mut slice = [0u8; 64];
        let length = bincode::encode_into_slice(&cmd, &mut slice, standard())
            .with_context(|| format!("encode command error: {:?}", cmd))?;
        let payload = &slice[..length];
        let nats = self.get_nats().context("get NATS client failed")?;
        // TODO: avoid copy
        nats.publish(subject, payload.to_owned().into())
            .await
            .context("publish PUB_JOIN to NATS failed")?;
        Ok(())
    }

    async fn on_command(&self, room: &str, cmd: &[u8]) -> Result<()> {
        let (cmd, _) =
            bincode::decode_from_slice(&cmd, standard()).context("decode command error")?;
        info!("on cmd, room {} msg '{:?}'", room, cmd);
        match cmd {
            Command::PubJoin(_) => {
                self.forward_to_all_subs(room, cmd).await?;
            }
            Command::PubLeft(_) => {
                self.forward_to_all_subs(room, cmd).await?;
            }
        }
        Ok(())
    }

    async fn listen_on_commands(&self) -> Result<()> {
        let nats = self.get_nats().context("get NATS client failed")?;
        // cmd.ROOM
        // e.g. cmd.1234
        let subject = "cmd.*";
        let mut sub = nats
            .subscribe(subject.to_string())
            .await
            .map_err(|_| anyhow!("NATS subscribe for commands failed"))?;

        async fn process(msg: async_nats::Message) -> Result<()> {
            let room = msg
                .subject
                .splitn(2, ".")
                .skip(1)
                .next()
                .context("extract room from NATS subject failed")?;
            SHARED_STATE.on_command(room, &msg.payload).await?;
            Ok(())
        }

        tokio::spawn(async move {
            use futures::StreamExt;
            // TODO: will we exit this loop when disconnect?
            while let Some(msg) = sub.next().await {
                tokio::spawn(catch(process(msg)));
            }
        });

        Ok(())
    }

    async fn forward_to_all_subs(&self, room: &str, cmd: Command) -> Result<()> {
        let subs = {
            let state = self
                .read()
                .map_err(|e| anyhow!("Get global state as read failed: {}", e))?;
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
            let result = sub
                .send(cmd.clone())
                .await
                .with_context(|| format!("send {:?} to mpsc Sender failed", cmd));
            if let Err(err) = result {
                error!("{:?}", err);
            }
        }
        Ok(())
    }

    async fn has_peers(&self) -> bool {
        let state = match self.read() {
            Err(_) => return true,
            Ok(state) => state,
        };
        !state.rooms.is_empty()
    }
}
