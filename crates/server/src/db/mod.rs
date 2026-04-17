pub mod migrate;

use chrono::{DateTime, Utc};
use sqlx::{PgPool, Postgres, Row};
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

/// Fetch a game row with `FOR UPDATE` lock within an existing transaction.
/// This serializes concurrent access to the same game row.
pub async fn get_game_for_update(
    tx: &mut sqlx::Transaction<'_, Postgres>,
    game_id: Uuid,
) -> Result<Option<GameRow>, sqlx::Error> {
    sqlx::query_as::<_, GameRow>("SELECT * FROM games WHERE id = $1 FOR UPDATE")
        .bind(game_id)
        .fetch_optional(&mut **tx)
        .await
}

/// Fetch the deadline for a specific game turn.
pub async fn get_turn_deadline(
    pool: &PgPool,
    game_id: Uuid,
    turn: i32,
) -> Result<Option<DateTime<Utc>>, sqlx::Error> {
    let row = sqlx::query("SELECT deadline FROM turn_deadlines WHERE game_id = $1 AND turn = $2")
        .bind(game_id)
        .bind(turn)
        .fetch_optional(pool)
        .await?;
    Ok(row.map(|r| r.get("deadline")))
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

/// Update game state within an existing transaction.
pub async fn update_game_state_tx(
    tx: &mut sqlx::Transaction<'_, Postgres>,
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
    .execute(&mut **tx)
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

/// Finish a game within an existing transaction.
pub async fn finish_game_tx(
    tx: &mut sqlx::Transaction<'_, Postgres>,
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
    .execute(&mut **tx)
    .await?;
    Ok(())
}

/// Set a turn deadline (non-transactional).
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

/// Set a turn deadline within an existing transaction.
pub async fn set_turn_deadline_tx(
    tx: &mut sqlx::Transaction<'_, Postgres>,
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
    .execute(&mut **tx)
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

/// Insert orders within an existing transaction.
pub async fn insert_orders_tx(
    tx: &mut sqlx::Transaction<'_, Postgres>,
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
    .fetch_one(&mut **tx)
    .await
}

/// Fetch orders for a turn within an existing transaction.
pub async fn get_orders_for_turn_tx(
    tx: &mut sqlx::Transaction<'_, Postgres>,
    game_id: Uuid,
    turn: i32,
) -> Result<Vec<OrderRow>, sqlx::Error> {
    sqlx::query_as::<_, OrderRow>("SELECT * FROM orders WHERE game_id = $1 AND turn = $2")
        .bind(game_id)
        .bind(turn)
        .fetch_all(&mut **tx)
        .await
}

// ── Turn deadline queries ──

pub async fn get_expired_deadlines(pool: &PgPool) -> Result<Vec<(Uuid, i32)>, sqlx::Error> {
    let rows: Vec<(Uuid, i32)> = sqlx::query_as(
        r#"
        SELECT td.game_id, td.turn
        FROM turn_deadlines td
        JOIN games g ON g.id = td.game_id
        WHERE td.deadline < now() AND g.status IN ('active', 'picking')
        "#,
    )
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

// ── Rating history queries ──

// ── Season queries ──

/// A season row from the database.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, sqlx::FromRow)]
pub struct SeasonRow {
    pub id: i32,
    pub name: String,
    pub starts_at: DateTime<Utc>,
    pub ends_at: DateTime<Utc>,
    pub is_active: bool,
    pub config: serde_json::Value,
}

/// A season standing row from the database.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, sqlx::FromRow)]
pub struct SeasonStandingRow {
    pub season_id: i32,
    pub user_id: Uuid,
    pub rank_tier: String,
    pub rank_points: i32,
    pub wins: i32,
    pub losses: i32,
    pub streak: i32,
    pub peak_rank_points: i32,
}

/// A match history row from the database.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, sqlx::FromRow)]
pub struct MatchHistoryRow {
    pub id: Uuid,
    pub game_id: Uuid,
    pub season_id: Option<i32>,
    pub player_a: Uuid,
    pub player_b: Uuid,
    pub winner_id: Option<Uuid>,
    pub player_a_rating_change: Option<f64>,
    pub player_b_rating_change: Option<f64>,
    pub player_a_rp_change: Option<i32>,
    pub player_b_rp_change: Option<i32>,
    pub turns_played: Option<i32>,
    pub template: Option<String>,
    pub played_at: DateTime<Utc>,
}

#[allow(dead_code)]
pub async fn create_season(
    pool: &PgPool,
    name: &str,
    starts_at: DateTime<Utc>,
    ends_at: DateTime<Utc>,
    config: &serde_json::Value,
) -> Result<SeasonRow, sqlx::Error> {
    sqlx::query_as::<_, SeasonRow>(
        r#"
        INSERT INTO seasons (name, starts_at, ends_at, config)
        VALUES ($1, $2, $3, $4)
        RETURNING *
        "#,
    )
    .bind(name)
    .bind(starts_at)
    .bind(ends_at)
    .bind(config)
    .fetch_one(pool)
    .await
}

pub async fn get_active_season(pool: &PgPool) -> Result<Option<SeasonRow>, sqlx::Error> {
    sqlx::query_as::<_, SeasonRow>("SELECT * FROM seasons WHERE is_active = true LIMIT 1")
        .fetch_optional(pool)
        .await
}

pub async fn list_seasons(pool: &PgPool) -> Result<Vec<SeasonRow>, sqlx::Error> {
    sqlx::query_as::<_, SeasonRow>("SELECT * FROM seasons ORDER BY starts_at DESC")
        .fetch_all(pool)
        .await
}

pub async fn get_season(pool: &PgPool, season_id: i32) -> Result<Option<SeasonRow>, sqlx::Error> {
    sqlx::query_as::<_, SeasonRow>("SELECT * FROM seasons WHERE id = $1")
        .bind(season_id)
        .fetch_optional(pool)
        .await
}

pub async fn get_or_create_standing(
    pool: &PgPool,
    season_id: i32,
    user_id: Uuid,
) -> Result<SeasonStandingRow, sqlx::Error> {
    sqlx::query_as::<_, SeasonStandingRow>(
        r#"
        INSERT INTO season_standings (season_id, user_id)
        VALUES ($1, $2)
        ON CONFLICT (season_id, user_id) DO NOTHING
        RETURNING *
        "#,
    )
    .bind(season_id)
    .bind(user_id)
    .fetch_optional(pool)
    .await?;

    // Always fetch to ensure we get the row (whether just inserted or pre-existing).
    sqlx::query_as::<_, SeasonStandingRow>(
        "SELECT * FROM season_standings WHERE season_id = $1 AND user_id = $2",
    )
    .bind(season_id)
    .bind(user_id)
    .fetch_one(pool)
    .await
}

#[allow(dead_code, clippy::too_many_arguments)]
pub async fn update_standing(
    pool: &PgPool,
    season_id: i32,
    user_id: Uuid,
    rank_tier: &str,
    rank_points: i32,
    won: bool,
    streak: i32,
    peak_rank_points: i32,
) -> Result<(), sqlx::Error> {
    if won {
        sqlx::query(
            r#"
            UPDATE season_standings
            SET rank_tier = $3, rank_points = $4, wins = wins + 1,
                streak = $5, peak_rank_points = $6
            WHERE season_id = $1 AND user_id = $2
            "#,
        )
    } else {
        sqlx::query(
            r#"
            UPDATE season_standings
            SET rank_tier = $3, rank_points = $4, losses = losses + 1,
                streak = $5, peak_rank_points = $6
            WHERE season_id = $1 AND user_id = $2
            "#,
        )
    }
    .bind(season_id)
    .bind(user_id)
    .bind(rank_tier)
    .bind(rank_points)
    .bind(streak)
    .bind(peak_rank_points)
    .execute(pool)
    .await?;
    Ok(())
}

/// Standing row joined with user info for leaderboard display.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, sqlx::FromRow)]
pub struct LeaderboardEntry {
    pub user_id: Uuid,
    pub username: String,
    pub avatar_url: Option<String>,
    pub rank_tier: String,
    pub rank_points: i32,
    pub wins: i32,
    pub losses: i32,
    pub streak: i32,
    pub peak_rank_points: i32,
}

pub async fn get_season_leaderboard(
    pool: &PgPool,
    season_id: i32,
    limit: i64,
) -> Result<Vec<LeaderboardEntry>, sqlx::Error> {
    sqlx::query_as::<_, LeaderboardEntry>(
        r#"
        SELECT s.user_id, u.username, u.avatar_url,
               s.rank_tier, s.rank_points, s.wins, s.losses,
               s.streak, s.peak_rank_points
        FROM season_standings s
        JOIN users u ON u.id = s.user_id
        WHERE s.season_id = $1
        ORDER BY s.rank_points DESC
        LIMIT $2
        "#,
    )
    .bind(season_id)
    .bind(limit)
    .fetch_all(pool)
    .await
}

#[allow(dead_code, clippy::too_many_arguments)]
pub async fn create_match_history(
    pool: &PgPool,
    game_id: Uuid,
    season_id: Option<i32>,
    player_a: Uuid,
    player_b: Uuid,
    winner_id: Option<Uuid>,
    player_a_rating_change: Option<f64>,
    player_b_rating_change: Option<f64>,
    player_a_rp_change: Option<i32>,
    player_b_rp_change: Option<i32>,
    turns_played: Option<i32>,
    template: Option<&str>,
) -> Result<MatchHistoryRow, sqlx::Error> {
    sqlx::query_as::<_, MatchHistoryRow>(
        r#"
        INSERT INTO match_history (
            game_id, season_id, player_a, player_b, winner_id,
            player_a_rating_change, player_b_rating_change,
            player_a_rp_change, player_b_rp_change,
            turns_played, template
        )
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)
        RETURNING *
        "#,
    )
    .bind(game_id)
    .bind(season_id)
    .bind(player_a)
    .bind(player_b)
    .bind(winner_id)
    .bind(player_a_rating_change)
    .bind(player_b_rating_change)
    .bind(player_a_rp_change)
    .bind(player_b_rp_change)
    .bind(turns_played)
    .bind(template)
    .fetch_one(pool)
    .await
}

pub async fn get_match_history(
    pool: &PgPool,
    user_id: Option<Uuid>,
    season_id: Option<i32>,
    limit: i64,
    offset: i64,
) -> Result<Vec<MatchHistoryRow>, sqlx::Error> {
    // Build query dynamically based on filters.
    match (user_id, season_id) {
        (Some(uid), Some(sid)) => {
            sqlx::query_as::<_, MatchHistoryRow>(
                r#"
                SELECT * FROM match_history
                WHERE (player_a = $1 OR player_b = $1) AND season_id = $2
                ORDER BY played_at DESC
                LIMIT $3 OFFSET $4
                "#,
            )
            .bind(uid)
            .bind(sid)
            .bind(limit)
            .bind(offset)
            .fetch_all(pool)
            .await
        }
        (Some(uid), None) => {
            sqlx::query_as::<_, MatchHistoryRow>(
                r#"
                SELECT * FROM match_history
                WHERE player_a = $1 OR player_b = $1
                ORDER BY played_at DESC
                LIMIT $2 OFFSET $3
                "#,
            )
            .bind(uid)
            .bind(limit)
            .bind(offset)
            .fetch_all(pool)
            .await
        }
        (None, Some(sid)) => {
            sqlx::query_as::<_, MatchHistoryRow>(
                r#"
                SELECT * FROM match_history
                WHERE season_id = $1
                ORDER BY played_at DESC
                LIMIT $2 OFFSET $3
                "#,
            )
            .bind(sid)
            .bind(limit)
            .bind(offset)
            .fetch_all(pool)
            .await
        }
        (None, None) => {
            sqlx::query_as::<_, MatchHistoryRow>(
                r#"
                SELECT * FROM match_history
                ORDER BY played_at DESC
                LIMIT $1 OFFSET $2
                "#,
            )
            .bind(limit)
            .bind(offset)
            .fetch_all(pool)
            .await
        }
    }
}

// ── Rating history queries ──

// ── Arena queries ──

/// An arena row from the database.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, sqlx::FromRow)]
pub struct ArenaRow {
    pub id: Uuid,
    pub name: String,
    pub template: String,
    pub start_time: DateTime<Utc>,
    pub end_time: DateTime<Utc>,
    pub time_control_secs: i32,
    pub created_at: DateTime<Utc>,
}

/// An arena participant row joined with user info.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, sqlx::FromRow)]
pub struct ArenaParticipantRow {
    pub arena_id: Uuid,
    pub user_id: Uuid,
    pub username: String,
    pub avatar_url: Option<String>,
    pub score: i32,
    pub games_played: i32,
    pub wins: i32,
    pub current_streak: i32,
}

pub async fn create_arena(
    pool: &PgPool,
    name: &str,
    template: &str,
    start_time: DateTime<Utc>,
    end_time: DateTime<Utc>,
    time_control_secs: i32,
) -> Result<ArenaRow, sqlx::Error> {
    sqlx::query_as::<_, ArenaRow>(
        r#"
        INSERT INTO arenas (name, template, start_time, end_time, time_control_secs)
        VALUES ($1, $2, $3, $4, $5)
        RETURNING *
        "#,
    )
    .bind(name)
    .bind(template)
    .bind(start_time)
    .bind(end_time)
    .bind(time_control_secs)
    .fetch_one(pool)
    .await
}

pub async fn get_arena(pool: &PgPool, arena_id: Uuid) -> Result<Option<ArenaRow>, sqlx::Error> {
    sqlx::query_as::<_, ArenaRow>("SELECT * FROM arenas WHERE id = $1")
        .bind(arena_id)
        .fetch_optional(pool)
        .await
}

/// List arenas that are active (currently running) or upcoming (not yet started).
pub async fn list_active_arenas(pool: &PgPool) -> Result<Vec<ArenaRow>, sqlx::Error> {
    sqlx::query_as::<_, ArenaRow>(
        "SELECT * FROM arenas WHERE end_time > now() ORDER BY start_time ASC",
    )
    .fetch_all(pool)
    .await
}

/// Get all participants for an arena, joined with user info.
pub async fn get_arena_participants(
    pool: &PgPool,
    arena_id: Uuid,
) -> Result<Vec<ArenaParticipantRow>, sqlx::Error> {
    sqlx::query_as::<_, ArenaParticipantRow>(
        r#"
        SELECT ap.arena_id, ap.user_id, u.username, u.avatar_url,
               ap.score, ap.games_played, ap.wins, ap.current_streak
        FROM arena_participants ap
        JOIN users u ON u.id = ap.user_id
        WHERE ap.arena_id = $1
        ORDER BY ap.score DESC, ap.wins DESC
        "#,
    )
    .bind(arena_id)
    .fetch_all(pool)
    .await
}

/// Join an arena (idempotent — does nothing if already joined).
pub async fn join_arena(pool: &PgPool, arena_id: Uuid, user_id: Uuid) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"
        INSERT INTO arena_participants (arena_id, user_id)
        VALUES ($1, $2)
        ON CONFLICT (arena_id, user_id) DO NOTHING
        "#,
    )
    .bind(arena_id)
    .bind(user_id)
    .execute(pool)
    .await?;
    Ok(())
}

/// Update a participant's stats after a game result.
#[allow(dead_code)]
pub async fn update_arena_participant(
    pool: &PgPool,
    arena_id: Uuid,
    user_id: Uuid,
    score_delta: i32,
    won: bool,
    new_streak: i32,
) -> Result<(), sqlx::Error> {
    if won {
        sqlx::query(
            r#"
            UPDATE arena_participants
            SET score = score + $3, games_played = games_played + 1,
                wins = wins + 1, current_streak = $4
            WHERE arena_id = $1 AND user_id = $2
            "#,
        )
    } else {
        sqlx::query(
            r#"
            UPDATE arena_participants
            SET score = score + $3, games_played = games_played + 1,
                current_streak = $4
            WHERE arena_id = $1 AND user_id = $2
            "#,
        )
    }
    .bind(arena_id)
    .bind(user_id)
    .bind(score_delta)
    .bind(new_streak)
    .execute(pool)
    .await?;
    Ok(())
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
