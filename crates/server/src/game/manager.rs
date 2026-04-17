use std::path::PathBuf;
use std::sync::Arc;

use rand::SeedableRng;
use rand::rngs::StdRng;
use serde_json;
use sqlx::PgPool;
use tokio::sync::Mutex;
use uuid::Uuid;

use strat_engine::board::Board;
use strat_engine::map::MapFile;
use strat_engine::orders::{Order, validate_orders};
use strat_engine::picking;
use strat_engine::state::{GameState, Phase};
use strat_engine::turn::resolve_turn;

use tracing::error;

use crate::db;
use crate::game::rating::{self, Rating};
use crate::ws;

/// Errors from game management operations.
#[derive(Debug, thiserror::Error)]
pub enum GameError {
    #[error("database error: {0}")]
    Db(#[from] sqlx::Error),
    #[error("game not found")]
    NotFound,
    #[error("game is not in the expected state: expected {expected}, got {actual}")]
    WrongStatus { expected: String, actual: String },
    #[error("player is not a participant in this game")]
    NotParticipant,
    #[error("cannot join your own game")]
    CannotJoinOwnGame,
    #[error("game is full")]
    GameFull,
    #[error("invalid picks: {0}")]
    InvalidPicks(String),
    #[error("invalid orders: {0}")]
    InvalidOrders(String),
    #[error("map not found: {0}")]
    MapNotFound(String),
    #[error("serialization error: {0}")]
    Serialization(String),
    #[error("turn deadline has passed")]
    DeadlinePassed,
    #[error("validation error: {0}")]
    Validation(String),
}

impl From<GameError> for (axum::http::StatusCode, String) {
    fn from(err: GameError) -> Self {
        use axum::http::StatusCode;
        match &err {
            GameError::NotFound => (StatusCode::NOT_FOUND, err.to_string()),
            GameError::WrongStatus { .. }
            | GameError::InvalidPicks(_)
            | GameError::InvalidOrders(_)
            | GameError::DeadlinePassed
            | GameError::Validation(_) => (StatusCode::BAD_REQUEST, err.to_string()),
            GameError::NotParticipant | GameError::CannotJoinOwnGame => {
                (StatusCode::FORBIDDEN, err.to_string())
            }
            GameError::GameFull => (StatusCode::CONFLICT, err.to_string()),
            _ => (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()),
        }
    }
}

/// Manages game lifecycle and coordinates engine, DB, and WebSocket notifications.
pub struct GameManager {
    pool: PgPool,
    rng: Mutex<StdRng>,
    hub: Arc<ws::Hub>,
}

impl GameManager {
    pub fn new(pool: PgPool, hub: Arc<ws::Hub>) -> Self {
        Self {
            pool,
            rng: Mutex::new(StdRng::from_entropy()),
            hub,
        }
    }

    /// Load a map template by name.
    fn load_map(template: &str) -> Result<MapFile, GameError> {
        let path = PathBuf::from(format!("maps/{}.json", template));
        MapFile::load(&path).map_err(|_| GameError::MapNotFound(template.to_string()))
    }

    /// Create a new game in "waiting" status.
    pub async fn create_game(
        &self,
        user_id: Uuid,
        template: &str,
    ) -> Result<db::GameRow, GameError> {
        let map_file = Self::load_map(template)?;
        let map_json =
            serde_json::to_value(&map_file).map_err(|e| GameError::Serialization(e.to_string()))?;
        let board = Board::from_map(map_file);
        let game_state = GameState::new(&board);
        let state_json = serde_json::to_value(&game_state)
            .map_err(|e| GameError::Serialization(e.to_string()))?;
        let row = db::create_game(&self.pool, template, user_id, &state_json, &map_json).await?;
        Ok(row)
    }

    /// Join an open game as the second player. Generates pick options and moves to "picking".
    pub async fn join_game(&self, game_id: Uuid, user_id: Uuid) -> Result<db::GameRow, GameError> {
        let game = db::get_game(&self.pool, game_id)
            .await?
            .ok_or(GameError::NotFound)?;

        if game.status != "waiting" {
            return Err(GameError::WrongStatus {
                expected: "waiting".to_string(),
                actual: game.status,
            });
        }

        if game.player_a == Some(user_id) {
            return Err(GameError::CannotJoinOwnGame);
        }

        if game.player_b.is_some() {
            return Err(GameError::GameFull);
        }

        // Parse map from stored JSON and wrap in Board.
        let map_file: MapFile = serde_json::from_value(game.map_json.clone().ok_or(GameError::NotFound)?)
            .map_err(|e| GameError::Serialization(e.to_string()))?;
        let board = Board::from_map(map_file);

        // Validate picks * players doesn't exceed available bonus territories.
        let available_starts = board.map.bonuses.iter()
            .filter(|b| b.value > 0 && b.territory_ids.iter().any(|&tid| !board.map.territories[tid].is_wasteland))
            .count();
        if board.config.picking.num_picks * 2 > available_starts {
            return Err(GameError::Validation(format!(
                "Too many picks for this map ({} picks × 2 players = {}, but only {} starts available)",
                board.config.picking.num_picks,
                board.config.picking.num_picks * 2,
                available_starts
            )));
        }

        // Generate pick options.
        let pick_options = {
            let mut rng = self.rng.lock().await;
            picking::generate_pick_options(&board, &mut *rng)
        };
        let pick_json = serde_json::to_value(&pick_options)
            .map_err(|e| GameError::Serialization(e.to_string()))?;

        db::set_game_player_b(&self.pool, game_id, user_id).await?;

        let state: GameState =
            serde_json::from_value(game.state_json.clone().ok_or(GameError::NotFound)?)
                .map_err(|e| GameError::Serialization(e.to_string()))?;
        let state_json =
            serde_json::to_value(&state).map_err(|e| GameError::Serialization(e.to_string()))?;

        db::update_game_state(
            &self.pool,
            game_id,
            "picking",
            0,
            &state_json,
            Some(&pick_json),
        )
        .await?;

        // Set picking phase deadline (same duration as turn deadlines).
        let deadline = chrono::Utc::now() + chrono::Duration::hours(24);
        db::set_turn_deadline(&self.pool, game_id, 0, deadline).await?;

        self.hub.broadcast(
            game_id,
            ws::GameEvent::GameStateUpdated {
                game_id,
                status: "picking".to_string(),
            },
        ).await;

        db::get_game(&self.pool, game_id)
            .await?
            .ok_or(GameError::NotFound)
    }

    /// Submit picks for a player during the picking phase.
    /// When both players have submitted, resolves picks and moves to "active".
    pub async fn submit_picks(
        &self,
        game_id: Uuid,
        user_id: Uuid,
        picks: Vec<usize>,
    ) -> Result<(), GameError> {
        let game = db::get_game(&self.pool, game_id)
            .await?
            .ok_or(GameError::NotFound)?;

        if game.status != "picking" {
            return Err(GameError::WrongStatus {
                expected: "picking".to_string(),
                actual: game.status,
            });
        }

        // Reject late submissions.
        if let Some(deadline) = db::get_turn_deadline(&self.pool, game_id, 0).await? {
            if chrono::Utc::now() > deadline {
                return Err(GameError::DeadlinePassed);
            }
        }

        let seat = self.player_seat(&game, user_id)?;

        let map_file: MapFile = serde_json::from_value(game.map_json.clone().ok_or(GameError::NotFound)?)
            .map_err(|e| GameError::Serialization(e.to_string()))?;
        let board = Board::from_map(map_file);

        if picks.len() < board.picking().num_picks {
            return Err(GameError::InvalidPicks(format!(
                "need at least {} picks, got {}",
                board.picking().num_picks,
                picks.len()
            )));
        }

        // Validate all pick IDs are in bounds.
        for &tid in &picks {
            if tid >= board.map.territory_count() {
                return Err(GameError::InvalidPicks(format!(
                    "invalid territory ID: {}",
                    tid
                )));
            }
        }

        // Validate picks are from pick_options.
        if let Some(pick_opts_json) = &game.pick_options {
            let pick_options: Vec<usize> = serde_json::from_value(pick_opts_json.clone())
                .map_err(|e| GameError::Serialization(e.to_string()))?;
            for &tid in &picks {
                if !pick_options.contains(&tid) {
                    return Err(GameError::InvalidPicks(format!(
                        "territory {} is not a valid pick option",
                        tid
                    )));
                }
            }
        }

        let picks_json =
            serde_json::to_value(&picks).map_err(|e| GameError::Serialization(e.to_string()))?;

        // Use a transaction with FOR UPDATE to prevent double resolution.
        let mut tx = self.pool.begin().await?;

        // Lock the game row to serialize concurrent pick submissions.
        let game = db::get_game_for_update(&mut tx, game_id)
            .await?
            .ok_or(GameError::NotFound)?;

        if game.status != "picking" {
            // Another thread already resolved; our picks were late.
            tx.rollback().await?;
            return Err(GameError::WrongStatus {
                expected: "picking".to_string(),
                actual: game.status,
            });
        }

        // Store picks as orders for turn 0.
        db::insert_orders_tx(&mut tx, game_id, user_id, 0, &picks_json).await?;

        // Check if both players have submitted picks.
        let all_orders = db::get_orders_for_turn_tx(&mut tx, game_id, 0).await?;
        if all_orders.len() < 2 {
            tx.commit().await?;
            self.hub
                .broadcast(game_id, ws::GameEvent::OpponentCommitted { game_id, seat }).await;
            return Ok(());
        }

        // Both picks in: resolve.
        let mut state: GameState =
            serde_json::from_value(game.state_json.clone().ok_or(GameError::NotFound)?)
                .map_err(|e| GameError::Serialization(e.to_string()))?;

        let mut player_picks: [Vec<usize>; 2] = [Vec::new(), Vec::new()];
        for order_row in &all_orders {
            let s = self.player_seat_by_id(&game, order_row.user_id)?;
            let p: Vec<usize> = serde_json::from_value(order_row.orders_json.clone())
                .map_err(|e| GameError::Serialization(e.to_string()))?;
            player_picks[s as usize] = p;
        }

        picking::resolve_picks(
            &mut state,
            [&player_picks[0], &player_picks[1]],
            &board,
            picking::DEFAULT_STARTING_ARMIES,
        );

        let state_json =
            serde_json::to_value(&state).map_err(|e| GameError::Serialization(e.to_string()))?;

        db::update_game_state_tx(&mut tx, game_id, "active", 1, &state_json, None).await?;

        // Set first turn deadline.
        let deadline = chrono::Utc::now() + chrono::Duration::hours(24);
        db::set_turn_deadline_tx(&mut tx, game_id, 1, deadline).await?;

        tx.commit().await?;

        self.hub
            .broadcast(game_id, ws::GameEvent::OpponentCommitted { game_id, seat }).await;

        self.hub.broadcast(
            game_id,
            ws::GameEvent::GameStateUpdated {
                game_id,
                status: "active".to_string(),
            },
        ).await;

        Ok(())
    }

    /// Submit orders for a player during the active (play) phase.
    /// When both players have submitted, resolves the turn.
    pub async fn submit_orders(
        &self,
        game_id: Uuid,
        user_id: Uuid,
        orders: Vec<Order>,
    ) -> Result<(), GameError> {
        let game = db::get_game(&self.pool, game_id)
            .await?
            .ok_or(GameError::NotFound)?;

        if game.status != "active" {
            return Err(GameError::WrongStatus {
                expected: "active".to_string(),
                actual: game.status,
            });
        }

        // Reject late submissions.
        if let Some(deadline) = db::get_turn_deadline(&self.pool, game_id, game.turn).await? {
            if chrono::Utc::now() > deadline {
                return Err(GameError::DeadlinePassed);
            }
        }

        let seat = self.player_seat(&game, user_id)?;

        let orders_json =
            serde_json::to_value(&orders).map_err(|e| GameError::Serialization(e.to_string()))?;

        // Use a transaction with FOR UPDATE to prevent double resolution.
        let mut tx = self.pool.begin().await?;

        // Lock the game row to serialize concurrent order submissions.
        let game = db::get_game_for_update(&mut tx, game_id)
            .await?
            .ok_or(GameError::NotFound)?;

        if game.status != "active" {
            tx.rollback().await?;
            return Err(GameError::WrongStatus {
                expected: "active".to_string(),
                actual: game.status,
            });
        }

        let current_turn = game.turn;

        // Validate orders before inserting.
        let map_file: MapFile = serde_json::from_value(game.map_json.clone().ok_or(GameError::NotFound)?)
            .map_err(|e| GameError::Serialization(e.to_string()))?;
        let board = Board::from_map(map_file);
        let state: GameState =
            serde_json::from_value(game.state_json.clone().ok_or(GameError::NotFound)?)
                .map_err(|e| GameError::Serialization(e.to_string()))?;
        if let Err(e) = validate_orders(&orders, seat, &state, &board) {
            tx.rollback().await?;
            return Err(GameError::InvalidOrders(e.to_string()));
        }

        // Store orders.
        db::insert_orders_tx(&mut tx, game_id, user_id, current_turn, &orders_json).await?;

        // Auto-submit empty orders for eliminated opponent.
        let opponent_seat: u8 = 1 - seat;
        if state.territory_count_for(opponent_seat) == 0 {
            let opponent_id = if opponent_seat == 0 { game.player_a } else { game.player_b };
            if let Some(opp_id) = opponent_id {
                let empty: Vec<Order> = Vec::new();
                let empty_json = serde_json::to_value(&empty)
                    .map_err(|e| GameError::Serialization(e.to_string()))?;
                db::insert_orders_tx(&mut tx, game_id, opp_id, current_turn, &empty_json).await?;
            }
        }

        // Check if both players have submitted.
        let all_orders = db::get_orders_for_turn_tx(&mut tx, game_id, current_turn).await?;
        if all_orders.len() < 2 {
            tx.commit().await?;
            self.hub
                .broadcast(game_id, ws::GameEvent::OpponentCommitted { game_id, seat }).await;
            return Ok(());
        }

        self.resolve_turn_inner_tx(&mut tx, game_id, &game, &all_orders)
            .await?;

        tx.commit().await?;

        self.hub
            .broadcast(game_id, ws::GameEvent::OpponentCommitted { game_id, seat }).await;

        Ok(())
    }

    /// Check and enforce boot timer for a game.
    pub async fn check_boot_timer(&self, game_id: Uuid, turn: i32) -> Result<(), GameError> {
        // Use a transaction with FOR UPDATE to prevent races with concurrent submissions.
        let mut tx = self.pool.begin().await?;

        let game = db::get_game_for_update(&mut tx, game_id)
            .await?
            .ok_or(GameError::NotFound)?;

        // Handle picking phase boot (turn 0).
        if game.status == "picking" && turn == 0 {
            return self.boot_picking_phase(tx, game_id, &game).await;
        }

        if game.status != "active" || game.turn != turn {
            tx.rollback().await?;
            return Ok(()); // Already moved on.
        }

        let existing = db::get_orders_for_turn_tx(&mut tx, game_id, turn).await?;
        if existing.len() >= 2 {
            tx.rollback().await?;
            return Ok(()); // Already resolved.
        }

        // Submit empty orders for missing players.
        let players = [game.player_a, game.player_b];
        let submitted_users: Vec<Uuid> = existing.iter().map(|o| o.user_id).collect();
        for pid in players.iter().flatten() {
            if !submitted_users.contains(pid) {
                let empty: Vec<Order> = Vec::new();
                let empty_json = serde_json::to_value(&empty)
                    .map_err(|e| GameError::Serialization(e.to_string()))?;
                db::insert_orders_tx(&mut tx, game_id, *pid, turn, &empty_json).await?;
            }
        }

        // Re-fetch and resolve.
        let all_orders = db::get_orders_for_turn_tx(&mut tx, game_id, turn).await?;
        if all_orders.len() >= 2 {
            self.resolve_turn_inner_tx(&mut tx, game_id, &game, &all_orders)
                .await?;
        }

        tx.commit().await?;

        Ok(())
    }

    /// Handle boot timer expiry during picking phase: auto-submit default picks for missing players,
    /// then resolve picks and transition to active.
    async fn boot_picking_phase(
        &self,
        mut tx: sqlx::Transaction<'_, sqlx::Postgres>,
        game_id: Uuid,
        game: &db::GameRow,
    ) -> Result<(), GameError> {
        let existing = db::get_orders_for_turn_tx(&mut tx, game_id, 0).await?;
        if existing.len() >= 2 {
            tx.rollback().await?;
            return Ok(());
        }

        let map_file: MapFile =
            serde_json::from_value(game.map_json.clone().ok_or(GameError::NotFound)?)
                .map_err(|e| GameError::Serialization(e.to_string()))?;
        let board = Board::from_map(map_file);
        let num_picks = board.picking().num_picks;

        // Get pick options to build default picks.
        let pick_options: Vec<usize> = game
            .pick_options
            .as_ref()
            .and_then(|v| serde_json::from_value(v.clone()).ok())
            .unwrap_or_default();
        let default_picks: Vec<usize> = pick_options.iter().copied().take(num_picks).collect();

        // Auto-submit default picks for missing players.
        let players = [game.player_a, game.player_b];
        let submitted_users: Vec<Uuid> = existing.iter().map(|o| o.user_id).collect();
        for pid in players.iter().flatten() {
            if !submitted_users.contains(pid) {
                let picks_json = serde_json::to_value(&default_picks)
                    .map_err(|e| GameError::Serialization(e.to_string()))?;
                db::insert_orders_tx(&mut tx, game_id, *pid, 0, &picks_json).await?;
            }
        }

        // Resolve picks.
        let all_orders = db::get_orders_for_turn_tx(&mut tx, game_id, 0).await?;
        if all_orders.len() < 2 {
            tx.rollback().await?;
            return Ok(());
        }

        let mut state: GameState =
            serde_json::from_value(game.state_json.clone().ok_or(GameError::NotFound)?)
                .map_err(|e| GameError::Serialization(e.to_string()))?;

        let mut player_picks: [Vec<usize>; 2] = [Vec::new(), Vec::new()];
        for order_row in &all_orders {
            let s = self.player_seat_by_id(game, order_row.user_id)?;
            let p: Vec<usize> = serde_json::from_value(order_row.orders_json.clone())
                .map_err(|e| GameError::Serialization(e.to_string()))?;
            player_picks[s as usize] = p;
        }

        picking::resolve_picks(
            &mut state,
            [&player_picks[0], &player_picks[1]],
            &board,
            picking::DEFAULT_STARTING_ARMIES,
        );

        let state_json =
            serde_json::to_value(&state).map_err(|e| GameError::Serialization(e.to_string()))?;

        db::update_game_state_tx(&mut tx, game_id, "active", 1, &state_json, None).await?;

        let deadline = chrono::Utc::now() + chrono::Duration::hours(24);
        db::set_turn_deadline_tx(&mut tx, game_id, 1, deadline).await?;

        tx.commit().await?;

        self.hub.broadcast(
            game_id,
            ws::GameEvent::GameStateUpdated {
                game_id,
                status: "active".to_string(),
            },
        ).await;

        Ok(())
    }

    /// Internal: resolve a turn once both players' orders are in (within a transaction).
    async fn resolve_turn_inner_tx(
        &self,
        tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
        game_id: Uuid,
        game: &db::GameRow,
        all_orders: &[db::OrderRow],
    ) -> Result<(), GameError> {
        let map_file: MapFile = serde_json::from_value(game.map_json.clone().ok_or(GameError::NotFound)?)
            .map_err(|e| GameError::Serialization(e.to_string()))?;
        let board = Board::from_map(map_file);

        let state: GameState =
            serde_json::from_value(game.state_json.clone().ok_or(GameError::NotFound)?)
                .map_err(|e| GameError::Serialization(e.to_string()))?;

        let mut player_orders: [Vec<Order>; 2] = [Vec::new(), Vec::new()];
        for order_row in all_orders {
            let seat = self.player_seat_by_id(game, order_row.user_id)?;
            let orders: Vec<Order> = serde_json::from_value(order_row.orders_json.clone())
                .map_err(|e| GameError::Serialization(e.to_string()))?;
            // Skip validation for eliminated players (they submit empty orders).
            if state.territory_count_for(seat) > 0 {
                if let Err(e) = validate_orders(&orders, seat, &state, &board) {
                    return Err(GameError::InvalidOrders(e.to_string()));
                }
            }
            player_orders[seat as usize] = orders;
        }

        let result = {
            let mut rng = self.rng.lock().await;
            resolve_turn(&state, player_orders, &board, &mut *rng)
        };

        let new_state = result.state;
        let new_turn = new_state.turn as i32;

        if new_state.phase == Phase::Finished {
            // Game over.
            if let Some(winner_seat) = new_state.winner {
                let winner_id = if winner_seat == 0 {
                    game.player_a.ok_or(GameError::NotFound)?
                } else {
                    game.player_b.ok_or(GameError::NotFound)?
                };
                let loser_id = if winner_seat == 0 {
                    game.player_b.ok_or(GameError::NotFound)?
                } else {
                    game.player_a.ok_or(GameError::NotFound)?
                };

                let state_json = serde_json::to_value(&new_state)
                    .map_err(|e| GameError::Serialization(e.to_string()))?;
                db::finish_game_tx(tx, game_id, winner_id, &state_json).await?;

                // Update ratings (uses pool directly, outside the row lock).
                // Best-effort with retry — don't fail the game resolution
                // if ratings can't be updated right now.
                if let Err(e) = self.update_ratings(game_id, winner_id, loser_id).await {
                    error!("Rating update failed for game {game_id}, retrying: {e}");
                    tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                    if let Err(e2) = self.update_ratings(game_id, winner_id, loser_id).await {
                        error!("Rating update retry failed for game {game_id}: {e2}");
                    }
                }

                self.hub
                    .broadcast(game_id, ws::GameEvent::GameFinished { game_id, winner_id }).await;
            }
        } else {
            let state_json = serde_json::to_value(&new_state)
                .map_err(|e| GameError::Serialization(e.to_string()))?;
            db::update_game_state_tx(tx, game_id, "active", new_turn, &state_json, None)
                .await?;

            // Set next turn deadline.
            let deadline = chrono::Utc::now() + chrono::Duration::hours(24);
            db::set_turn_deadline_tx(tx, game_id, new_turn, deadline).await?;

            self.hub.broadcast(
            game_id,
            ws::GameEvent::TurnResolved {
                    game_id,
                    turn: new_turn,
                },
            ).await;
        }

        Ok(())
    }

    /// Update ratings for winner and loser after a game.
    async fn update_ratings(
        &self,
        game_id: Uuid,
        winner_id: Uuid,
        loser_id: Uuid,
    ) -> Result<(), GameError> {
        let winner_user = db::get_user(&self.pool, winner_id)
            .await?
            .ok_or(GameError::NotFound)?;
        let loser_user = db::get_user(&self.pool, loser_id)
            .await?
            .ok_or(GameError::NotFound)?;

        let winner_rating = Rating {
            rating: winner_user.rating,
            rd: winner_user.rd,
            volatility: winner_user.volatility,
        };
        let loser_rating = Rating {
            rating: loser_user.rating,
            rd: loser_user.rd,
            volatility: loser_user.volatility,
        };

        let (new_winner, new_loser) =
            rating::update_ratings_after_game(winner_rating, loser_rating);

        db::update_user_rating(
            &self.pool,
            winner_id,
            new_winner.rating,
            new_winner.rd,
            new_winner.volatility,
        )
        .await?;
        db::update_user_rating(
            &self.pool,
            loser_id,
            new_loser.rating,
            new_loser.rd,
            new_loser.volatility,
        )
        .await?;

        db::increment_games_played(&self.pool, winner_id, true).await?;
        db::increment_games_played(&self.pool, loser_id, false).await?;

        db::insert_rating_history(
            &self.pool,
            winner_id,
            game_id,
            winner_rating.rating,
            new_winner.rating,
        )
        .await?;
        db::insert_rating_history(
            &self.pool,
            loser_id,
            game_id,
            loser_rating.rating,
            new_loser.rating,
        )
        .await?;

        Ok(())
    }

    /// Resign: forfeit the game, opponent wins.
    pub async fn resign(&self, game_id: Uuid, user_id: Uuid) -> Result<(), GameError> {
        let game = db::get_game(&self.pool, game_id)
            .await?
            .ok_or(GameError::NotFound)?;

        if game.status != "active" && game.status != "picking" {
            return Err(GameError::WrongStatus {
                expected: "active".to_string(),
                actual: game.status,
            });
        }

        let seat = self.player_seat(&game, user_id)?;
        let winner_seat: u8 = 1 - seat;
        let winner_id = if winner_seat == 0 {
            game.player_a.ok_or(GameError::NotFound)?
        } else {
            game.player_b.ok_or(GameError::NotFound)?
        };

        let state: GameState = serde_json::from_value(
            game.state_json.clone().ok_or(GameError::NotFound)?
        ).map_err(|e| GameError::Serialization(e.to_string()))?;

        let mut final_state = state;
        final_state.phase = Phase::Finished;
        final_state.winner = Some(winner_seat);
        final_state.alive[seat as usize] = false;

        let state_json = serde_json::to_value(&final_state)
            .map_err(|e| GameError::Serialization(e.to_string()))?;

        db::finish_game(&self.pool, game_id, winner_id, &state_json).await?;

        if let Err(e) = self.update_ratings(game_id, winner_id, user_id).await {
            tracing::error!("Rating update failed after resign: {e}");
        }

        self.hub.broadcast(game_id, ws::GameEvent::GameFinished {
            game_id,
            winner_id,
        }).await;

        Ok(())
    }

    /// Determine which seat (0 or 1) a user occupies in a game.
    fn player_seat(&self, game: &db::GameRow, user_id: Uuid) -> Result<u8, GameError> {
        self.player_seat_by_id(game, user_id)
    }

    fn player_seat_by_id(&self, game: &db::GameRow, user_id: Uuid) -> Result<u8, GameError> {
        if game.player_a == Some(user_id) {
            Ok(0)
        } else if game.player_b == Some(user_id) {
            Ok(1)
        } else {
            Err(GameError::NotParticipant)
        }
    }
}
