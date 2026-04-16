mod api;
mod auth;
mod config;
mod db;
mod game;
mod ws;

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{Html, IntoResponse, Redirect};
use axum::routing::{get, post};
use axum::{Json, Router};
use rand::rngs::StdRng;
use rand::SeedableRng;
use serde::{Deserialize, Serialize};
use tower_http::cors::CorsLayer;
use tracing::info;

use strat_engine::ai::{self, AiStrength};
use strat_engine::analysis;
use strat_engine::fog;
use strat_engine::map::Map;
use strat_engine::orders::Order;
use strat_engine::picking;
use strat_engine::state::{GameState, Phase, PlayerId, NEUTRAL};
use strat_engine::turn::{resolve_turn, TurnEvent};

use crate::config::Config;

const PLAYER: PlayerId = 0;
const AI_PLAYER: PlayerId = 1;

// ── Application state ──

/// Shared application state for all routes.
#[derive(Clone)]
pub struct AppState {
    pub config: Arc<Config>,
    /// Database pool (None if no DATABASE_URL configured -- local play only).
    pub db_pool: Option<sqlx::PgPool>,
    /// WebSocket hub for multiplayer event broadcasting.
    pub ws_hub: Arc<ws::Hub>,
    /// Game manager for multiplayer games (None if no DB).
    pub game_manager: Option<Arc<game::manager::GameManager>>,
    /// Matchmaking queue for real-time player pairing.
    pub matchmaking: Arc<game::matchmaking::MatchmakingQueue>,
    /// Local (single-player vs AI) game state.
    pub local: Arc<Mutex<LocalState>>,
}

impl AppState {
    /// Get the database pool or return an error suitable for API responses.
    pub fn require_db(&self) -> Result<&sqlx::PgPool, (StatusCode, String)> {
        self.db_pool.as_ref().ok_or((
            StatusCode::SERVICE_UNAVAILABLE,
            "database not configured; multiplayer features require DATABASE_URL".to_string(),
        ))
    }

    /// Get the game manager or return an error.
    pub fn require_game_manager(
        &self,
    ) -> Result<&Arc<game::manager::GameManager>, (StatusCode, String)> {
        self.game_manager.as_ref().ok_or((
            StatusCode::SERVICE_UNAVAILABLE,
            "multiplayer not configured; requires DATABASE_URL".to_string(),
        ))
    }
}

// ── Local play state (preserved from original server) ──

pub struct LocalState {
    map: Map,
    game: GameState,
    rng: StdRng,
    pick_options: Vec<usize>,
    turn_history: Vec<TurnLog>,
    ai_strength: AiStrength,
    /// Complete game state snapshots before each turn's orders (for replay).
    state_history: Vec<GameState>,
    /// Player 0 win probability recorded after each turn resolves.
    win_prob_history: Vec<f64>,
    /// Persistent local play statistics across games.
    local_stats: LocalStats,
}

/// Cumulative statistics for local play sessions (no database required).
#[derive(Clone, Serialize, Default)]
struct LocalStats {
    games_played: u32,
    wins: u32,
    losses: u32,
    total_turns: u32,
    /// Positive = win streak, negative = loss streak.
    streak: i32,
    /// Glicko rating after each completed game.
    rating_history: Vec<f64>,
    /// Current Glicko rating state (not serialized to JSON).
    #[serde(skip)]
    rating: game::rating::Rating,
    /// Win probability at game start for each completed game.
    start_win_probs: Vec<f64>,
    /// Map name for each completed game.
    game_maps: Vec<String>,
    /// Turn count for each completed game.
    game_turns: Vec<u32>,
    /// Whether the player won each completed game.
    game_results: Vec<bool>,
    /// Bonus names the player has fully captured, with frequency.
    bonus_captures: Vec<(String, u32)>,
    /// Territory names the player picked at game start, with frequency.
    pick_choices: Vec<(String, u32)>,
}

#[derive(Clone, Serialize)]
struct TurnLog {
    turn: u32,
    events: Vec<TurnEvent>,
}

// ── Local play API response types ──

#[derive(Serialize)]
struct GameView {
    phase: String,
    turn: u32,
    income: u32,
    my_territories: u32,
    enemy_territories: u32,
    winner: Option<u8>,
    pick_options: Vec<usize>,
    picks_needed: usize,
    territories: Vec<TerritoryView>,
    bonuses: Vec<BonusView>,
    history: Vec<TurnLog>,
    win_probability: WinProbView,
    win_prob_history: Vec<f64>,
}

#[derive(Serialize)]
struct WinProbView {
    player_0: f64,
    player_1: f64,
    simulations: u32,
}

#[derive(Serialize)]
struct TerritoryView {
    id: usize,
    name: String,
    bonus_id: usize,
    adjacent: Vec<usize>,
    owner: u8,
    armies: u32,
    visible: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    path: Option<String>,
    label_x: f64,
    label_y: f64,
}

#[derive(Serialize)]
struct BonusView {
    id: usize,
    name: String,
    value: u32,
    territory_ids: Vec<usize>,
    player_count: usize,
    total: usize,
}

#[derive(Deserialize)]
struct PicksRequest {
    picks: Vec<usize>,
}

#[derive(Deserialize)]
struct OrdersRequest {
    orders: Vec<Order>,
}

#[derive(Serialize)]
struct ActionResult {
    success: bool,
    message: String,
    events: Vec<TurnEvent>,
}

fn build_game_view(app: &LocalState) -> GameView {
    let visible = fog::visible_territories(&app.game, PLAYER, &app.map);
    let is_picking = app.game.phase == Phase::Picking;

    let territories: Vec<TerritoryView> = app
        .map
        .territories
        .iter()
        .enumerate()
        .map(|(i, t)| {
            let is_visible = is_picking || visible.contains(&i) || !app.map.settings.fog_of_war;
            let (lx, ly) = t
                .visual
                .as_ref()
                .map(|v| (v.label_pos[0], v.label_pos[1]))
                .unwrap_or((0.0, 0.0));
            TerritoryView {
                id: i,
                name: t.name.clone(),
                bonus_id: t.bonus_id,
                adjacent: t.adjacent.clone(),
                owner: if is_visible {
                    app.game.territory_owners[i]
                } else {
                    NEUTRAL
                },
                armies: if is_visible {
                    app.game.territory_armies[i]
                } else {
                    0
                },
                visible: is_visible,
                path: t.visual.as_ref().map(|v| v.path.clone()),
                label_x: lx,
                label_y: ly,
            }
        })
        .collect();

    let bonuses: Vec<BonusView> = app
        .map
        .bonuses
        .iter()
        .map(|b| {
            let player_count = b
                .territory_ids
                .iter()
                .filter(|&&tid| app.game.territory_owners[tid] == PLAYER)
                .count();
            BonusView {
                id: b.id,
                name: b.name.clone(),
                value: b.value,
                territory_ids: b.territory_ids.clone(),
                player_count,
                total: b.territory_ids.len(),
            }
        })
        .collect();

    // Compute win probability using fast material evaluation (< 1ms).
    let wp = analysis::quick_win_probability(&app.game, &app.map);

    GameView {
        phase: match app.game.phase {
            Phase::Picking => "picking".into(),
            Phase::Play => "play".into(),
            Phase::Finished => "finished".into(),
        },
        turn: app.game.turn,
        income: app.game.income(PLAYER, &app.map),
        my_territories: app.game.territory_count_for(PLAYER) as u32,
        enemy_territories: app.game.territory_count_for(AI_PLAYER) as u32,
        winner: app.game.winner,
        pick_options: app.pick_options.clone(),
        picks_needed: app.map.picking.num_picks,
        territories,
        bonuses,
        history: app.turn_history.clone(),
        win_probability: WinProbView {
            player_0: wp.player_0,
            player_1: wp.player_1,
            simulations: wp.simulations_run,
        },
        win_prob_history: app.win_prob_history.clone(),
    }
}

fn fog_filter_events(
    events: &[TurnEvent],
    visible_before: &std::collections::HashSet<usize>,
    visible_after: &std::collections::HashSet<usize>,
) -> Vec<TurnEvent> {
    events
        .iter()
        .filter(|e| match e {
            TurnEvent::Deploy { territory, .. } => visible_after.contains(territory),
            TurnEvent::Attack { from, to, .. } => {
                visible_before.contains(from)
                    || visible_before.contains(to)
                    || visible_after.contains(from)
                    || visible_after.contains(to)
            }
            TurnEvent::Transfer { from, to, .. } => {
                visible_after.contains(from) || visible_after.contains(to)
            }
            TurnEvent::Capture { territory, .. } => visible_after.contains(territory),
            TurnEvent::Blockade { territory, .. } => visible_after.contains(territory),
            TurnEvent::Eliminated { .. } | TurnEvent::Victory { .. } => true,
        })
        .cloned()
        .collect()
}

// ── Local play route handlers ──

async fn get_local_game(State(state): State<AppState>) -> Json<GameView> {
    let app = state.local.lock().unwrap();
    Json(build_game_view(&app))
}

async fn submit_local_picks(
    State(state): State<AppState>,
    Json(body): Json<PicksRequest>,
) -> Result<Json<ActionResult>, (StatusCode, String)> {
    let mut app = state.local.lock().unwrap();

    if app.game.phase != Phase::Picking {
        return Err((StatusCode::BAD_REQUEST, "Not in picking phase".into()));
    }

    let needed = app.map.picking.num_picks;
    if body.picks.len() < needed {
        return Err((
            StatusCode::BAD_REQUEST,
            format!("Need at least {} picks", needed),
        ));
    }

    // Track pick choices in local stats.
    for &tid in &body.picks {
        if let Some(t) = app.map.territories.get(tid) {
            let name = t.name.clone();
            if let Some(entry) = app
                .local_stats
                .pick_choices
                .iter_mut()
                .find(|(n, _)| n == &name)
            {
                entry.1 += 1;
            } else {
                app.local_stats.pick_choices.push((name, 1));
            }
        }
    }

    let ai_picks = ai::generate_picks(&app.game, &app.map);
    let map = app.map.clone();

    picking::resolve_picks(&mut app.game, [&body.picks, &ai_picks], &map);

    Ok(Json(ActionResult {
        success: true,
        message: "Picks resolved. Game begins!".into(),
        events: Vec::new(),
    }))
}

async fn submit_local_orders(
    State(state): State<AppState>,
    Json(body): Json<OrdersRequest>,
) -> Result<Json<ActionResult>, (StatusCode, String)> {
    let mut app = state.local.lock().unwrap();

    if app.game.phase != Phase::Play {
        return Err((StatusCode::BAD_REQUEST, "Not in play phase".into()));
    }

    let visible_before = fog::visible_territories(&app.game, PLAYER, &app.map);

    // Snapshot the current state before resolving orders (for replay).
    let state_snapshot = app.game.clone();
    app.state_history.push(state_snapshot);

    let ai_orders = ai::generate_orders_for_strength(&app.game, AI_PLAYER, &app.map, app.ai_strength);
    let game_clone = app.game.clone();
    let map = app.map.clone();

    let result = resolve_turn(&game_clone, [body.orders, ai_orders], &map, &mut app.rng);

    let visible_after = fog::visible_territories(&result.state, PLAYER, &app.map);
    let filtered_events = fog_filter_events(&result.events, &visible_before, &visible_after);

    let turn_num = app.game.turn;
    app.turn_history.push(TurnLog {
        turn: turn_num,
        events: filtered_events.clone(),
    });

    app.game = result.state;

    // Record win probability after the turn resolves (1-ply lookahead for history).
    let wp = analysis::win_probability_with_lookahead(&app.game, &app.map);
    app.win_prob_history.push(wp.player_0);

    let msg = if app.game.phase == Phase::Finished {
        let won = app.game.winner == Some(PLAYER);
        // Record stats for completed game.
        app.local_stats.games_played += 1;
        app.local_stats.total_turns += app.game.turn;
        if won {
            app.local_stats.wins += 1;
            app.local_stats.streak = app.local_stats.streak.max(0) + 1;
        } else {
            app.local_stats.losses += 1;
            app.local_stats.streak = app.local_stats.streak.min(0) - 1;
        }
        // Update Glicko rating against a default-rated AI opponent.
        let ai_rating = game::rating::Rating::default();
        let outcome = if won {
            game::rating::Outcome::Win
        } else {
            game::rating::Outcome::Loss
        };
        app.local_stats.rating =
            game::rating::update_rating(app.local_stats.rating, ai_rating, outcome);
        let new_rating = app.local_stats.rating.rating;
        app.local_stats.rating_history.push(new_rating);
        // Record per-game metadata.
        let map_name = app.map.name.clone();
        let game_turn = app.game.turn;
        let start_wp = app.win_prob_history.first().copied().unwrap_or(0.5);
        app.local_stats.game_maps.push(map_name);
        app.local_stats.game_turns.push(game_turn);
        app.local_stats.game_results.push(won);
        app.local_stats.start_win_probs.push(start_wp);
        // Track bonus captures: bonuses fully owned by the player at game end.
        let bonus_data: Vec<(String, bool)> = app.map.bonuses.iter().map(|b| {
            let owns_all = b.territory_ids.iter().all(|&tid| app.game.territory_owners[tid] == PLAYER);
            (b.name.clone(), owns_all)
        }).collect();
        for (name, owns_all) in bonus_data {
            if owns_all {
                if let Some(entry) = app.local_stats.bonus_captures.iter_mut().find(|(n, _)| n == &name) {
                    entry.1 += 1;
                } else {
                    app.local_stats.bonus_captures.push((name, 1));
                }
            }
        }

        if won {
            "Victory!".to_string()
        } else {
            "Defeat.".to_string()
        }
    } else {
        format!("Turn {} complete", app.game.turn - 1)
    };

    Ok(Json(ActionResult {
        success: true,
        message: msg,
        events: filtered_events,
    }))
}

#[derive(Deserialize, Default)]
struct NewGameRequest {
    #[serde(default)]
    template: Option<String>,
}

async fn new_local_game(
    State(state): State<AppState>,
    body: Option<Json<NewGameRequest>>,
) -> Json<ActionResult> {
    let template = body.and_then(|b| b.0.template).unwrap_or_default();

    let mut app = state.local.lock().unwrap();

    // Load a different map if requested.
    if !template.is_empty() {
        let map_path = format!("maps/{}.json", template);
        match Map::load(&PathBuf::from(&map_path)) {
            Ok(new_map) => {
                app.map = new_map;
            }
            Err(e) => {
                return Json(ActionResult {
                    success: false,
                    message: format!("Failed to load template '{}': {}", template, e),
                    events: Vec::new(),
                });
            }
        }
    }

    let map = app.map.clone();
    app.game = GameState::new(&map);
    app.rng = StdRng::from_entropy();
    app.pick_options = picking::generate_pick_options(&map, &mut app.rng);
    app.turn_history.clear();
    app.state_history.clear();
    app.win_prob_history.clear();
    Json(ActionResult {
        success: true,
        message: format!("New game on {}", map.name),
        events: Vec::new(),
    })
}

async fn get_replay_turn(
    State(state): State<AppState>,
    axum::extract::Path(turn): axum::extract::Path<u32>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let app = state.local.lock().unwrap();

    let idx = turn as usize;
    if idx >= app.state_history.len() {
        return Err((
            StatusCode::NOT_FOUND,
            format!(
                "Turn {} not found in replay history (available: 0..{})",
                turn,
                app.state_history.len().saturating_sub(1)
            ),
        ));
    }

    let historical_state = &app.state_history[idx];

    // Replay is fog-free — show the full board state for both players.
    let territories: Vec<TerritoryView> = app
        .map
        .territories
        .iter()
        .enumerate()
        .map(|(i, t)| {
            let (lx, ly) = t
                .visual
                .as_ref()
                .map(|v| (v.label_pos[0], v.label_pos[1]))
                .unwrap_or((0.0, 0.0));
            TerritoryView {
                id: i,
                name: t.name.clone(),
                bonus_id: t.bonus_id,
                adjacent: t.adjacent.clone(),
                owner: historical_state.territory_owners[i],
                armies: historical_state.territory_armies[i],
                visible: true,
                path: t.visual.as_ref().map(|v| v.path.clone()),
                label_x: lx,
                label_y: ly,
            }
        })
        .collect();

    Ok(Json(serde_json::json!({
        "turn": turn,
        "phase": match historical_state.phase {
            Phase::Picking => "picking",
            Phase::Play => "play",
            Phase::Finished => "finished",
        },
        "territories": territories,
        "events": app.turn_history.get(idx).map(|tl| &tl.events),
        "win_probability": app.win_prob_history.get(idx),
    })))
}

async fn index() -> Html<&'static str> {
    Html(include_str!("../../static/index.html"))
}

async fn editor() -> Html<&'static str> {
    Html(include_str!("../../static/editor.html"))
}

async fn favicon() -> impl IntoResponse {
    use axum::http::header;
    // Shield icon with crossed swords — strategy game favicon
    let svg = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 32 32"><defs><linearGradient id="sg" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stop-color="#2563eb"/><stop offset="1" stop-color="#1d4ed8"/></linearGradient></defs><path d="M16 2 L28 8 L28 16 Q28 26 16 30 Q4 26 4 16 L4 8Z" fill="url(#sg)" stroke="#e3b341" stroke-width="1.2"/><path d="M16 6 L24 10 L24 16 Q24 23 16 26 Q8 23 8 16 L8 10Z" fill="#1e40af" stroke="none"/><line x1="11" y1="22" x2="21" y2="10" stroke="#e3b341" stroke-width="1.8" stroke-linecap="round"/><line x1="21" y1="22" x2="11" y2="10" stroke="#e3b341" stroke-width="1.8" stroke-linecap="round"/><circle cx="16" cy="16" r="3" fill="#e3b341"/><text x="16" y="18" text-anchor="middle" font-size="5" fill="#1e3a5f" font-weight="900" font-family="sans-serif">S</text></svg>"##;
    (
        [(header::CONTENT_TYPE, "image/svg+xml"), (header::CACHE_CONTROL, "public, max-age=86400")],
        svg,
    )
}

// ── Auth route handlers ──

async fn auth_discord_redirect(
    State(state): State<AppState>,
) -> Result<Redirect, (StatusCode, String)> {
    let client_id = state
        .config
        .discord_client_id
        .as_ref()
        .ok_or((StatusCode::SERVICE_UNAVAILABLE, "Discord OAuth not configured".to_string()))?;
    let redirect_uri = state
        .config
        .discord_redirect_uri
        .as_ref()
        .ok_or((StatusCode::SERVICE_UNAVAILABLE, "Discord OAuth not configured".to_string()))?;
    let url = auth::discord::build_auth_url(client_id, redirect_uri);
    Ok(Redirect::temporary(&url))
}

#[derive(Deserialize)]
struct CallbackQuery {
    code: String,
}

async fn auth_discord_callback(
    State(state): State<AppState>,
    axum::extract::Query(query): axum::extract::Query<CallbackQuery>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let client_id = state.config.discord_client_id.as_ref()
        .ok_or((StatusCode::SERVICE_UNAVAILABLE, "Discord OAuth not configured".to_string()))?;
    let client_secret = state.config.discord_client_secret.as_ref()
        .ok_or((StatusCode::SERVICE_UNAVAILABLE, "Discord OAuth not configured".to_string()))?;
    let redirect_uri = state.config.discord_redirect_uri.as_ref()
        .ok_or((StatusCode::SERVICE_UNAVAILABLE, "Discord OAuth not configured".to_string()))?;
    let pool = state.require_db()?;

    // Exchange code for access token.
    let access_token =
        auth::discord::exchange_code(client_id, client_secret, redirect_uri, &query.code)
            .await
            .map_err(|e| (StatusCode::BAD_GATEWAY, format!("Discord token exchange failed: {}", e)))?;

    // Fetch Discord user info.
    let discord_user = auth::discord::fetch_user(&access_token)
        .await
        .map_err(|e| (StatusCode::BAD_GATEWAY, format!("Discord user fetch failed: {}", e)))?;

    let discord_id = discord_user
        .discord_id_i64()
        .map_err(|e| (StatusCode::BAD_GATEWAY, format!("invalid Discord ID: {}", e)))?;

    // Upsert user in database.
    let user = db::upsert_user(pool, discord_id, &discord_user.username, discord_user.avatar_url().as_deref())
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    // Create JWT.
    let token = auth::session::create_token(user.id, &user.username, &state.config.jwt_secret)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("JWT creation failed: {}", e)))?;

    // Set cookie and redirect to app.
    let cookie = format!(
        "token={}; Path=/; HttpOnly; SameSite=Lax; Max-Age=604800",
        token
    );
    Ok((
        [(axum::http::header::SET_COOKIE, cookie)],
        Redirect::temporary("/app"),
    ))
}

async fn auth_logout() -> impl IntoResponse {
    let cookie = "token=; Path=/; HttpOnly; SameSite=Lax; Max-Age=0";
    (
        [(axum::http::header::SET_COOKIE, cookie.to_string())],
        Json(serde_json::json!({"success": true})),
    )
}

async fn auth_me(
    auth: auth::AuthUser,
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    if let Some(pool) = &state.db_pool {
        if let Some(user) = db::get_user(pool, auth.user_id)
            .await
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        {
            return Ok(Json(serde_json::json!({
                "id": user.id,
                "username": user.username,
                "avatar_url": user.avatar_url,
                "rating": user.rating,
                "games_played": user.games_played,
                "games_won": user.games_won,
            })));
        }
    }
    Ok(Json(serde_json::json!({
        "id": auth.user_id,
        "username": auth.username,
    })))
}

// ── Analysis & difficulty endpoints ──

async fn get_analysis(State(state): State<AppState>) -> Json<serde_json::Value> {
    let app = state.local.lock().unwrap();
    let wp = analysis::full_win_probability(&app.game, &app.map, 200);
    Json(serde_json::json!({
        "win_probability": {
            "player_0": wp.player_0,
            "player_1": wp.player_1,
            "simulations": wp.simulations_run,
        }
    }))
}

#[derive(Deserialize)]
struct DifficultyRequest {
    level: AiStrength,
}

async fn set_difficulty(
    State(state): State<AppState>,
    Json(body): Json<DifficultyRequest>,
) -> Json<ActionResult> {
    let mut app = state.local.lock().unwrap();
    app.ai_strength = body.level;
    Json(ActionResult {
        success: true,
        message: format!("AI difficulty set to {:?}", body.level),
        events: Vec::new(),
    })
}

async fn games_page() -> Html<&'static str> {
    Html(include_str!("../../static/games.html"))
}

async fn tutorial_page() -> Html<&'static str> {
    Html(include_str!("../../static/tutorial.html"))
}

// ── Profile & stats endpoints ──

async fn profile_page() -> Html<&'static str> {
    Html(include_str!("../../static/profile.html"))
}

async fn get_local_stats(State(state): State<AppState>) -> Json<serde_json::Value> {
    let app = state.local.lock().unwrap();
    let stats = &app.local_stats;
    let current_rating = stats.rating.rating;
    let rd = stats.rating.rd;

    // Determine rank tier from rating.
    let rank_tier = match current_rating as u32 {
        0..=1199 => "Bronze",
        1200..=1399 => "Silver",
        1400..=1599 => "Gold",
        1600..=1799 => "Platinum",
        1800..=1999 => "Diamond",
        2000..=2199 => "Master",
        _ => "Grandmaster",
    };

    let win_rate = if stats.games_played > 0 {
        (stats.wins as f64 / stats.games_played as f64) * 100.0
    } else {
        0.0
    };

    let avg_game_length = if stats.games_played > 0 {
        stats.total_turns as f64 / stats.games_played as f64
    } else {
        0.0
    };

    // Build match history (last 20 games).
    let total = stats.game_results.len();
    let start = total.saturating_sub(20);
    let match_history: Vec<serde_json::Value> = (start..total)
        .rev()
        .map(|i| {
            serde_json::json!({
                "game_number": i + 1,
                "result": if stats.game_results[i] { "win" } else { "loss" },
                "turns": stats.game_turns[i],
                "map": stats.game_maps[i],
                "rating_after": stats.rating_history[i],
            })
        })
        .collect();

    // Sort bonus captures by frequency (descending).
    let mut bonuses = stats.bonus_captures.clone();
    bonuses.sort_by(|a, b| b.1.cmp(&a.1));

    // Sort pick choices by frequency (descending).
    let mut picks = stats.pick_choices.clone();
    picks.sort_by(|a, b| b.1.cmp(&a.1));

    Json(serde_json::json!({
        "games_played": stats.games_played,
        "wins": stats.wins,
        "losses": stats.losses,
        "win_rate": win_rate,
        "total_turns": stats.total_turns,
        "avg_game_length": avg_game_length,
        "streak": stats.streak,
        "rating": current_rating,
        "rd": rd,
        "rank_tier": rank_tier,
        "rating_history": stats.rating_history,
        "start_win_probs": stats.start_win_probs,
        "match_history": match_history,
        "bonus_captures": bonuses,
        "pick_choices": picks,
    }))
}

async fn landing() -> Html<&'static str> {
    Html(include_str!("../../static/landing.html"))
}

async fn app_placeholder() -> Html<&'static str> {
    Html("<html><body><h1>strat-club multiplayer</h1><p>Frontend coming soon.</p></body></html>")
}

// ── Entrypoint ──

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();

    // Load .env file if present.
    let _ = dotenvy::dotenv();

    let config = Config::from_env();
    info!("starting strat-club server");

    // Load map for local play.
    let map_path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| config.default_map_path.clone());
    let map = Map::load(&PathBuf::from(&map_path)).unwrap_or_else(|e| {
        eprintln!("Failed to load map '{}': {}", map_path, e);
        std::process::exit(1);
    });

    info!(
        map = %map.name,
        territories = map.territory_count(),
        bonuses = map.bonuses.len(),
        "loaded map for local play"
    );

    let mut rng = StdRng::from_entropy();
    let game = GameState::new(&map);
    let pick_options = picking::generate_pick_options(&map, &mut rng);

    let local_state = LocalState {
        map,
        game,
        rng,
        pick_options,
        turn_history: Vec::new(),
        ai_strength: AiStrength::Hard,
        state_history: Vec::new(),
        win_prob_history: Vec::new(),
        local_stats: LocalStats::default(),
    };

    // Set up WebSocket hub.
    let ws_hub = Arc::new(ws::Hub::new());

    // Connect to database if configured.
    let db_pool = if let Some(url) = &config.database_url {
        match sqlx::PgPool::connect(url).await {
            Ok(pool) => {
                info!("connected to database");
                if let Err(e) = db::migrate::run_migrations(&pool).await {
                    eprintln!("migration error: {}", e);
                    eprintln!("continuing without database -- multiplayer will be unavailable");
                    None
                } else {
                    Some(pool)
                }
            }
            Err(e) => {
                eprintln!("database connection failed: {}", e);
                eprintln!("continuing without database -- multiplayer will be unavailable");
                None
            }
        }
    } else {
        info!("no DATABASE_URL set -- running in local play mode only");
        None
    };

    // Set up game manager if DB is available.
    let game_manager = db_pool.as_ref().map(|pool| {
        Arc::new(game::manager::GameManager::new(pool.clone(), ws_hub.clone()))
    });

    // Start boot timer background task if DB is available.
    if let (Some(pool), Some(manager)) = (&db_pool, &game_manager) {
        let pool = pool.clone();
        let manager = manager.clone();
        tokio::spawn(game::timer::boot_timer_task(pool, manager));
    }

    // Set up matchmaking queue.
    let matchmaking = Arc::new(game::matchmaking::MatchmakingQueue::new());

    // Spawn matchmaking background task.
    {
        let mm_queue = matchmaking.clone();
        let mm_manager = game_manager.clone();
        let mm_hub = ws_hub.clone();
        let mm_pool = db_pool.clone();
        tokio::spawn(game::matchmaking::matchmaking_task(
            mm_queue, mm_manager, mm_hub, mm_pool,
        ));
    }

    let bind_addr = config.bind_addr.clone();

    let app_state = AppState {
        config: Arc::new(config),
        db_pool,
        ws_hub,
        game_manager,
        matchmaking,
        local: Arc::new(Mutex::new(local_state)),
    };

    let app = Router::new()
        // Local play routes (original functionality).
        .route("/", get(index))
        .route("/favicon.ico", get(favicon))
        .route("/favicon.svg", get(favicon))
        .route("/api/game", get(get_local_game))
        .route("/api/picks", post(submit_local_picks))
        .route("/api/orders", post(submit_local_orders))
        .route("/api/new", post(new_local_game))
        .route("/api/game/replay/{turn}", get(get_replay_turn))
        .route("/api/game/analysis", get(get_analysis))
        .route("/api/difficulty", post(set_difficulty))
        .route("/api/stats", get(get_local_stats))
        // Landing page.
        .route("/landing", get(landing))
        // Profile page.
        .route("/profile", get(profile_page))
        // Map editor.
        .route("/editor", get(editor))
        // Tutorial.
        .route("/tutorial", get(tutorial_page))
        // Game browser & spectator.
        .route("/games", get(games_page))
        .route("/api/games/active", get(api::spectate::active_games))
        .route("/api/games/recent", get(api::spectate::recent_games))
        .route("/api/games/{id}/spectate", get(api::spectate::spectate_game))
        // Multiplayer app placeholder.
        .route("/app", get(app_placeholder))
        // Auth routes.
        .route("/api/auth/discord", get(auth_discord_redirect))
        .route("/api/auth/discord/callback", get(auth_discord_callback))
        .route("/api/auth/logout", post(auth_logout))
        .route("/api/auth/me", get(auth_me))
        // Game API routes.
        .route("/api/games", post(api::games::create_game))
        .route("/api/games", get(api::games::list_games))
        .route("/api/games/{id}", get(api::games::get_game))
        .route("/api/games/{id}/join", post(api::games::join_game))
        .route("/api/games/{id}/picks", post(api::orders::submit_picks))
        .route("/api/games/{id}/orders", post(api::orders::submit_orders))
        // Ladder and lobby.
        .route("/api/ladder", get(api::ladder::leaderboard))
        .route("/api/users/{id}", get(api::ladder::player_profile))
        .route("/api/lobby", get(api::lobby::open_games))
        // League / seasons.
        .route("/api/seasons", get(api::league::list_seasons))
        .route("/api/seasons/current", get(api::league::current_season))
        .route("/api/seasons/{id}/standings", get(api::league::season_standings))
        .route("/api/seasons/{season_id}/standings/{user_id}", get(api::league::player_season_stats))
        .route("/api/match-history", get(api::league::match_history))
        // Matchmaking queue.
        .route("/api/queue/join", post(api::queue::join_queue))
        .route("/api/queue/leave", post(api::queue::leave_queue))
        .route("/api/queue/status", get(api::queue::queue_status))
        // Map management.
        .route("/api/maps", get(api::maps::list_maps))
        .route("/api/maps", post(api::maps::save_map))
        .route("/api/maps/{id}", axum::routing::delete(api::maps::delete_map))
        // WebSocket.
        .route("/ws", get(ws::handler::ws_upgrade))
        .layer(CorsLayer::permissive())
        .with_state(app_state);

    info!(addr = %bind_addr, "server listening");
    println!("Playing at http://localhost:3000");
    let listener = tokio::net::TcpListener::bind(&bind_addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}
