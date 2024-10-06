use opentelemetry::{global, trace::TracerProvider, KeyValue};
use opentelemetry_otlp::WithExportConfig;
use opentelemetry_sdk::trace::Tracer;
use opentelemetry_sdk::{runtime, trace as sdktrace, Resource};
use opentelemetry_semantic_conventions::resource::SERVICE_NAME;
use serde::{Deserialize, Serialize};

use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

use redis::aio::ConnectionManager;
use std::{collections::HashMap, sync::Arc};
use tokio::sync::Mutex;

use uuid::Uuid;
pub mod handlers;

#[derive(Clone)]
pub struct Appstate {
    pub conn: ConnectionManager,
    pub socket_to_tx: Arc<Mutex<HashMap<Uuid, flume::Sender<WorkerMessage>>>>,
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "lowercase")]
pub enum CommandType {
    Enqueue,
    Processed,
}
#[derive(Serialize, Deserialize, Debug)]
pub struct ClientMessage {
    pub command: CommandType,
    pub count: usize,
    pub mid: usize,
}
// { "command": "enqueue", "count": 100, "mid": 12 }

#[derive(Serialize, Deserialize, Debug)]
pub struct WorkerMessage {
    pub command: CommandType,
    pub result_idx: usize,
    pub mid: usize,
    pub cid: Uuid,
}

pub fn init_otel_tracer(resource_name: &'static str) -> anyhow::Result<Tracer> {
    let provider = opentelemetry_otlp::new_pipeline()
        .tracing()
        .with_exporter(
            opentelemetry_otlp::new_exporter()
                .tonic()
                .with_endpoint("http://localhost:4317"),
        )
        .with_trace_config(
            sdktrace::Config::default().with_resource(Resource::new(vec![KeyValue::new(
                SERVICE_NAME,
                resource_name,
            )])),
        )
        .install_batch(runtime::Tokio)?;

    global::set_tracer_provider(provider.clone());
    Ok(provider.tracer(resource_name))
}

pub fn init_tracing(otel_tracer: Tracer) {
    let otel_layer = tracing_opentelemetry::layer().with_tracer(otel_tracer);
    tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::from_default_env())
        .with(tracing_subscriber::fmt::layer())
        .with(otel_layer)
        .init()
}
