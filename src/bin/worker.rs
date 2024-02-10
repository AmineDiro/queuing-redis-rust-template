use std::{str::FromStr, thread::sleep, time::Duration};

use rand::{self, rngs::ThreadRng, Rng};
use redis::{Client, Commands};
use redis_queue::{CommandType, WorkerMessage};
use reqwest;
use tracing::instrument;
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
    // Send POST request to the backend
    let parts: Vec<String> = msg.splitn(3, ":").map(|e| e.to_string()).collect();

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
    tracing_subscriber::fmt::init();
    tracing::info!("Started worker. Waiting for work..");
    let client = Client::open("redis://localhost:6379")?;
    let mut con = client.get_connection()?;

    let http_client = reqwest::blocking::Client::new();

    let mut rng = rand::thread_rng();
    loop {
        let msg: Vec<String> = con.brpop("queue", 0.5)?;
        if msg.len() > 0 {
            process_task(&mut rng, &http_client, &msg[1])?;
        }
    }
}
