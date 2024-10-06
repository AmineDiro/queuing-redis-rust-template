use axum::extract::MatchedPath;
use axum::http::Request;
use opentelemetry::global;

use redis_queue::{init_otel_tracer, init_tracing};
use tracing::info_span;

use axum::{
    routing::{get, post},
    Router,
};

use axum_tracing_opentelemetry::middleware::OtelAxumLayer;
use tower_http::trace::TraceLayer;

use redis::{aio::ConnectionManager, Client};
use std::{collections::HashMap, net::SocketAddr, sync::Arc};
use tokio::sync::Mutex;

use tower_http::services::ServeDir;

use redis_queue::handlers::{handle_worker, healthz, ws_handler};
use redis_queue::Appstate;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let tracer = init_otel_tracer("queuing-server").expect("can't init otel tracer");
    init_tracing(tracer);

    let client = Client::open("redis://localhost:6379")?;
    let conn = ConnectionManager::new(client).await?;
    let state = Appstate {
        conn,
        socket_to_tx: Arc::new(Mutex::new(HashMap::new())),
    };

    let app = Router::new()
        .route("/healthz", get(healthz))
        .route("/processed", post(handle_worker))
        .route("/enqueue", get(ws_handler))
        .with_state(state)
        .fallback_service(ServeDir::new("assets/").append_index_html_on_directories(true))
        .layer(
            // TraceLayer::new_for_http().make_span_with(DefaultMakeSpan::new().include_headers(true)),
            TraceLayer::new_for_http().make_span_with(|request: &Request<_>| {
                let matched_path = request
                    .extensions()
                    .get::<MatchedPath>()
                    .map(MatchedPath::as_str);

                info_span!(
                    "http_request",
                    method = ?request.method(),
                    matched_path,
                    some_other_field = tracing::field::Empty,
                )
            }),
        )
        .layer(OtelAxumLayer::default());

    let listener = tokio::net::TcpListener::bind("127.0.0.1:3000")
        .await
        .unwrap();

    tracing::info!("listening on {}", listener.local_addr().unwrap());
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .await?;
    global::shutdown_tracer_provider();
    Ok(())
}
