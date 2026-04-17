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
use rand::SeedableRng;
use rand::rngs::StdRng;
use serde::{Deserialize, Serialize};
use tower_http::cors::CorsLayer;
use tracing::info;

use strat_engine::ai::{self, AiStrength};
use strat_engine::analysis;
use strat_engine::board::Board;
use strat_engine::fog;
use strat_engine::game_analysis;
use strat_engine::map::{Map, MapFile};
use strat_engine::openings;
use strat_engine::orders::Order;
use strat_engine::picking;
use strat_engine::state::{GameState, NEUTRAL, Phase, PlayerId};
use strat_engine::turn::{TurnEvent, resolve_turn};

use crate::config::Config;
use crate::game::achievements::{self, EarnedAchievement};

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
    board: Board,
    game: GameState,
    rng: StdRng,
    pick_options: Vec<usize>,
    turn_history: Vec<TurnLog>,
    ai_strength: AiStrength,
    /// Starting armies per picked territory (customizable via game settings).
    starting_armies: u32,
    /// Complete game state snapshots before each turn's orders (for replay).
    state_history: Vec<GameState>,
    /// Player 0 win probability recorded after each turn resolves.
    win_prob_history: Vec<f64>,
    /// Persistent local play statistics across games.
    local_stats: LocalStats,
    /// Earned achievements.
    achievements: Vec<EarnedAchievement>,
    /// Territories the player owned after picking phase (for flawless victory check).
    starting_territories: Vec<usize>,
    /// Maximum number of complete bonuses owned simultaneously during the current game.
    max_simultaneous_bonuses: u32,
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

#[derive(Clone, Serialize, Deserialize)]
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
    base_income: u32,
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
    enemy_count: usize,
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
    #[serde(skip_serializing_if = "Vec::is_empty")]
    new_achievements: Vec<achievements::AchievementView>,
}

fn build_game_view(app: &LocalState) -> GameView {
    let visible = fog::visible_territories(&app.game, PLAYER, &app.board);
    let is_picking = app.game.phase == Phase::Picking;

    let territories: Vec<TerritoryView> = app
        .board
        .map
        .territories
        .iter()
        .enumerate()
        .map(|(i, t)| {
            let is_visible = is_picking || visible.contains(&i) || !app.board.config.settings.fog_of_war;
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
        .board
        .map
        .bonuses
        .iter()
        .map(|b| {
            let player_count = b
                .territory_ids
                .iter()
                .filter(|&&tid| app.game.territory_owners[tid] == PLAYER)
                .count();
            let enemy_count = b
                .territory_ids
                .iter()
                .filter(|&&tid| {
                    let owner = app.game.territory_owners[tid];
                    owner != PLAYER && owner != NEUTRAL
                })
                .count();
            BonusView {
                id: b.id,
                name: b.name.clone(),
                value: b.value,
                territory_ids: b.territory_ids.clone(),
                player_count,
                enemy_count,
                total: b.territory_ids.len(),
            }
        })
        .collect();

    // Compute win probability using fast material evaluation (< 1ms).
    let wp = analysis::quick_win_probability(&app.game, &app.board);

    GameView {
        phase: match app.game.phase {
            Phase::Picking => "picking".into(),
            Phase::Play => "play".into(),
            Phase::Finished => "finished".into(),
        },
        turn: app.game.turn,
        income: app.game.income(PLAYER, &app.board),
        base_income: app.board.config.settings.base_income,
        my_territories: app.game.territory_count_for(PLAYER) as u32,
        enemy_territories: app.game.territory_count_for(AI_PLAYER) as u32,
        winner: app.game.winner,
        pick_options: app.pick_options.clone(),
        picks_needed: app.board.config.picking.num_picks,
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

    let needed = app.board.config.picking.num_picks;
    if body.picks.len() < needed {
        return Err((
            StatusCode::BAD_REQUEST,
            format!("Need at least {} picks", needed),
        ));
    }

    // Track pick choices in local stats.
    for &tid in &body.picks {
        if let Some(t) = app.board.map.territories.get(tid) {
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

    let ai_picks = ai::generate_picks(&app.game, &app.board, &app.pick_options);
    let board = app.board.clone();

    let starting_armies = app.starting_armies;
    picking::resolve_picks(
        &mut app.game,
        [&body.picks, &ai_picks],
        &board,
        starting_armies,
    );

    // Record starting territories for flawless victory tracking.
    app.starting_territories = (0..app.game.territory_owners.len())
        .filter(|&i| app.game.territory_owners[i] == PLAYER)
        .collect();

    Ok(Json(ActionResult {
        success: true,
        message: "Picks resolved. Game begins!".into(),
        events: Vec::new(),
        new_achievements: Vec::new(),
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

    let visible_before = fog::visible_territories(&app.game, PLAYER, &app.board);

    // Snapshot the current state before resolving orders (for replay).
    let state_snapshot = app.game.clone();
    app.state_history.push(state_snapshot);

    let ai_orders =
        ai::generate_orders_for_strength(&app.game, AI_PLAYER, &app.board, app.ai_strength);
    let game_clone = app.game.clone();

    let board = app.board.clone();
    let result = resolve_turn(&game_clone, [body.orders, ai_orders], &board, &mut app.rng);

    let visible_after = fog::visible_territories(&result.state, PLAYER, &app.board);
    let filtered_events = fog_filter_events(&result.events, &visible_before, &visible_after);

    let turn_num = app.game.turn;
    app.turn_history.push(TurnLog {
        turn: turn_num,
        events: filtered_events.clone(),
    });

    app.game = result.state;

    // Track max simultaneous bonuses owned by the player this turn.
    let bonuses_owned_now = app
        .board
        .map
        .bonuses
        .iter()
        .filter(|b| {
            b.territory_ids
                .iter()
                .all(|&tid| app.game.territory_owners[tid] == PLAYER)
        })
        .count() as u32;
    if bonuses_owned_now > app.max_simultaneous_bonuses {
        app.max_simultaneous_bonuses = bonuses_owned_now;
    }

    // Record win probability after the turn resolves (1-ply lookahead for history).
    let wp = analysis::win_probability_with_lookahead(&app.game, &app.board);
    app.win_prob_history.push(wp.player_0);

    let mut newly_earned_achievements: Vec<achievements::EarnedAchievement> = Vec::new();

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
        let map_name = app.board.map.name.clone();
        let game_turn = app.game.turn;
        let start_wp = app.win_prob_history.first().copied().unwrap_or(0.5);
        app.local_stats.game_maps.push(map_name);
        app.local_stats.game_turns.push(game_turn);
        app.local_stats.game_results.push(won);
        app.local_stats.start_win_probs.push(start_wp);
        // Track bonus captures: bonuses fully owned by the player at game end.
        let bonus_data: Vec<(String, bool)> = app
            .board
            .map
            .bonuses
            .iter()
            .map(|b| {
                let owns_all = b
                    .territory_ids
                    .iter()
                    .all(|&tid| app.game.territory_owners[tid] == PLAYER);
                (b.name.clone(), owns_all)
            })
            .collect();
        for (name, owns_all) in bonus_data {
            if owns_all {
                if let Some(entry) = app
                    .local_stats
                    .bonus_captures
                    .iter_mut()
                    .find(|(n, _)| n == &name)
                {
                    entry.1 += 1;
                } else {
                    app.local_stats.bonus_captures.push((name, 1));
                }
            }
        }

        // Check achievements.
        let kept_all = app
            .starting_territories
            .iter()
            .all(|&tid| app.game.territory_owners[tid] == PLAYER);
        // Count distinct maps played.
        let mut distinct_maps: Vec<String> = app.local_stats.game_maps.clone();
        distinct_maps.sort();
        distinct_maps.dedup();
        // Count available built-in maps.
        let total_maps = std::fs::read_dir("maps")
            .map(|entries| {
                entries
                    .flatten()
                    .filter(|e| {
                        let p = e.path();
                        p.is_file() && p.extension().is_some_and(|ext| ext == "json")
                    })
                    .count() as u32
            })
            .unwrap_or(0);

        let ctx = achievements::GameContext {
            won,
            total_wins: app.local_stats.wins,
            turn_count: game_turn,
            streak: app.local_stats.streak,
            start_win_prob: start_wp,
            rating: new_rating,
            kept_all_starting_territories: kept_all,
            max_simultaneous_bonuses: app.max_simultaneous_bonuses,
            maps_played: distinct_maps,
            total_maps_available: total_maps,
        };

        newly_earned_achievements =
            achievements::check_achievements(&app.achievements, &ctx, app.local_stats.games_played);
        app.achievements.extend(newly_earned_achievements.clone());

        if won {
            "Victory!".to_string()
        } else {
            "Defeat.".to_string()
        }
    } else {
        format!("Turn {} complete", app.game.turn - 1)
    };

    let achievement_views = achievements::build_newly_earned_views(&newly_earned_achievements);

    Ok(Json(ActionResult {
        success: true,
        message: msg,
        events: filtered_events,
        new_achievements: achievement_views,
    }))
}

#[derive(Deserialize, Default)]
struct GameSettings {
    #[serde(default)]
    fog_of_war: Option<bool>,
    #[serde(default)]
    starting_armies: Option<u32>,
    #[serde(default)]
    base_income: Option<u32>,
    #[serde(default)]
    num_picks: Option<usize>,
    #[serde(default)]
    ai_difficulty: Option<String>,
}

#[derive(Deserialize, Default)]
struct NewGameRequest {
    #[serde(default)]
    template: Option<String>,
    #[serde(default)]
    settings: Option<GameSettings>,
}

async fn new_local_game(
    State(state): State<AppState>,
    body: Option<Json<NewGameRequest>>,
) -> Json<ActionResult> {
    let (template, settings) = body
        .map(|b| (b.0.template.unwrap_or_default(), b.0.settings))
        .unwrap_or_default();

    let mut app = state.local.lock().unwrap();

    // Load a different map if requested.
    if !template.is_empty() {
        let map_path = format!("maps/{}.json", template);
        match MapFile::load(&PathBuf::from(&map_path)) {
            Ok(new_map) => {
                app.board = Board::from_map(new_map);
            }
            Err(e) => {
                return Json(ActionResult {
                    success: false,
                    message: format!("Failed to load template '{}': {}", template, e),
                    events: Vec::new(),
                    new_achievements: Vec::new(),
                });
            }
        }
    }

    // Apply custom game settings if provided.
    if let Some(ref s) = settings {
        if let Some(fog) = s.fog_of_war {
            app.board.config.settings.fog_of_war = fog;
        }
        if let Some(income) = s.base_income {
            app.board.config.settings.base_income = income;
        }
        if let Some(num_picks) = s.num_picks {
            app.board.config.picking.num_picks = num_picks;
        }
        if let Some(ref difficulty) = s.ai_difficulty {
            app.ai_strength = match difficulty.as_str() {
                "easy" => AiStrength::Easy,
                "medium" => AiStrength::Medium,
                _ => AiStrength::Hard,
            };
        }
    }

    // Set starting armies (default 5).
    app.starting_armies = settings
        .as_ref()
        .and_then(|s| s.starting_armies)
        .unwrap_or(picking::DEFAULT_STARTING_ARMIES);

    app.game = GameState::new(&app.board);
    app.rng = StdRng::from_entropy();
    let board_ref = app.board.clone();
    app.pick_options = picking::generate_pick_options(&board_ref, &mut app.rng);
    app.turn_history.clear();
    app.state_history.clear();
    app.win_prob_history.clear();
    app.starting_territories.clear();
    app.max_simultaneous_bonuses = 0;
    let board_name = app.board.name.clone();
    Json(ActionResult {
        success: true,
        message: format!("New game on {}", board_name),
        events: Vec::new(),
        new_achievements: Vec::new(),
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
        .board
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

async fn favicon() -> impl IntoResponse {
    use axum::http::header;
    // Shield icon with crossed swords — strategy game favicon
    let svg = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 32 32"><defs><linearGradient id="sg" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stop-color="#2563eb"/><stop offset="1" stop-color="#1d4ed8"/></linearGradient></defs><path d="M16 2 L28 8 L28 16 Q28 26 16 30 Q4 26 4 16 L4 8Z" fill="url(#sg)" stroke="#e3b341" stroke-width="1.2"/><path d="M16 6 L24 10 L24 16 Q24 23 16 26 Q8 23 8 16 L8 10Z" fill="#1e40af" stroke="none"/><line x1="11" y1="22" x2="21" y2="10" stroke="#e3b341" stroke-width="1.8" stroke-linecap="round"/><line x1="21" y1="22" x2="11" y2="10" stroke="#e3b341" stroke-width="1.8" stroke-linecap="round"/><circle cx="16" cy="16" r="3" fill="#e3b341"/><text x="16" y="18" text-anchor="middle" font-size="5" fill="#1e3a5f" font-weight="900" font-family="sans-serif">S</text></svg>"##;
    (
        [
            (header::CONTENT_TYPE, "image/svg+xml"),
            (header::CACHE_CONTROL, "public, max-age=86400"),
        ],
        svg,
    )
}

// ── Auth route handlers ──

async fn auth_discord_redirect(
    State(state): State<AppState>,
) -> Result<Redirect, (StatusCode, String)> {
    let client_id = state.config.discord_client_id.as_ref().ok_or((
        StatusCode::SERVICE_UNAVAILABLE,
        "Discord OAuth not configured".to_string(),
    ))?;
    let redirect_uri = state.config.discord_redirect_uri.as_ref().ok_or((
        StatusCode::SERVICE_UNAVAILABLE,
        "Discord OAuth not configured".to_string(),
    ))?;
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
    let client_id = state.config.discord_client_id.as_ref().ok_or((
        StatusCode::SERVICE_UNAVAILABLE,
        "Discord OAuth not configured".to_string(),
    ))?;
    let client_secret = state.config.discord_client_secret.as_ref().ok_or((
        StatusCode::SERVICE_UNAVAILABLE,
        "Discord OAuth not configured".to_string(),
    ))?;
    let redirect_uri = state.config.discord_redirect_uri.as_ref().ok_or((
        StatusCode::SERVICE_UNAVAILABLE,
        "Discord OAuth not configured".to_string(),
    ))?;
    let pool = state.require_db()?;

    // Exchange code for access token.
    let access_token =
        auth::discord::exchange_code(client_id, client_secret, redirect_uri, &query.code)
            .await
            .map_err(|e| {
                (
                    StatusCode::BAD_GATEWAY,
                    format!("Discord token exchange failed: {}", e),
                )
            })?;

    // Fetch Discord user info.
    let discord_user = auth::discord::fetch_user(&access_token)
        .await
        .map_err(|e| {
            (
                StatusCode::BAD_GATEWAY,
                format!("Discord user fetch failed: {}", e),
            )
        })?;

    let discord_id = discord_user.discord_id_i64().map_err(|e| {
        (
            StatusCode::BAD_GATEWAY,
            format!("invalid Discord ID: {}", e),
        )
    })?;

    // Upsert user in database.
    let user = db::upsert_user(
        pool,
        discord_id,
        &discord_user.username,
        discord_user.avatar_url().as_deref(),
    )
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    // Create JWT.
    let token = auth::session::create_token(user.id, &user.username, &state.config.jwt_secret)
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("JWT creation failed: {}", e),
            )
        })?;

    // Set cookie and redirect to app.
    let cookie = format!(
        "token={}; Path=/; HttpOnly; SameSite=Lax; Max-Age=604800",
        token
    );
    Ok((
        [(axum::http::header::SET_COOKIE, cookie)],
        Redirect::temporary("/"),
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
    if let Some(pool) = &state.db_pool
        && let Some(user) = db::get_user(pool, auth.user_id)
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
    Ok(Json(serde_json::json!({
        "id": auth.user_id,
        "username": auth.username,
    })))
}

// ── Analysis & difficulty endpoints ──

async fn get_analysis(State(state): State<AppState>) -> Json<serde_json::Value> {
    let app = state.local.lock().unwrap();
    let wp = analysis::full_win_probability(&app.game, &app.board, 200);
    Json(serde_json::json!({
        "win_probability": {
            "player_0": wp.player_0,
            "player_1": wp.player_1,
            "simulations": wp.simulations_run,
        }
    }))
}

/// Export the complete game data as a JSON download (for sharing/replay).
async fn export_game(State(state): State<AppState>) -> impl IntoResponse {
    let app = state.local.lock().unwrap();

    let map_data = serde_json::to_value(&app.board.map).unwrap_or_default();

    // Build territory snapshot for current (final) state.
    let territories_final: Vec<serde_json::Value> = app
        .board
        .map
        .territories
        .iter()
        .enumerate()
        .map(|(i, t)| {
            serde_json::json!({
                "id": i,
                "name": t.name,
                "owner": app.game.territory_owners[i],
                "armies": app.game.territory_armies[i],
            })
        })
        .collect();

    // Build state history snapshots.
    let state_snapshots: Vec<serde_json::Value> = app
        .state_history
        .iter()
        .enumerate()
        .map(|(idx, gs)| {
            let terrs: Vec<serde_json::Value> = (0..gs.territory_owners.len())
                .map(|i| {
                    serde_json::json!({
                        "id": i,
                        "owner": gs.territory_owners[i],
                        "armies": gs.territory_armies[i],
                    })
                })
                .collect();
            serde_json::json!({
                "turn": idx,
                "phase": match gs.phase {
                    Phase::Picking => "picking",
                    Phase::Play => "play",
                    Phase::Finished => "finished",
                },
                "territories": terrs,
            })
        })
        .collect();

    let export = serde_json::json!({
        "version": 1,
        "exported_at": chrono::Utc::now().to_rfc3339(),
        "app": "strat.club",
        "map": map_data,
        "game": {
            "turn": app.game.turn,
            "phase": match app.game.phase {
                Phase::Picking => "picking",
                Phase::Play => "play",
                Phase::Finished => "finished",
            },
            "winner": app.game.winner,
            "territories_final": territories_final,
        },
        "turn_history": app.turn_history,
        "state_history": state_snapshots,
        "win_prob_history": app.win_prob_history,
        "ai_strength": format!("{:?}", app.ai_strength),
    });

    let json_str = serde_json::to_string_pretty(&export).unwrap_or_default();
    let filename = format!(
        "strat-club-{}-turn{}.json",
        app.board.map.name.to_lowercase().replace(' ', "-"),
        app.game.turn
    );

    (
        [
            (
                axum::http::header::CONTENT_TYPE,
                "application/json".to_string(),
            ),
            (
                axum::http::header::CONTENT_DISPOSITION,
                format!("attachment; filename=\"{}\"", filename),
            ),
        ],
        json_str,
    )
}

/// Import a previously exported game for replay.
async fn import_game(
    State(state): State<AppState>,
    Json(body): Json<serde_json::Value>,
) -> Result<Json<ActionResult>, (StatusCode, String)> {
    // Validate the import data.
    let version = body.get("version").and_then(|v| v.as_u64()).unwrap_or(0);
    if version != 1 {
        return Err((
            StatusCode::BAD_REQUEST,
            "Unsupported export version".to_string(),
        ));
    }

    // Parse the map and wrap in a Board.
    let map_val = body
        .get("map")
        .ok_or((StatusCode::BAD_REQUEST, "Missing map data".to_string()))?;
    // Try parsing as MapFile first (full format with settings), fall back to Map (geography only).
    let board = if let Ok(map_file) = serde_json::from_value::<MapFile>(map_val.clone()) {
        Board::from_map(map_file)
    } else {
        let map: Map = serde_json::from_value(map_val.clone())
            .map_err(|e| (StatusCode::BAD_REQUEST, format!("Invalid map data: {}", e)))?;
        Board {
            id: map.id.clone(),
            name: map.name.clone(),
            config: strat_engine::board::BoardConfig {
                picking: strat_engine::map::PickingConfig {
                    num_picks: map.bonuses.len(),
                    method: strat_engine::map::PickingMethod::RandomWarlords,
                },
                settings: strat_engine::map::MapSettings {
                    luck_pct: 0,
                    base_income: 5,
                    wasteland_armies: 6,
                    unpicked_neutral_armies: 2,
                    fog_of_war: true,
                    offense_kill_rate: 0.6,
                    defense_kill_rate: 0.7,
                },
            },
            map,
        }
    };

    // Parse turn history.
    let turn_history: Vec<TurnLog> = body
        .get("turn_history")
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_default();

    // Parse win prob history.
    let win_prob_history: Vec<f64> = body
        .get("win_prob_history")
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_default();

    // Parse state history snapshots back into GameState objects.
    let mut state_history: Vec<GameState> = Vec::new();
    if let Some(snapshots) = body.get("state_history").and_then(|v| v.as_array()) {
        for snap in snapshots {
            let terrs = snap
                .get("territories")
                .and_then(|v| v.as_array())
                .ok_or((StatusCode::BAD_REQUEST, "Invalid state history".to_string()))?;
            let mut owners: Vec<u8> = vec![NEUTRAL; board.map.territory_count()];
            let mut armies: Vec<u32> = vec![0; board.map.territory_count()];
            for t in terrs {
                let id = t.get("id").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
                if id < owners.len() {
                    owners[id] = t.get("owner").and_then(|v| v.as_u64()).unwrap_or(255) as u8;
                    armies[id] = t.get("armies").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
                }
            }
            let phase_str = snap.get("phase").and_then(|v| v.as_str()).unwrap_or("play");
            let phase = match phase_str {
                "picking" => Phase::Picking,
                "finished" => Phase::Finished,
                _ => Phase::Play,
            };
            state_history.push(GameState {
                territory_owners: owners,
                territory_armies: armies,
                turn: snap.get("turn").and_then(|v| v.as_u64()).unwrap_or(0) as u32,
                phase,
                hands: [Vec::new(), Vec::new()],
                card_pieces: [0; 2],
                alive: [true; 2],
                winner: None,
            });
        }
    }

    // Build the final game state from export.
    let game_data = body
        .get("game")
        .ok_or((StatusCode::BAD_REQUEST, "Missing game data".to_string()))?;
    let final_turn = game_data.get("turn").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
    let winner = game_data
        .get("winner")
        .and_then(|v| v.as_u64())
        .map(|w| w as u8);
    let final_phase = game_data
        .get("phase")
        .and_then(|v| v.as_str())
        .unwrap_or("finished");

    let mut final_state = GameState::new(&board);
    final_state.turn = final_turn;
    final_state.phase = match final_phase {
        "picking" => Phase::Picking,
        "play" => Phase::Play,
        _ => Phase::Finished,
    };
    final_state.winner = winner;

    // Apply final territory data if available.
    if let Some(terrs) = game_data
        .get("territories_final")
        .and_then(|v| v.as_array())
    {
        for t in terrs {
            let id = t.get("id").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
            if id < final_state.territory_owners.len() {
                final_state.territory_owners[id] =
                    t.get("owner").and_then(|v| v.as_u64()).unwrap_or(255) as u8;
                final_state.territory_armies[id] =
                    t.get("armies").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
            }
        }
    }

    // Apply to local state.
    let mut app = state.local.lock().unwrap();
    let map_name = board.map.name.clone();
    app.board = board;
    app.game = final_state;
    app.rng = StdRng::from_entropy();
    app.pick_options = Vec::new();
    app.turn_history = turn_history;
    app.state_history = state_history;
    app.win_prob_history = win_prob_history;
    app.starting_territories.clear();
    app.max_simultaneous_bonuses = 0;

    Ok(Json(ActionResult {
        success: true,
        message: format!("Imported game on {}", map_name),
        events: Vec::new(),
        new_achievements: Vec::new(),
    }))
}

async fn get_post_analysis(State(state): State<AppState>) -> Json<serde_json::Value> {
    let app = state.local.lock().unwrap();

    // Collect turn events from the turn history.
    let turn_events: Vec<Vec<TurnEvent>> = app
        .turn_history
        .iter()
        .map(|tl| tl.events.clone())
        .collect();

    let analysis = game_analysis::analyze_game(
        &app.state_history,
        &app.win_prob_history,
        &turn_events,
        &app.board,
    );

    Json(serde_json::to_value(&analysis).unwrap_or_default())
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
        new_achievements: Vec::new(),
    })
}

async fn games_page() -> Html<&'static str> {
    Html(include_str!("../../static/games.html"))
}

// ── Stats endpoints ──

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

async fn get_achievements(State(state): State<AppState>) -> Json<serde_json::Value> {
    let app = state.local.lock().unwrap();
    let views = achievements::build_achievement_views(&app.achievements);
    let earned_count = views.iter().filter(|v| v.earned).count();
    Json(serde_json::json!({
        "achievements": views,
        "earned": earned_count,
        "total": views.len(),
    }))
}

async fn landing() -> Html<&'static str> {
    Html(include_str!("../../static/landing.html"))
}

async fn feedback_page() -> Html<&'static str> {
    Html(include_str!("../../static/feedback.html"))
}

async fn ladder_page() -> Html<&'static str> {
    Html(include_str!("../../static/ladder.html"))
}

async fn app_placeholder() -> Html<&'static str> {
    Html("<html><body><h1>strat-club multiplayer</h1><p>Frontend coming soon.</p></body></html>")
}

async fn multiplayer_game_page() -> Html<&'static str> {
    Html(include_str!("../../static/game.html"))
}

// ── Opening book ──

#[derive(Deserialize)]
struct OpeningsQuery {
    map: Option<String>,
}

async fn get_openings(
    axum::extract::Query(query): axum::extract::Query<OpeningsQuery>,
) -> Json<serde_json::Value> {
    let map_id = query.map.as_deref().unwrap_or("small_earth");
    let book = openings::get_openings(map_id);
    Json(serde_json::json!({ "map": map_id, "openings": book }))
}

// ── Entrypoint ──

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();

    // Load .env file if present.
    let _ = dotenvy::dotenv();

    let config = Config::from_env();
    info!("starting strat-club server");

    // Load board for local play.
    // Try new split format (board JSON + maps dir) first, fall back to legacy MapFile.
    let map_path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| config.default_map_path.clone());
    let board = if let Ok(b) = Board::load(&PathBuf::from(&map_path), &PathBuf::from("maps")) {
        b
    } else {
        let map = MapFile::load(&PathBuf::from(&map_path)).unwrap_or_else(|e| {
            eprintln!("Failed to load map '{}': {}", map_path, e);
            std::process::exit(1);
        });
        Board::from_map(map)
    };

    info!(
        map = %board.name,
        territories = board.map.territory_count(),
        bonuses = board.map.bonuses.len(),
        "loaded map for local play"
    );

    let mut rng = StdRng::from_entropy();
    let game = GameState::new(&board);
    let pick_options = picking::generate_pick_options(&board, &mut rng);

    let local_state = LocalState {
        board,
        game,
        rng,
        pick_options,
        turn_history: Vec::new(),
        ai_strength: AiStrength::Hard,
        starting_armies: picking::DEFAULT_STARTING_ARMIES,
        state_history: Vec::new(),
        win_prob_history: Vec::new(),
        local_stats: LocalStats::default(),
        achievements: Vec::new(),
        starting_territories: Vec::new(),
        max_simultaneous_bonuses: 0,
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
        Arc::new(game::manager::GameManager::new(
            pool.clone(),
            ws_hub.clone(),
        ))
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
        .route("/api/game/post-analysis", get(get_post_analysis))
        .route("/api/game/export", get(export_game))
        .route("/api/game/import", post(import_game))
        .route("/api/difficulty", post(set_difficulty))
        .route("/api/stats", get(get_local_stats))
        .route("/api/achievements", get(get_achievements))
        .route("/api/openings", get(get_openings))
        // Landing page.
        .route("/landing", get(landing))
        // Game browser & spectator.
        .route("/games", get(games_page))
        .route("/api/games/active", get(api::spectate::active_games))
        .route("/api/games/recent", get(api::spectate::recent_games))
        .route(
            "/api/games/{id}/spectate",
            get(api::spectate::spectate_game),
        )
        // Multiplayer app placeholder.
        .route("/app", get(app_placeholder))
        // Multiplayer game board.
        .route("/game/{id}", get(multiplayer_game_page))
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
        .route(
            "/api/seasons/{id}/standings",
            get(api::league::season_standings),
        )
        .route(
            "/api/seasons/{season_id}/standings/{user_id}",
            get(api::league::player_season_stats),
        )
        .route("/api/match-history", get(api::league::match_history))
        // Matchmaking queue.
        .route("/api/queue/join", post(api::queue::join_queue))
        .route("/api/queue/leave", post(api::queue::leave_queue))
        .route("/api/queue/status", get(api::queue::queue_status))
        // Arena tournaments.
        .route("/api/arenas", post(api::tournament::create_arena))
        .route("/api/arenas", get(api::tournament::list_arenas))
        .route("/api/arenas/{id}", get(api::tournament::get_arena))
        .route("/api/arenas/{id}/join", post(api::tournament::join_arena))
        .route(
            "/api/arenas/{id}/leaderboard",
            get(api::tournament::arena_leaderboard),
        )
        // Ladder.
        .route("/ladder", get(ladder_page))
        // Feedback.
        .route("/feedback", get(feedback_page))
        .route("/api/feedback", post(api::feedback::submit_feedback))
        .route("/api/feedback", get(api::feedback::list_feedback))
        .route(
            "/api/feedback/{id}/vote",
            post(api::feedback::vote_feedback),
        )
        .route(
            "/api/feedback/{id}",
            axum::routing::delete(api::feedback::delete_feedback),
        )
        // Map management.
        .route("/api/maps", get(api::maps::list_maps))
        .route("/api/maps", post(api::maps::save_map))
        .route(
            "/api/maps/{id}",
            axum::routing::delete(api::maps::delete_map),
        )
        // WebSocket.
        .route("/ws", get(ws::handler::ws_upgrade))
        .layer(CorsLayer::permissive())
        .with_state(app_state);

    info!(addr = %bind_addr, "server listening");
    println!("Playing at http://localhost:3000");
    let listener = tokio::net::TcpListener::bind(&bind_addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}
