//! CLI setup, parse arguments for running service

use clap::Parser;


/// Janus Gateway test client
#[derive(Clone, Debug)]
#[derive(Parser)]
pub struct CliOptions {
    /// NATS server URL
    #[clap(short, long, env, default_value = "localhost")]
    pub nats: String,
    ///
    /// STUN server URL
    #[clap(long, env, default_value = "stun://stun.l.google.com:19302")]
    pub stun: String,

    /// TURN server URL
    #[clap(short, long, env)]
    pub turn: Option<String>,

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
