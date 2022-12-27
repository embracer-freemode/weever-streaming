//! CLI setup, parse arguments for running service

use clap::Parser;

/// WebRTC SFU server
#[derive(Clone, Debug, Parser)]
pub struct CliOptions {
    /// CORS domain
    #[arg(long, env, default_value = "localhost")]
    pub cors_domain: String,

    /// Web server host
    #[arg(long, env, default_value = "0.0.0.0")]
    pub host: String,

    /// Web server port (Public)
    #[arg(short, long, env, default_value = "8443")]
    pub port: String,

    /// Private web server port (Management, Metrics, Probes)
    #[arg(long, env, default_value = "9443")]
    pub private_port: String,

    /// NATS server URL
    #[arg(short, long, env, default_value = "localhost")]
    pub nats: String,

    /// Redis server URL
    #[arg(short, long, env, default_value = "redis://127.0.0.1/")]
    pub redis: String,

    /// STUN server URL
    #[arg(long, env, default_value = "stun:stun.l.google.com:19302")]
    pub stun: String,

    /// TURN server URL
    #[arg(short, long, env)]
    pub turn: Option<String>,

    /// TURN server username
    #[arg(long, env)]
    pub turn_username: Option<String>,

    /// TURN server password
    #[arg(long, env)]
    pub turn_password: Option<String>,

    /// SSL cert file
    #[arg(long, env, default_value = "cert.pem")]
    pub cert_file: String,

    /// SSL key file
    #[arg(long, env, default_value = "key.pem")]
    pub key_file: String,

    /// assign external IP addresses of 1:1 (D)NAT
    #[arg(long, env)]
    pub public_ip: Option<String>,

    /// auth mode (token verification for publisher/subscriber)
    #[arg(long, env)]
    pub auth: bool,

    /// debug mode (demo site & public management API)
    #[arg(long, env)]
    pub debug: bool,
}

/// parse CLI arguments & load env
pub fn get_args() -> CliOptions {
    // this will parse CLI args and construct CliOptions
    CliOptions::parse()
}
