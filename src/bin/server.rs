use axum::{
    extract::{ws::WebSocketUpgrade, State},
    response::IntoResponse,
    routing::{get, post},
    Router,
};
use axum_tracing_opentelemetry::middleware::OtelAxumLayer;
use tower_http::trace::{DefaultMakeSpan, TraceLayer};
use tracing::{instrument, Instrument};

use opentelemetry::global;
use redis::{aio::ConnectionManager, Client};
use std::{collections::HashMap, net::SocketAddr, sync::Arc};
use tokio::sync::Mutex;
use tracing::{self, Level};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};
use uuid::Uuid;

use tower_http::services::ServeDir;

use axum::extract::connect_info::ConnectInfo;

use opentelemetry::runtime::Tokio;
use opentelemetry::sdk::propagation::TraceContextPropagator;
use redis_queue::{
    handlers::{handle_socket, handle_worker},
    Appstate,
};

async fn healthz() -> &'static str {
    "Ok"
}

#[instrument(skip(state, ws))]
async fn ws_handler(
    ws: WebSocketUpgrade,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    State(state): State<Appstate>,
) -> impl IntoResponse {
    // finalize the upgrade process by returning upgrade callback.
    let client_id = Uuid::new_v4();
    let mut hashmap = state.socket_to_tx.lock().await;
    let (tx, rx) = flume::unbounded();
    hashmap.insert(client_id, tx);
    tracing::info!(client_id = &client_id.to_string(), "{addr} connected. ");
    // ws.on_upgrade(move |socket| handle_socket(socket, addr, client_id, state.conn, rx))
    let span = tracing::Span::current();
    ws.on_upgrade(move |socket| {
        let span = span.clone();
        async move {
            handle_socket(socket, addr, client_id, state.conn, rx)
                .instrument(span)
                .await;
        }
    })
}
fn init_tracing() {
    axum_tracing_opentelemetry::tracing_subscriber_ext::init_subscribers()?;

    // Create a Jaeger pipeline
    let tracer = opentelemetry_jaeger::new_agent_pipeline()
        .with_service_name("redis-server")
        .install_batch(Tokio)
        .expect("Error initializing Jaeger exporter");

    // Initialize tracing subscriber
    tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::from_default_env())
        .with(tracing_subscriber::fmt::layer())
        .with(tracing_opentelemetry::layer().with_tracer(tracer))
        .init();
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    init_tracing();

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
            TraceLayer::new_for_http().make_span_with(DefaultMakeSpan::new().include_headers(true)),
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
