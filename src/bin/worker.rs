use opentelemetry::propagation::TextMapPropagator;
use opentelemetry::Context;
use opentelemetry_sdk::propagation::TraceContextPropagator;
use std::{str::FromStr, thread::sleep, time::Duration};
use tracing_opentelemetry::OpenTelemetrySpanExt;

use rand::{self, rngs::ThreadRng, Rng};
use redis::{Client, Commands};
use redis_queue::{init_otel_tracer, init_tracing, CommandType, WorkerMessage};
use tracing::instrument;
use uuid::Uuid;

#[instrument(skip(rng, client))]
fn process_task<'a>(
    rng: &mut ThreadRng,
    client: &reqwest::blocking::Client,
    cid: Uuid,
    mid: usize,
    idx: usize,
) -> anyhow::Result<()> {
    let work_time = rng.gen_range(1..10);
    tracing::info!("Started processing task");
    sleep(Duration::from_millis(work_time));
    // Send POST request to the backend
    let msg = WorkerMessage {
        command: CommandType::Processed,
        cid,
        mid,
        result_idx: idx,
    };
    tracing::info!(msg = ?msg, "Sending worker message to client.");
    client
        .post("http://localhost:3000/processed")
        .json(&msg)
        .send()?;
    Ok(())
}

fn run_worker() {
    tracing::info!("Started worker. Waiting for work..");
    let client = Client::open("redis://localhost:6379").expect("can't connect to redis");
    let mut con = client.get_connection().expect("can't get connection");

    let http_client = reqwest::blocking::Client::new();

    let mut rng = rand::thread_rng();
    loop {
        let msg: Vec<String> = con.brpop("queue", 0.5).unwrap();
        if msg.len() >= 1 {
            let (client_id, mid, idx, span_ctx_json) = parse_msg(&msg).expect("error parsing msg");

            let parent_ctx = extract_span_ctx(&span_ctx_json).expect("can't parse parent_ctx");

            let span =
                tracing::span!(tracing::Level::INFO, "process_task",client_id=?client_id, mid, idx);
            span.set_parent(parent_ctx);

            let _guard = span.enter();
            process_task(&mut rng, &http_client, client_id, mid, idx)
                .expect("error processing task");
        }
    }
}

#[tokio::main]
async fn main() {
    let tracer = init_otel_tracer("queuing-worker").expect("can't init otel tracer");
    init_tracing(tracer);

    tokio::task::spawn_blocking(move || run_worker())
        .await
        .unwrap();
}

fn parse_msg(msg: &[String]) -> anyhow::Result<(Uuid, usize, usize, String)> {
    let parts: Vec<String> = msg[1].splitn(4, ':').map(|e| e.to_string()).collect();
    let client_id = Uuid::from_str(&parts[0])?;
    let mid = &parts[1].parse::<usize>()?;
    let idx = &parts[2].parse::<usize>()?;
    let span_context_json = &parts[3];

    Ok((client_id, *mid, *idx, span_context_json.to_owned()))
}

fn extract_span_ctx(span_ctx_json: &str) -> anyhow::Result<Context> {
    let span_context_map: std::collections::HashMap<String, String> =
        serde_json::from_str(span_ctx_json)?;
    let propagator = TraceContextPropagator::new();
    Ok(propagator.extract(&span_context_map))
}
