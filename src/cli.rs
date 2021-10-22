//! CLI setup, parse arguments for running service

use clap::Parser;


/// WebRTC SFU server
#[derive(Clone, Debug)]
#[derive(Parser)]
pub struct CliOptions {
    /// Web server host
    #[clap(short, long, env, default_value = "0.0.0.0")]
    pub host: String,

    /// Web server port
    #[clap(short, long, env, default_value = "8443")]
    pub port: String,

    /// NATS server URL
    #[clap(short, long, env, default_value = "localhost")]
    pub nats: String,

    /// STUN server URL
    #[clap(long, env, default_value = "stun:stun.l.google.com:19302")]
    pub stun: String,

    /// TURN server URL
    #[clap(short, long, env)]
    pub turn: Option<String>,

    /// TURN server username
    #[clap(long, env)]
    pub turn_username: Option<String>,

    /// TURN server password
    #[clap(long, env)]
    pub turn_password: Option<String>,

    /// SSL cert file
    #[clap(long, env, default_value = "cert.pem")]
    pub cert_file: String,

    /// SSL key file
    #[clap(long, env, default_value = "key.pem")]
    pub key_file: String,
}

/// parse CLI arguments & load env
pub fn get_args() -> CliOptions {
    // this will parse CLI args and construct CliOptions
    CliOptions::parse()
}
