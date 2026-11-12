/*!
 * tharhosting server entry point.
 *
 * Loads `.env` (if present), reads configuration, prepares the data
 * directories, opens the database, recovers deployments interrupted by a
 * previous run, and then serves the API until Ctrl-C.
 */

use tharhosting::config::Config;
use tharhosting::db::store::Store;
use tharhosting::state::AppState;

/**
 * Starts the server. Exits with a non-zero code on any startup error: 2 for
 * invalid configuration, 1 for anything else.
 */
#[tokio::main]
async fn main() {
    // Load `.env` if it exists. Variables already set in the real environment
    // win, and a missing file is fine. This runs first so `RUST_LOG` can be set
    // in the file too.
    dotenvy::dotenv().ok();

    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    let config = match Config::from_env() {
        Ok(c) => c,
        Err(e) => {
            log::error!("{e}");
            std::process::exit(2);
        }
    };

    for dir in [config.logs_dir(), config.builds_dir()] {
        if let Err(e) = std::fs::create_dir_all(&dir) {
            log::error!("cannot create {}: {e}", dir.display());
            std::process::exit(1);
        }
    }

    let store = match Store::open(config.db_path()) {
        Ok(s) => s,
        Err(e) => {
            log::error!("cannot open {}: {e}", config.db_path().display());
            std::process::exit(1);
        }
    };
    match store.write(|d| d.recover_interrupted()) {
        Ok(0) => {}
        Ok(n) => log::warn!("marked {n} interrupted deployment(s) as failed"),
        Err(e) => log::error!("cannot write db: {e}"),
    }

    if config.api_token.is_none() {
        log::warn!("THARHOSTING_TOKEN not set: the API is unauthenticated");
    }
    if config.webhook_secret.is_none() {
        log::warn!("THARHOSTING_WEBHOOK_SECRET not set: webhooks are not verified");
    }

    let addr = format!("{}:{}", config.host, config.port);
    log::info!("data dir: {}", config.data_dir.display());
    
    // Build the api router.
    let app = tharhosting::api::router(AppState::new(config, store));
    let address_binding = tokio::net::TcpListener::bind(&addr).await;
    let listener = match address_binding {
        Ok(tcp) => tcp,
        Err(e) => {
            log::error!("cannot bind {addr}: {e}");
            std::process::exit(1);
        }
    };
    log::info!("tharhosting listening on http://{addr}");
    axum::serve(listener, app)
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
            log::info!("shutting down");
        })
        .await
        .expect("server error");
}
