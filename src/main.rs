use axum::{routing::{get, post}, Router};
use std::{net::SocketAddr, sync::Arc, time::Duration};
use tower_http::{cors::CorsLayer, services::ServeDir, trace::TraceLayer};
use tracing::info;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

mod error;
mod handlers;
mod models;

use models::app_state::AppState;

#[tokio::main]
async fn main() {
    // 优化：更灵活的日志配置
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "rustdemo=debug,tower_http=debug".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    let state = Arc::new(AppState::new());

    let cwd = std::env::current_dir().unwrap_or_default();
    info!("serving static files from: {:?}", cwd);

    let app = Router::new()
        .route("/", get(handlers::root))
        .route("/health", get(handlers::health))
        .route("/sum", get(handlers::sum))
        .route("/echo", post(handlers::echo))
        .route("/parallel", get(handlers::parallel))
        .route("/metrics", get(handlers::metrics))
        .fallback_service(ServeDir::new(cwd))
        .with_state(state)
        .layer(CorsLayer::permissive())
        .layer(TraceLayer::new_for_http());

    let addr = SocketAddr::from(([127, 0, 0, 1], 3000));
    info!("listening on http://{}", addr);
    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await
        .unwrap();
}

async fn shutdown_signal() {
    let _ = tokio::signal::ctrl_c().await;
}
