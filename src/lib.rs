use serde::{Deserialize, Serialize};
use uuid::Uuid;

use redis::aio::ConnectionManager;
pub mod handlers;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;

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
