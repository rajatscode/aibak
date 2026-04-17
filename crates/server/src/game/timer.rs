use std::sync::Arc;
use std::time::Duration;

use sqlx::PgPool;
use tracing::{error, info};

use crate::game::manager::GameManager;

/// Background task that checks for expired turn deadlines and auto-submits empty orders.
pub async fn boot_timer_task(pool: PgPool, manager: Arc<GameManager>) {
    info!("boot timer background task started");
    let mut interval = tokio::time::interval(Duration::from_secs(10));

    loop {
        interval.tick().await;

        match crate::db::get_expired_deadlines(&pool).await {
            Ok(expired) => {
                for (game_id, turn) in expired {
                    info!(%game_id, turn, "boot timer expired, auto-submitting");
                    if let Err(e) = manager.check_boot_timer(game_id, turn).await {
                        error!(%game_id, turn, "boot timer error: {}", e);
                    }
                }
            }
            Err(e) => {
                error!("failed to check expired deadlines: {}", e);
            }
        }

        // Clean up stale waiting games (created > 30 minutes ago, nobody joined).
        if let Err(e) = crate::db::cleanup_stale_waiting_games(&pool).await {
            error!("failed to clean up stale waiting games: {}", e);
        }
    }
}
