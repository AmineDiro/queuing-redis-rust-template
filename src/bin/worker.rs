use std::{str::FromStr, thread::sleep, time::Duration};

use opentelemetry::global;
use rand::{self, rngs::ThreadRng, Rng};
use redis::{Client, Commands};
use redis_queue::{CommandType, WorkerMessage};
use tracing::instrument;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};
use uuid::Uuid;

#[instrument(skip(rng, client))]
fn process_task<'a>(
    rng: &mut ThreadRng,
    client: &reqwest::blocking::Client,
    msg: &'a str,
) -> anyhow::Result<()> {
    tracing::debug!("Received msg : {:?}", &msg);
    let work_time = rng.gen_range(1..10);
    sleep(Duration::from_millis(work_time));
    // Send POST request to the backen'
    let parts: Vec<String> = msg.splitn(3, ':').map(|e| e.to_string()).collect();

    let msg = WorkerMessage {
        command: CommandType::Processed,
        result_idx: parts[2].parse::<usize>()?,
        mid: parts[1].parse::<usize>()?,
        cid: Uuid::from_str(&parts[0])?,
    };
    tracing::debug!(msg = ?msg, "Sending worker message to client.");
    client
        .post("http://localhost:3000/processed")
        .json(&msg)
        .send()?;
    Ok(())
}

fn main() -> Result<(), anyhow::Error> {
    global::set_text_map_propagator(opentelemetry_zipkin::Propagator::new());
    let fmt_subscriber = tracing_subscriber::fmt::layer();
    let tracer = opentelemetry_zipkin::new_pipeline()
        .with_service_name("redis-worker")
        .with_service_address("127.0.0.1:3000".parse().unwrap())
        .with_collector_endpoint("http://localhost:9411/api/v2/spans")
        .install_simple()
        .expect("unable to install zipkin tracer");
    let tracer = tracing_opentelemetry::layer().with_tracer(tracer);

    let registry = tracing_subscriber::registry()
        .with(fmt_subscriber)
        .with(tracer)
        .with(EnvFilter::try_from_default_env().unwrap_or_else(|_| "worker=debug".into()));
    registry.init();

    tracing::info!("Started worker. Waiting for work..");
    let client = Client::open("redis://localhost:6379")?;
    let mut con = client.get_connection()?;

    let http_client = reqwest::blocking::Client::new();

    let mut rng = rand::thread_rng();
    loop {
        let msg: Vec<String> = con.brpop("queue", 0.5)?;
        if msg.len() >= 1 {
            process_task(&mut rng, &http_client, &msg[1])?;
        }
    }

    global::shutdown_tracer_provider();
}
