pub mod migrate;

use chrono::{DateTime, Utc};
use sqlx::PgPool;
use uuid::Uuid;

/// A user row from the database.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, sqlx::FromRow)]
pub struct UserRow {
    pub id: Uuid,
    pub discord_id: i64,
    pub username: String,
    pub avatar_url: Option<String>,
    pub rating: f64,
    pub rd: f64,
    pub volatility: f64,
    pub games_played: i32,
    pub games_won: i32,
    pub created_at: DateTime<Utc>,
}

/// A game row from the database.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, sqlx::FromRow)]
pub struct GameRow {
    pub id: Uuid,
    pub template: String,
    pub status: String,
    pub player_a: Option<Uuid>,
    pub player_b: Option<Uuid>,
    pub winner_id: Option<Uuid>,
    pub turn: i32,
    pub state_json: Option<serde_json::Value>,
    pub map_json: Option<serde_json::Value>,
    pub pick_options: Option<serde_json::Value>,
    pub created_at: DateTime<Utc>,
    pub finished_at: Option<DateTime<Utc>>,
}

/// An orders row from the database.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, sqlx::FromRow)]
pub struct OrderRow {
    pub id: Uuid,
    pub game_id: Uuid,
    pub user_id: Uuid,
    pub turn: i32,
    pub orders_json: serde_json::Value,
    pub submitted_at: DateTime<Utc>,
}

// ── User queries ──

pub async fn upsert_user(
    pool: &PgPool,
    discord_id: i64,
    username: &str,
    avatar_url: Option<&str>,
) -> Result<UserRow, sqlx::Error> {
    sqlx::query_as::<_, UserRow>(
        r#"
        INSERT INTO users (discord_id, username, avatar_url)
        VALUES ($1, $2, $3)
        ON CONFLICT (discord_id) DO UPDATE
            SET username = EXCLUDED.username,
                avatar_url = EXCLUDED.avatar_url
        RETURNING *
        "#,
    )
    .bind(discord_id)
    .bind(username)
    .bind(avatar_url)
    .fetch_one(pool)
    .await
}

pub async fn get_user(pool: &PgPool, user_id: Uuid) -> Result<Option<UserRow>, sqlx::Error> {
    sqlx::query_as::<_, UserRow>("SELECT * FROM users WHERE id = $1")
        .bind(user_id)
        .fetch_optional(pool)
        .await
}

#[allow(dead_code)]
pub async fn get_user_by_discord_id(
    pool: &PgPool,
    discord_id: i64,
) -> Result<Option<UserRow>, sqlx::Error> {
    sqlx::query_as::<_, UserRow>("SELECT * FROM users WHERE discord_id = $1")
        .bind(discord_id)
        .fetch_optional(pool)
        .await
}

pub async fn update_user_rating(
    pool: &PgPool,
    user_id: Uuid,
    rating: f64,
    rd: f64,
    volatility: f64,
) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE users SET rating = $2, rd = $3, volatility = $4 WHERE id = $1")
        .bind(user_id)
        .bind(rating)
        .bind(rd)
        .bind(volatility)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn increment_games_played(
    pool: &PgPool,
    user_id: Uuid,
    won: bool,
) -> Result<(), sqlx::Error> {
    if won {
        sqlx::query(
            "UPDATE users SET games_played = games_played + 1, games_won = games_won + 1 WHERE id = $1",
        )
        .bind(user_id)
        .execute(pool)
        .await?;
    } else {
        sqlx::query("UPDATE users SET games_played = games_played + 1 WHERE id = $1")
            .bind(user_id)
            .execute(pool)
            .await?;
    }
    Ok(())
}

pub async fn get_leaderboard(pool: &PgPool, limit: i64) -> Result<Vec<UserRow>, sqlx::Error> {
    sqlx::query_as::<_, UserRow>(
        "SELECT * FROM users WHERE games_played > 0 ORDER BY rating DESC LIMIT $1",
    )
    .bind(limit)
    .fetch_all(pool)
    .await
}

// ── Game queries ──

pub async fn create_game(
    pool: &PgPool,
    template: &str,
    player_a: Uuid,
    state_json: &serde_json::Value,
    map_json: &serde_json::Value,
) -> Result<GameRow, sqlx::Error> {
    sqlx::query_as::<_, GameRow>(
        r#"
        INSERT INTO games (template, status, player_a, state_json, map_json)
        VALUES ($1, 'waiting', $2, $3, $4)
        RETURNING *
        "#,
    )
    .bind(template)
    .bind(player_a)
    .bind(state_json)
    .bind(map_json)
    .fetch_one(pool)
    .await
}

pub async fn get_game(pool: &PgPool, game_id: Uuid) -> Result<Option<GameRow>, sqlx::Error> {
    sqlx::query_as::<_, GameRow>("SELECT * FROM games WHERE id = $1")
        .bind(game_id)
        .fetch_optional(pool)
        .await
}

pub async fn update_game_state(
    pool: &PgPool,
    game_id: Uuid,
    status: &str,
    turn: i32,
    state_json: &serde_json::Value,
    pick_options: Option<&serde_json::Value>,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"
        UPDATE games SET status = $2, turn = $3, state_json = $4, pick_options = $5
        WHERE id = $1
        "#,
    )
    .bind(game_id)
    .bind(status)
    .bind(turn)
    .bind(state_json)
    .bind(pick_options)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn set_game_player_b(
    pool: &PgPool,
    game_id: Uuid,
    player_b: Uuid,
) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE games SET player_b = $2 WHERE id = $1")
        .bind(game_id)
        .bind(player_b)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn finish_game(
    pool: &PgPool,
    game_id: Uuid,
    winner_id: Uuid,
    state_json: &serde_json::Value,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"
        UPDATE games SET status = 'finished', winner_id = $2, state_json = $3, finished_at = now()
        WHERE id = $1
        "#,
    )
    .bind(game_id)
    .bind(winner_id)
    .bind(state_json)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn list_games_by_status(
    pool: &PgPool,
    status: &str,
    limit: i64,
) -> Result<Vec<GameRow>, sqlx::Error> {
    sqlx::query_as::<_, GameRow>(
        "SELECT * FROM games WHERE status = $1 ORDER BY created_at DESC LIMIT $2",
    )
    .bind(status)
    .bind(limit)
    .fetch_all(pool)
    .await
}

pub async fn list_user_games(
    pool: &PgPool,
    user_id: Uuid,
    limit: i64,
) -> Result<Vec<GameRow>, sqlx::Error> {
    sqlx::query_as::<_, GameRow>(
        r#"
        SELECT * FROM games
        WHERE (player_a = $1 OR player_b = $1)
        ORDER BY created_at DESC
        LIMIT $2
        "#,
    )
    .bind(user_id)
    .bind(limit)
    .fetch_all(pool)
    .await
}

// ── Orders queries ──

pub async fn insert_orders(
    pool: &PgPool,
    game_id: Uuid,
    user_id: Uuid,
    turn: i32,
    orders_json: &serde_json::Value,
) -> Result<OrderRow, sqlx::Error> {
    sqlx::query_as::<_, OrderRow>(
        r#"
        INSERT INTO orders (game_id, user_id, turn, orders_json)
        VALUES ($1, $2, $3, $4)
        ON CONFLICT (game_id, user_id, turn) DO UPDATE
            SET orders_json = EXCLUDED.orders_json,
                submitted_at = now()
        RETURNING *
        "#,
    )
    .bind(game_id)
    .bind(user_id)
    .bind(turn)
    .bind(orders_json)
    .fetch_one(pool)
    .await
}

pub async fn get_orders_for_turn(
    pool: &PgPool,
    game_id: Uuid,
    turn: i32,
) -> Result<Vec<OrderRow>, sqlx::Error> {
    sqlx::query_as::<_, OrderRow>(
        "SELECT * FROM orders WHERE game_id = $1 AND turn = $2",
    )
    .bind(game_id)
    .bind(turn)
    .fetch_all(pool)
    .await
}

// ── Turn deadline queries ──

pub async fn set_turn_deadline(
    pool: &PgPool,
    game_id: Uuid,
    turn: i32,
    deadline: DateTime<Utc>,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"
        INSERT INTO turn_deadlines (game_id, turn, deadline)
        VALUES ($1, $2, $3)
        ON CONFLICT (game_id, turn) DO UPDATE SET deadline = EXCLUDED.deadline
        "#,
    )
    .bind(game_id)
    .bind(turn)
    .bind(deadline)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn get_expired_deadlines(
    pool: &PgPool,
) -> Result<Vec<(Uuid, i32)>, sqlx::Error> {
    let rows: Vec<(Uuid, i32)> = sqlx::query_as(
        r#"
        SELECT td.game_id, td.turn
        FROM turn_deadlines td
        JOIN games g ON g.id = td.game_id
        WHERE td.deadline < now() AND g.status = 'active'
        "#,
    )
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

// ── Rating history queries ──

pub async fn insert_rating_history(
    pool: &PgPool,
    user_id: Uuid,
    game_id: Uuid,
    old_rating: f64,
    new_rating: f64,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"
        INSERT INTO rating_history (user_id, game_id, old_rating, new_rating)
        VALUES ($1, $2, $3, $4)
        "#,
    )
    .bind(user_id)
    .bind(game_id)
    .bind(old_rating)
    .bind(new_rating)
    .execute(pool)
    .await?;
    Ok(())
}
