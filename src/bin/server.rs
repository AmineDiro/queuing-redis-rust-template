use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        State,
    },
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use flume::Receiver;
use redis::{aio::ConnectionManager, Client};
use redis_queue::{ClientMessage, WorkerMessage};
use std::{collections::HashMap, net::SocketAddr, ops::ControlFlow, sync::Arc};
use tokio::sync::Mutex;
use tracing;
use tracing_subscriber;
use uuid::Uuid;

use tower_http::{
    services::ServeDir,
    trace::{DefaultMakeSpan, TraceLayer},
};

use axum::extract::connect_info::ConnectInfo;
use futures::{stream::StreamExt, SinkExt};

async fn ws_handler(
    ws: WebSocketUpgrade,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    State(state): State<Appstate>,
) -> impl IntoResponse {
    tracing::info!("{addr} connected.");
    // finalize the upgrade process by returning upgrade callback.
    let client_id = Uuid::new_v4();

    let mut hashmap = state.socket_to_tx.lock().await;
    let (tx, rx) = flume::unbounded();
    hashmap.insert(client_id, tx);
    ws.on_upgrade(move |socket| handle_socket(socket, addr, client_id, state.conn, rx))
}

// Websocket statemachine (one will be spawned per connection)
async fn handle_socket(
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
                        match process_ws_message(msg, peer_addr) {
                            ControlFlow::Continue(Some(msg)) => {
                                // Enqueue to redis
                                tracing::info!("Enqueueing {} msg for {}.", msg.count, &peer_addr);
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
                                            tracing::error!("can't enqueue message ");
                                            return;
                                        }
                                    }
                                }
                            }
                            ControlFlow::Continue(None) => continue,
                            // TODO: maybe flush before sending
                            ControlFlow::Break(_) => {
                                tracing::info!("Websocket context {peer_addr} destroyed");
                            return
                        }
                    }
                    }
                }
                worker_msg = rx.recv_async() => {
                    // Send response to the client
                  if let Ok(worker_msg) = worker_msg {

                  tracing::debug!(worker_msg = ?worker_msg ,"Sent worker message to client : {} ",&client_id);
                  if sender
                        .send(Message::Binary(serde_json::to_vec(&worker_msg).unwrap()))
                        .await
                        .is_err()
                    {
                        tracing::error!("can't send processed message to client");
                        return;
                    }

                  }else{
                    tracing::error!("send channel closed")
                  }


                }

        }
    }
}

fn process_ws_message(msg: Message, peer: SocketAddr) -> ControlFlow<(), Option<ClientMessage>> {
    match msg {
        Message::Binary(data) => {
            // serde json to deserialize
            let client_msg: ClientMessage =
                serde_json::from_slice(&data).expect("can't deserialize data");
            tracing::info!("Received  binary: {:?}", client_msg);
            return ControlFlow::Continue(Some(client_msg));
        }
        Message::Close(c) => {
            if let Some(cf) = c {
                tracing::info!(
                    ">>> {} sent close with code {} and reason `{}`",
                    peer,
                    cf.code,
                    cf.reason
                );
            } else {
                tracing::error!(">>> {peer} somehow sent close message without CloseFrame");
            }
            return ControlFlow::Break(());
        }
        _ => {
            tracing::info!("Client send an unhandled message type");
            return ControlFlow::Continue(None);
        }
    }
}

async fn handle_worker(
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

#[derive(Clone)]
struct Appstate {
    conn: ConnectionManager,
    socket_to_tx: Arc<Mutex<HashMap<Uuid, flume::Sender<WorkerMessage>>>>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();

    let client = Client::open("redis://localhost:6379")?;
    let conn = ConnectionManager::new(client).await?;
    let state = Appstate {
        conn,
        socket_to_tx: Arc::new(Mutex::new(HashMap::new())),
    };

    let app = Router::new()
        .route("/processed", post(handle_worker))
        .route("/enqueue", get(ws_handler))
        .with_state(state)
        .fallback_service(ServeDir::new("assets/").append_index_html_on_directories(true))
        .layer(
            TraceLayer::new_for_http()
                .make_span_with(DefaultMakeSpan::default().include_headers(true)),
        );

    let listener = tokio::net::TcpListener::bind("127.0.0.1:3000")
        .await
        .unwrap();

    tracing::info!("listening on {}", listener.local_addr().unwrap());
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .await?;
    Ok(())
}
