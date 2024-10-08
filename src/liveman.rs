use clap::Parser;
use tracing::{debug, info, warn};

mod helper;

#[derive(Parser)]
#[command(version)]
struct Args {
    /// Set config file path
    #[arg(short, long)]
    config: Option<String>,
}

#[tokio::main]
async fn main() {
    let args = Args::parse();
    let cfg = liveman::config::Config::parse(args.config);

    #[cfg(debug_assertions)]
    utils::set_log(format!(
        "liveman={},liveion={},http_log={},webrtc=error",
        cfg.log.level, cfg.log.level, cfg.log.level
    ));

    #[cfg(not(debug_assertions))]
    utils::set_log(format!(
        "liveman={},http_log={},webrtc=error",
        cfg.log.level, cfg.log.level
    ));

    warn!("set log level : {}", cfg.log.level);
    debug!("config : {:?}", cfg);

    let listener = tokio::net::TcpListener::bind(cfg.http.listen)
        .await
        .unwrap();
    info!("Server listening on {}", listener.local_addr().unwrap());

    liveman::server_up(cfg, listener, helper::shutdown_signal()).await;
    info!("Server shutdown");
}
