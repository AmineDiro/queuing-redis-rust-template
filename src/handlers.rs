use crate::{Appstate, ClientMessage, WorkerMessage};
use axum::{
    extract::{
        ws::{Message, WebSocket},
        State,
    },
    http::StatusCode,
    Json,
};
use flume::Receiver;
use futures::{stream::StreamExt, SinkExt};
use redis::aio::ConnectionManager;
use std::{net::SocketAddr, ops::ControlFlow};
use tracing::{self, Span};
use uuid::Uuid;

// Websocket statemachine (one will be spawned per connection)
pub async fn handle_socket(
    span: Span,
    socket: WebSocket,
    peer_addr: SocketAddr,
    client_id: Uuid,
    mut conn: ConnectionManager,
    rx: Receiver<WorkerMessage>,
) {
    let (mut sender, mut receiver) = socket.split();
    loop {
        tokio::select! {
                client_msg = receiver.next() => {
                    if let Some(Ok(msg)) = client_msg {
                        // Process ws message
                        match process_ws_message(msg, peer_addr,client_id) {
                            ControlFlow::Continue(Some(msg)) => {
                                // Enqueue to redis
                                tracing::trace!(parent:&span, "enqueueing {} msg for {}.", msg.count, &peer_addr);
                                for idx in 0..msg.count {
                                    let worker_msg = format!("{}:{}:{}", client_id, msg.mid, idx);
                                    match redis::cmd("LPUSH")
                                        .arg("queue")
                                        .arg(worker_msg)
                                        .query_async::<_, usize>(&mut conn)
                                        .await
                                    {
                                        Ok(_) => continue,
                                        Err(_) => {
                                            // TODO ??
                                            tracing::error!(parent:&span,"can't enqueue message ");
                                            return;
                                        }
                                    }
                                }
                            }
                            ControlFlow::Continue(None) => continue,
                            // TODO: maybe flush before sending
                            ControlFlow::Break(_) => {
                                tracing::trace!(parent:&span,"websocket context {peer_addr} destroyed");
                            return
                        }
                    }
                    }
                }
                worker_msg = rx.recv_async() => {
                    // Send response to the client
                  if let Ok(worker_msg) = worker_msg {
                  tracing::trace!(parent:&span,worker_msg = ?worker_msg ,"sent worker message to client : {} ",&client_id);
                  if sender
                        .send(Message::Binary(serde_json::to_vec(&worker_msg).unwrap()))
                        .await
                        .is_err()
                    {
                        tracing::error!(parent:&span,"can't send processed message to client");
                        return;
                    }

                  }else{
                    tracing::error!(parent:&span,"send channel closed")
                  }


                }



        }
    }
}

pub fn process_ws_message(
    msg: Message,
    peer: SocketAddr,
    client_id: Uuid,
) -> ControlFlow<(), Option<ClientMessage>> {
    match msg {
        Message::Binary(data) => {
            // serde json to deserialize
            let client_msg: ClientMessage =
                serde_json::from_slice(&data).expect("can't deserialize data");
            tracing::info!("received  binary: {:?}", client_msg);
            ControlFlow::Continue(Some(client_msg))
        }
        Message::Close(c) => {
            if let Some(cf) = c {
                tracing::info!(
                    ">>> {} sent close with code {} and reason `{}`",
                    client_id,
                    cf.code,
                    cf.reason
                );
            } else {
                tracing::error!(
                    ">>> {client_id}:{peer} somehow sent close message without CloseFrame"
                );
            }
            ControlFlow::Break(())
        }
        _ => {
            tracing::info!("Client send an unhandled message type");
            ControlFlow::Continue(None)
        }
    }
}

pub async fn handle_worker(
    State(state): State<Appstate>,
    Json(msg): Json<WorkerMessage>,
) -> StatusCode {
    let map = state.socket_to_tx.lock().await;
    tracing::info!(msg = ?msg,"received processed_msg from worker");

    match map.get(&msg.cid) {
        Some(tx) => {
            //todo: deal with channel closed
            tx.send_async(msg).await.expect("can't send worker message");
            StatusCode::ACCEPTED
        }
        None => StatusCode::NOT_FOUND,
    }
}
