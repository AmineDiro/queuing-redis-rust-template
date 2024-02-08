use std::{thread::sleep, time::Duration};

use rand::{self, rngs::ThreadRng, Rng};
use redis::{Client, Commands, ErrorKind, FromRedisValue, RedisResult};
use uuid::Uuid;

struct Message {
    _id: Uuid,
    _data: Vec<u8>,
}

impl FromRedisValue for Message {
    fn from_redis_value(v: &redis::Value) -> redis::RedisResult<Self> {
        match v {
            redis::Value::Data(data) => {
                return Ok(Message {
                    _id: Uuid::new_v4(),
                    _data: data.to_vec(),
                })
            }
            _ => RedisResult::Err((ErrorKind::TypeError, "Can't build Message from nil ").into()),
        }
    }
}

fn process_task<'a>(rng: &mut ThreadRng, msg: &'a str) -> anyhow::Result<()> {
    tracing::info!("Received msg : {}", msg);
    let work_time = rng.gen_range(300..500);
    sleep(Duration::from_millis(work_time));
    Ok(())
    // Send POST request to the backend
}

fn main() -> Result<(), anyhow::Error> {
    tracing_subscriber::fmt::init();
    let client = Client::open("redis://localhost:6379")?;

    let mut con = client.get_connection()?;
    let mut rng = rand::thread_rng();
    loop {
        let msg: Vec<String> = con.brpop("queue", 0.5)?;
        if msg.len() > 0 {
            process_task(&mut rng, &msg[1])?;
        }
    }
}
