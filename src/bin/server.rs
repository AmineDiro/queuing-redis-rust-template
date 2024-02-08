use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        State,
    },
    response::IntoResponse,
    routing::get,
    Router,
};
use redis::{aio::ConnectionManager, AsyncCommands, Client};
use serde::{Deserialize, Serialize};

use std::{net::SocketAddr, ops::ControlFlow, sync::Arc};
use tracing;
use tracing_subscriber;
use uuid::Uuid;

use tower_http::{
    services::ServeDir,
    trace::{DefaultMakeSpan, TraceLayer},
};

//allows to extract the IP of connecting user
use axum::extract::connect_info::ConnectInfo;
use futures::stream::StreamExt;

async fn ws_handler(
    ws: WebSocketUpgrade,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    State(conn): State<ConnectionManager>,
) -> impl IntoResponse {
    tracing::info!("{addr} connected.");
    // finalize the upgrade process by returning upgrade callback.
    let client_id = Uuid::new_v4();
    ws.on_upgrade(move |socket| handle_socket(socket, addr, client_id, conn))
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "lowercase")]
enum CommandType {
    Enqueue,
    Processed,
}
#[derive(Serialize, Deserialize, Debug)]
struct ClientMessage {
    command: CommandType,
    count: usize,
    mid: usize,
}

// Websocket statemachine (one will be spawned per connection)
async fn handle_socket(
    mut socket: WebSocket,
    peer_addr: SocketAddr,
    client_id: Uuid,
    mut conn: ConnectionManager,
) {
    let (mut sender, mut receiver) = socket.split();

    // This second task will receive messages from client and print them on server console
    let mut recv_task = tokio::spawn(async move {
        while let Some(Ok(msg)) = receiver.next().await {
            // Process ws message
            match process_message(msg, peer_addr) {
                ControlFlow::Continue(Some(msg)) => {
                    // Enqueue to redis
                    tracing::info!("Enqueueing {} msg for {}.", msg.count, &peer_addr);
                    for idx in 0..msg.count {
                        let worker_msg = format!("{}:{}:{}", client_id, msg.mid, idx);
                        let _ = redis::cmd("LPUSH")
                            .arg("queue")
                            .arg(worker_msg)
                            .query_async(&mut conn)
                            .await
                            .unwrap();
                    }
                }
                ControlFlow::Continue(None) => continue,
                ControlFlow::Break(_) => return,
            }
        }
    });

    // If any one of the tasks exit, abort the other.
    // tokio::select! {
    //     rv_a = (&mut send_task) => {
    //         recv_task.abort();
    //     },
    //     rv_b = (&mut recv_task) => {
    //         send_task.abort();
    //     }
    // }

    // returning from the handler closes the websocket connection
    tracing::info!("Websocket context {peer_addr} destroyed");
}

/// helper to print contents of messages to stdout. Has special treatment for Close.
fn process_message(msg: Message, peer: SocketAddr) -> ControlFlow<(), Option<ClientMessage>> {
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
                println!(
                    ">>> {} sent close with code {} and reason `{}`",
                    peer, cf.code, cf.reason
                );
            } else {
                println!(">>> {peer} somehow sent close message without CloseFrame");
            }
            return ControlFlow::Break(());
        }
        _ => {
            tracing::info!("Client send an unhandled message type");
            return ControlFlow::Continue(None);
        }
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();

    let client = Client::open("redis://localhost:6379")?;
    let mut con = ConnectionManager::new(client).await?;

    let app = Router::new()
        .route("/ws", get(ws_handler))
        .with_state(con)
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
