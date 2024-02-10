use serde::{Deserialize, Serialize};

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
// {
// "command": "processed",
// "result": {
// "idx": idx
// },
// "mid": messageId
// }

#[derive(Serialize, Deserialize, Debug)]
pub struct WorkerMessage {
    command: CommandType,
    result_idx: usize,
    mid: usize,
}
