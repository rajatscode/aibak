use axum::{
    extract::{
        State, WebSocketUpgrade,
        ws::{Message, WebSocket},
    },
    response::IntoResponse,
};
use serde::Deserialize;
use tracing::{error, info};
use uuid::Uuid;

use crate::AppState;
use crate::auth::AuthUser;

/// Client-to-server WebSocket messages.
#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum ClientMessage {
    Subscribe { game_id: Uuid },
    Unsubscribe { game_id: Uuid },
}

/// Handle WebSocket upgrade request.
pub async fn ws_upgrade(
    ws: WebSocketUpgrade,
    auth: AuthUser,
    State(state): State<AppState>,
) -> impl IntoResponse {
    info!(user_id = %auth.user_id, "websocket upgrade");
    ws.on_upgrade(move |socket| handle_socket(socket, auth, state))
}

async fn handle_socket(mut socket: WebSocket, auth: AuthUser, state: AppState) {
    let hub = &state.ws_hub;

    // Channel for forwarding game events to the WebSocket sender.
    let (tx, mut rx) = tokio::sync::mpsc::channel::<String>(256);
    let mut subscriptions: Vec<(Uuid, tokio::task::JoinHandle<()>)> = Vec::new();

    // Main loop: read from WebSocket or forward events.
    loop {
        tokio::select! {
            // Receive from WebSocket client.
            msg = socket.recv() => {
                match msg {
                    Some(Ok(Message::Text(text))) => {
                        let text_str: &str = &text;
                        match serde_json::from_str::<ClientMessage>(text_str) {
                            Ok(ClientMessage::Subscribe { game_id }) => {
                                let mut sub_receiver = hub.subscribe(game_id).await;
                                let tx_clone = tx.clone();
                                let handle = tokio::spawn(async move {
                                    while let Ok(event) = sub_receiver.recv().await {
                                        if let Ok(json) = serde_json::to_string(&event)
                                            && tx_clone.send(json).await.is_err()
                                        {
                                            break;
                                        }
                                    }
                                });
                                // Remove any existing subscription for this game to avoid duplicates on reconnect.
                                subscriptions.retain(|(gid, h)| {
                                    if *gid == game_id { h.abort(); false } else { true }
                                });
                                subscriptions.push((game_id, handle));
                            }
                            Ok(ClientMessage::Unsubscribe { game_id }) => {
                                subscriptions.retain(|(gid, handle)| {
                                    if *gid == game_id {
                                        handle.abort();
                                        false
                                    } else {
                                        true
                                    }
                                });
                            }
                            Err(e) => {
                                error!(user_id = %auth.user_id, "invalid ws message: {}", e);
                            }
                        }
                    }
                    Some(Ok(Message::Close(_))) | None => break,
                    Some(Err(e)) => {
                        error!(user_id = %auth.user_id, "ws error: {}", e);
                        break;
                    }
                    _ => {} // Ping/Pong/Binary ignored.
                }
            }
            // Forward game events to WebSocket client.
            Some(json) = rx.recv() => {
                if socket.send(Message::Text(json.into())).await.is_err() {
                    break;
                }
            }
        }
    }

    // Cleanup subscriptions.
    for (_, handle) in subscriptions {
        handle.abort();
    }
    info!(user_id = %auth.user_id, "websocket disconnected");
}
