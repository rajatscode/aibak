pub mod handler;

use std::collections::HashMap;

use serde::Serialize;
use tokio::sync::{broadcast, RwLock};
use uuid::Uuid;

/// Maximum number of pending messages per game channel.
const CHANNEL_CAPACITY: usize = 64;

/// Events pushed to WebSocket subscribers.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum GameEvent {
    GameStateUpdated {
        game_id: Uuid,
        status: String,
    },
    OpponentCommitted {
        game_id: Uuid,
        seat: u8,
    },
    TurnResolved {
        game_id: Uuid,
        turn: i32,
    },
    GameFinished {
        game_id: Uuid,
        winner_id: Uuid,
    },
    MatchFound {
        game_id: Uuid,
        opponent_name: String,
    },
    QueueUpdate {
        position: usize,
        estimated_wait_secs: u32,
    },
}

/// Central hub for WebSocket game event channels.
pub struct Hub {
    channels: RwLock<HashMap<Uuid, broadcast::Sender<GameEvent>>>,
}

impl Hub {
    pub fn new() -> Self {
        Self {
            channels: RwLock::new(HashMap::new()),
        }
    }

    /// Subscribe to events for a specific game. Creates the channel if it doesn't exist.
    pub async fn subscribe(&self, game_id: Uuid) -> broadcast::Receiver<GameEvent> {
        let mut channels = self.channels.write().await;
        let sender = channels
            .entry(game_id)
            .or_insert_with(|| broadcast::channel(CHANNEL_CAPACITY).0);
        sender.subscribe()
    }

    /// Broadcast an event to all subscribers of a game.
    pub fn broadcast(&self, game_id: Uuid, event: GameEvent) {
        let channels = self.channels.blocking_read();
        if let Some(sender) = channels.get(&game_id) {
            // Ignore send errors (no receivers).
            let _ = sender.send(event);
        }
    }

    /// Remove a game channel (called when game is finished and all clients disconnect).
    pub async fn remove_channel(&self, game_id: Uuid) {
        let mut channels = self.channels.write().await;
        channels.remove(&game_id);
    }
}

impl Default for Hub {
    fn default() -> Self {
        Self::new()
    }
}
