//! WebRTC SFU with horizontal scale design

use anyhow::{Context, Result};
use log::debug;
use tracing_subscriber::{fmt, layer::SubscriberExt, EnvFilter};

pub mod cli;
pub mod helper;
pub mod publisher;
pub mod state;
pub mod subscriber;
pub mod web;

/// entrypoint for whole program
fn main() -> Result<()> {
    // CLI
    let args = cli::get_args();
    debug!("CLI args: {:?}", args);

    // logger
    // bridge "log" crate and "tracing" crate
    tracing_log::LogTracer::init()?;
    // create "logs" dir if not exist
    if !std::path::Path::new("./logs").is_dir() {
        std::fs::create_dir("logs")?;
    }
    // logfile writer
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_millis();
    let file = format!("rtc.{}.log", now);
    let file_appender = tracing_appender::rolling::never("logs", file);
    let (non_blocking, _guard) = tracing_appender::non_blocking(file_appender);
    // compose our complex logger
    // 1. filter via RUST_LOG env
    // 2. output to stdout
    // 3. output to logfile
    //
    // use "console_subscriber::build()"
    // instead of "tracing_subscriber::registry()"
    // for tokio-console
    #[cfg(not(console))]
    let subscriber = tracing_subscriber::registry();
    #[cfg(console)]
    let subscriber = console_subscriber::build();

    let subscriber = subscriber
        .with(EnvFilter::from_default_env()) // RUST_LOG env filter
        .with(fmt::Layer::new().with_writer(std::io::stdout))
        .with(fmt::Layer::new().with_writer(non_blocking));
    // set our logger as global default
    tracing::subscriber::set_global_default(subscriber)
        .context("Unable to set global collector")?;

    web::web_main(args)?;

    Ok(())
}
