use std::sync::Arc;

use chrono::{DateTime, Utc};
use tokio::sync::Mutex;
use tracing::{info, warn};
use uuid::Uuid;

/// A player waiting in the matchmaking queue.
#[derive(Debug, Clone)]
pub struct QueueEntry {
    pub user_id: Uuid,
    pub username: String,
    pub rating: f64,
    pub joined_at: DateTime<Utc>,
    pub template: String,
}

/// A matched pair ready for game creation.
#[derive(Debug, Clone)]
pub struct MatchedPair {
    pub player_a: QueueEntry,
    pub player_b: QueueEntry,
}

/// Real-time matchmaking queue that pairs players of similar skill.
pub struct MatchmakingQueue {
    entries: Arc<Mutex<Vec<QueueEntry>>>,
}

/// Initial rating-difference threshold for matching.
const BASE_THRESHOLD: f64 = 100.0;
/// How much the threshold grows per expansion step.
const THRESHOLD_GROWTH: f64 = 50.0;
/// Seconds between threshold expansions.
const EXPANSION_INTERVAL_SECS: f64 = 10.0;
/// After this many seconds, match with anyone on the same template.
const MAX_WAIT_SECS: f64 = 60.0;

impl MatchmakingQueue {
    pub fn new() -> Self {
        Self {
            entries: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Add a player to the queue. Returns `false` if already queued.
    pub async fn enqueue(
        &self,
        user_id: Uuid,
        username: String,
        rating: f64,
        template: String,
    ) -> bool {
        let mut entries = self.entries.lock().await;
        if entries.iter().any(|e| e.user_id == user_id) {
            return false;
        }
        entries.push(QueueEntry {
            user_id,
            username,
            rating,
            joined_at: Utc::now(),
            template,
        });
        info!(user_id = %user_id, "player joined matchmaking queue");
        true
    }

    /// Remove a player from the queue. Returns `true` if they were found.
    pub async fn dequeue(&self, user_id: Uuid) -> bool {
        let mut entries = self.entries.lock().await;
        let before = entries.len();
        entries.retain(|e| e.user_id != user_id);
        let removed = entries.len() < before;
        if removed {
            info!(user_id = %user_id, "player left matchmaking queue");
        }
        removed
    }

    /// Check if a player is currently queued.
    pub async fn is_queued(&self, user_id: Uuid) -> bool {
        let entries = self.entries.lock().await;
        entries.iter().any(|e| e.user_id == user_id)
    }

    /// Get the current queue size.
    pub async fn queue_size(&self) -> usize {
        let entries = self.entries.lock().await;
        entries.len()
    }

    /// Get queue position (1-based) and estimated wait time for a player.
    pub async fn queue_status(&self, user_id: Uuid) -> Option<(usize, u32)> {
        let entries = self.entries.lock().await;
        let entry = entries.iter().find(|e| e.user_id == user_id)?;
        let template = &entry.template;
        let rating = entry.rating;
        let wait_secs = (Utc::now() - entry.joined_at).num_seconds().max(0) as u32;

        // Count how many people on the same template are ahead of this player.
        let position = entries
            .iter()
            .filter(|e| e.template == *template && e.joined_at < entry.joined_at)
            .count()
            + 1;

        // Estimate: check how many compatible opponents exist.
        let compatible = entries
            .iter()
            .filter(|e| e.user_id != user_id && e.template == *template)
            .filter(|e| {
                let threshold = compute_threshold(entry);
                (e.rating - rating).abs() < threshold
            })
            .count();

        // Rough estimate: if there's a compatible opponent, match in the next tick (2s).
        // Otherwise estimate based on threshold expansion.
        let estimated_wait = if compatible > 0 {
            2
        } else {
            // Time until we'd match with anyone.
            MAX_WAIT_SECS as u32 - wait_secs.min(MAX_WAIT_SECS as u32)
        };

        Some((position, estimated_wait))
    }

    /// Try to find matches among queued players. Returns matched pairs and removes
    /// them from the queue. Called periodically by a background task.
    pub async fn find_matches(&self) -> Vec<MatchedPair> {
        let mut entries = self.entries.lock().await;
        let mut matched_pairs = Vec::new();
        let mut matched_indices = Vec::new();

        // Sort by rating for efficient nearest-neighbor matching.
        entries.sort_by(|a, b| a.rating.partial_cmp(&b.rating).unwrap_or(std::cmp::Ordering::Equal));

        let len = entries.len();
        let mut used = vec![false; len];

        for i in 0..len {
            if used[i] {
                continue;
            }

            let mut best_j: Option<usize> = None;
            let mut best_diff = f64::MAX;

            let threshold_i = compute_threshold(&entries[i]);

            for j in (i + 1)..len {
                if used[j] {
                    continue;
                }

                // Must be on the same template.
                if entries[i].template != entries[j].template {
                    continue;
                }

                let diff = (entries[i].rating - entries[j].rating).abs();

                // Use the more permissive threshold of the two players.
                let threshold_j = compute_threshold(&entries[j]);
                let threshold = threshold_i.max(threshold_j);

                if diff <= threshold && diff < best_diff {
                    best_diff = diff;
                    best_j = Some(j);
                }
            }

            if let Some(j) = best_j {
                used[i] = true;
                used[j] = true;
                matched_indices.push(i);
                matched_indices.push(j);
                matched_pairs.push(MatchedPair {
                    player_a: entries[i].clone(),
                    player_b: entries[j].clone(),
                });
                info!(
                    player_a = %entries[i].user_id,
                    player_b = %entries[j].user_id,
                    rating_diff = best_diff,
                    "matched players"
                );
            }
        }

        // Remove matched entries (iterate in reverse to preserve indices).
        matched_indices.sort_unstable();
        for &idx in matched_indices.iter().rev() {
            entries.remove(idx);
        }

        matched_pairs
    }
}

impl Default for MatchmakingQueue {
    fn default() -> Self {
        Self::new()
    }
}

/// Compute the rating threshold for a queue entry based on how long they've waited.
fn compute_threshold(entry: &QueueEntry) -> f64 {
    let waited_secs = (Utc::now() - entry.joined_at).num_seconds().max(0) as f64;

    if waited_secs >= MAX_WAIT_SECS {
        // After max wait, match with anyone.
        return f64::MAX;
    }

    let expansions = (waited_secs / EXPANSION_INTERVAL_SECS).floor();
    BASE_THRESHOLD + THRESHOLD_GROWTH * expansions
}

/// Background task that periodically runs matchmaking and creates games.
pub async fn matchmaking_task(
    queue: Arc<MatchmakingQueue>,
    game_manager: Option<Arc<super::manager::GameManager>>,
    hub: Arc<crate::ws::Hub>,
    _db_pool: Option<sqlx::PgPool>,
) {
    let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(2));

    loop {
        interval.tick().await;

        let matches = queue.find_matches().await;

        for matched in matches {
            let player_a = matched.player_a;
            let player_b = matched.player_b;
            let template = player_a.template.clone();

            // Try to create a game if the game manager is available.
            if let Some(ref manager) = game_manager {
                match create_matched_game(manager, &player_a, &player_b, &template).await {
                    Ok(game_id) => {
                        info!(
                            game_id = %game_id,
                            player_a = %player_a.user_id,
                            player_b = %player_b.user_id,
                            "matchmaking created game"
                        );

                        // Notify both players via WebSocket.
                        // We use a user-specific channel approach: broadcast on a
                        // pseudo game_id derived from the user_id so each user's
                        // personal subscription gets the event.
                        hub.broadcast(
                            player_a.user_id,
                            crate::ws::GameEvent::MatchFound {
                                game_id,
                                opponent_name: player_b.username.clone(),
                            },
                        );
                        hub.broadcast(
                            player_b.user_id,
                            crate::ws::GameEvent::MatchFound {
                                game_id,
                                opponent_name: player_a.username.clone(),
                            },
                        );
                    }
                    Err(e) => {
                        warn!(
                            player_a = %player_a.user_id,
                            player_b = %player_b.user_id,
                            error = %e,
                            "failed to create matched game, re-queuing players"
                        );
                        // Re-queue on failure (best effort).
                        let _ = queue
                            .enqueue(
                                player_a.user_id,
                                player_a.username,
                                player_a.rating,
                                player_a.template,
                            )
                            .await;
                        let _ = queue
                            .enqueue(
                                player_b.user_id,
                                player_b.username,
                                player_b.rating,
                                player_b.template,
                            )
                            .await;
                    }
                }
            } else {
                // No DB / game manager -- log the match but can't create a game.
                info!(
                    player_a = %player_a.user_id,
                    player_b = %player_b.user_id,
                    "match found (no game manager -- skipping game creation)"
                );

                // Still notify via WebSocket with a nil game_id.
                hub.broadcast(
                    player_a.user_id,
                    crate::ws::GameEvent::MatchFound {
                        game_id: Uuid::nil(),
                        opponent_name: player_b.username.clone(),
                    },
                );
                hub.broadcast(
                    player_b.user_id,
                    crate::ws::GameEvent::MatchFound {
                        game_id: Uuid::nil(),
                        opponent_name: player_a.username.clone(),
                    },
                );
            }
        }
    }
}

/// Create a game for a matched pair: player_a creates, player_b joins.
async fn create_matched_game(
    manager: &super::manager::GameManager,
    player_a: &QueueEntry,
    player_b: &QueueEntry,
    template: &str,
) -> Result<Uuid, super::manager::GameError> {
    let game = manager.create_game(player_a.user_id, template).await?;
    let game = manager.join_game(game.id, player_b.user_id).await?;
    Ok(game.id)
}
