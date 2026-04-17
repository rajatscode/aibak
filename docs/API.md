# strat.club API Reference

All endpoints are served from the same origin as the game (default `http://localhost:3000`).

Responses are JSON unless otherwise noted. Error responses use standard HTTP status codes with a plain-text or JSON body.

---

## Authentication

Multiplayer endpoints require a JWT token, set as an `HttpOnly` cookie named `token` after Discord OAuth login. Endpoints marked **Auth: required** will return `401 Unauthorized` without a valid session.

Local play endpoints (under `/api/game`, `/api/new`, etc.) do not require authentication.

---

## Table of Contents

- [Local Play](#local-play)
- [Authentication](#authentication-endpoints)
- [Games (Multiplayer)](#games-multiplayer)
- [Maps](#maps)
- [Matchmaking Queue](#matchmaking-queue)
- [League and Seasons](#league-and-seasons)
- [Ladder](#ladder)
- [Spectate](#spectate)
- [Arena Tournaments](#arena-tournaments)
- [Puzzles](#puzzles)
- [Stats and Achievements](#stats-and-achievements)
- [WebSocket](#websocket)

---

## Local Play

Local play runs entirely in-memory on the server. No database required.

### GET /api/game

Get the current local game state (fog-filtered for the human player).

**Auth:** none

**Response:**
```json
{
  "phase": "picking" | "play" | "finished",
  "turn": 1,
  "income": 7,
  "my_territories": 4,
  "enemy_territories": 4,
  "winner": null,
  "pick_options": [3, 7, 12, 18, 25, 31],
  "picks_needed": 4,
  "territories": [
    {
      "id": 0,
      "name": "Alaska",
      "bonus_id": 0,
      "adjacent": [1, 5, 33],
      "owner": 0,
      "armies": 5,
      "visible": true,
      "path": "M 100 200 L ...",
      "label_x": 120.5,
      "label_y": 215.3
    }
  ],
  "bonuses": [
    {
      "id": 0,
      "name": "North America",
      "value": 5,
      "territory_ids": [0, 1, 2, 3, 4, 5, 6, 7, 8],
      "player_count": 3,
      "total": 9
    }
  ],
  "history": [
    {
      "turn": 1,
      "events": [...]
    }
  ],
  "win_probability": {
    "player_0": 0.52,
    "player_1": 0.48,
    "simulations": 0
  },
  "win_prob_history": [0.50, 0.52]
}
```

---

### POST /api/new

Start a new local game. Optionally specify a map template and game settings.

**Auth:** none

**Request body (optional):**
```json
{
  "template": "big_earth",
  "settings": {
    "fog_of_war": true,
    "starting_armies": 5,
    "base_income": 5,
    "num_picks": 4,
    "ai_difficulty": "hard"
  }
}
```

All fields are optional. If `template` is omitted, the current map is reused. If `settings` is omitted, defaults apply.

`ai_difficulty` accepts `"easy"`, `"medium"`, or `"hard"` (default).

**Response:**
```json
{
  "success": true,
  "message": "New game on Medium Earth",
  "events": [],
  "new_achievements": []
}
```

---

### POST /api/picks

Submit territory picks during the picking phase.

**Auth:** none

**Request body:**
```json
{
  "picks": [3, 12, 25, 31]
}
```

The `picks` array contains territory IDs in priority order. Must contain at least `picks_needed` entries (typically 4). Extra picks serve as fallbacks if earlier choices are taken by the AI.

**Response:**
```json
{
  "success": true,
  "message": "Picks resolved. Game begins!",
  "events": [],
  "new_achievements": []
}
```

---

### POST /api/orders

Submit orders (deploy + attack/transfer) for the current turn. The AI submits its orders simultaneously.

**Auth:** none

**Request body:**
```json
{
  "orders": [
    { "Deploy": { "territory": 3, "armies": 7 } },
    { "Attack": { "from": 3, "to": 4, "armies": 10 } },
    { "Transfer": { "from": 5, "to": 3, "armies": 4 } }
  ]
}
```

Order types:
- `Deploy` -- place income armies on a territory you own.
- `Attack` -- attack an adjacent territory you do not own.
- `Transfer` -- move armies between two adjacent territories you own.

**Response:**
```json
{
  "success": true,
  "message": "Turn 3 complete",
  "events": [
    { "Deploy": { "territory": 3, "player": 0, "armies": 7 } },
    { "Attack": { "from": 3, "to": 4, "attackers": 10, "defenders": 4, "captured": true, "surviving_attackers": 7, "surviving_defenders": 0 } },
    { "Capture": { "territory": 4, "player": 0 } }
  ],
  "new_achievements": [
    { "id": "first_win", "name": "First Victory", "description": "Win your first game", "earned": true, "earned_at": "2026-04-15T12:00:00Z" }
  ]
}
```

Events are fog-filtered: you only see events involving territories visible to you before or after the turn.

---

### GET /api/game/replay/{turn}

Get the full (unfogged) board state at a specific turn. Available after the game starts.

**Auth:** none

**Response:**
```json
{
  "turn": 3,
  "phase": "play",
  "territories": [...],
  "events": [...],
  "win_probability": 0.65
}
```

---

### GET /api/game/analysis

Get a deep win probability analysis of the current position using Monte Carlo simulation (200 simulations, <500ms).

**Auth:** none

**Response:**
```json
{
  "win_probability": {
    "player_0": 0.63,
    "player_1": 0.37,
    "simulations": 200
  }
}
```

---

### GET /api/game/post-analysis

Get post-game analysis including turning points, mistake detection, and territory timeline. Best called after a game finishes.

**Auth:** none

**Response:**
```json
{
  "turning_points": [...],
  "territory_timeline": [...],
  "mistakes": [...],
  "summary": "..."
}
```

---

### POST /api/difficulty

Change the AI difficulty for local play. Takes effect on the next turn (or new game).

**Auth:** none

**Request body:**
```json
{
  "level": "medium"
}
```

Accepts `"easy"`, `"medium"`, or `"hard"`.

**Response:**
```json
{
  "success": true,
  "message": "AI difficulty set to Medium",
  "events": [],
  "new_achievements": []
}
```

---

## Authentication Endpoints

### GET /api/auth/discord

Redirects the user to Discord's OAuth2 authorization page. After the user approves, Discord redirects back to the callback URL.

**Auth:** none

**Response:** `302 Redirect` to Discord.

---

### GET /api/auth/discord/callback

Handles the OAuth2 callback from Discord. Exchanges the authorization code for an access token, fetches the user's Discord profile, upserts the user in the database, creates a JWT, and sets it as an HttpOnly cookie.

**Auth:** none

**Query parameters:**
- `code` (string, required) -- the authorization code from Discord.

**Response:** `302 Redirect` to `/app` with `Set-Cookie: token=<jwt>`.

---

### POST /api/auth/logout

Clears the session cookie.

**Auth:** none (clears cookie if present)

**Response:**
```json
{
  "success": true
}
```

---

### GET /api/auth/me

Get the currently authenticated user's profile.

**Auth:** required

**Response:**
```json
{
  "id": "550e8400-e29b-41d4-a716-446655440000",
  "username": "PlayerOne",
  "avatar_url": "https://cdn.discordapp.com/avatars/...",
  "rating": 1523.4,
  "games_played": 42,
  "games_won": 25
}
```

---

## Games (Multiplayer)

All multiplayer game endpoints require authentication and a configured database.

### POST /api/games

Create a new multiplayer game.

**Auth:** required

**Request body:**
```json
{
  "template": "small_earth"
}
```

**Response:**
```json
{
  "id": "550e8400-e29b-41d4-a716-446655440000",
  "template": "small_earth",
  "status": "waiting",
  "player_a": "...",
  "player_b": null,
  "winner_id": null,
  "turn": 0,
  "created_at": "2026-04-15T12:00:00+00:00",
  "finished_at": null
}
```

---

### GET /api/games

List games. Without a `status` filter, returns the authenticated user's games.

**Auth:** required

**Query parameters:**
- `status` (string, optional) -- filter by status: `"waiting"`, `"picking"`, `"active"`, `"finished"`.
- `limit` (integer, optional) -- max results (default 20).

**Response:** Array of game objects (same schema as POST /api/games response).

---

### GET /api/games/{id}

Get a specific game's state, fog-filtered for the requesting player.

**Auth:** required

**Response:**
```json
{
  "game": { ... },
  "state": { ... },
  "pick_options": [3, 7, 12],
  "my_seat": 0
}
```

`my_seat` is `0` (player A), `1` (player B), or `null` (spectator).

---

### POST /api/games/{id}/join

Join an open game as player B.

**Auth:** required

**Response:** Game object with updated `player_b` and `status`.

---

### POST /api/games/{id}/picks

Submit picks for a multiplayer game during the picking phase.

**Auth:** required

**Request body:**
```json
{
  "picks": [3, 12, 25, 31]
}
```

**Response:**
```json
{
  "success": true,
  "message": "picks submitted"
}
```

---

### POST /api/games/{id}/orders

Submit orders for a multiplayer game during the play phase.

**Auth:** required

**Request body:**
```json
{
  "orders": [
    { "Deploy": { "territory": 3, "armies": 7 } },
    { "Attack": { "from": 3, "to": 4, "armies": 10 } }
  ]
}
```

**Response:**
```json
{
  "success": true,
  "message": "orders submitted"
}
```

Turn resolution happens when both players have submitted orders.

---

## Maps

### GET /api/maps

List all available maps (built-in and custom).

**Auth:** none

**Response:**
```json
{
  "maps": [
    {
      "id": "small_earth",
      "name": "Small Earth",
      "territories": 42,
      "bonuses": 6,
      "is_custom": false
    },
    {
      "id": "custom/my-map",
      "name": "My Custom Map",
      "territories": 20,
      "bonuses": 4,
      "is_custom": true
    }
  ]
}
```

---

### POST /api/maps

Save a custom map. The request body is the full map JSON (same format used in `maps/*.json` files).

**Auth:** none

The server validates:
- At least one territory and one bonus.
- Non-empty map ID.
- Full graph connectivity (all territories reachable).
- Bidirectional adjacency (if A is adjacent to B, B must be adjacent to A).

**Response:**
```json
{
  "id": "custom/my-map",
  "name": "My Custom Map",
  "territories": 20,
  "bonuses": 4,
  "is_custom": true
}
```

**Errors:**
- `400` if validation fails (with descriptive message).

---

### DELETE /api/maps/{id}

Delete a custom map by ID.

**Auth:** none

**Response:**
```json
{
  "deleted": true
}
```

**Errors:**
- `404` if map not found.

---

## Matchmaking Queue

### POST /api/queue/join

Join the matchmaking queue. The server pairs players with similar ratings.

**Auth:** required

**Request body:**
```json
{
  "template": "small_earth"
}
```

**Response:**
```json
{
  "success": true,
  "message": "Joined matchmaking queue"
}
```

Returns `"Already in queue"` if the user is already queued.

When a match is found, both players are notified via WebSocket with a `MatchFound` event containing the game ID.

---

### POST /api/queue/leave

Leave the matchmaking queue.

**Auth:** required

**Response:**
```json
{
  "success": true,
  "message": "Left matchmaking queue"
}
```

---

### GET /api/queue/status

Check your position in the matchmaking queue.

**Auth:** required

**Response:**
```json
{
  "queued": true,
  "position": 2,
  "estimated_wait_secs": 15,
  "queue_size": 5
}
```

If not in queue:
```json
{
  "queued": false,
  "position": null,
  "estimated_wait_secs": null,
  "queue_size": 5
}
```

---

## League and Seasons

### GET /api/seasons

List all seasons.

**Auth:** none (requires database)

**Response:**
```json
[
  {
    "id": 1,
    "name": "Season 1",
    "starts_at": "2026-01-01T00:00:00+00:00",
    "ends_at": "2026-04-01T00:00:00+00:00",
    "is_active": false
  },
  {
    "id": 2,
    "name": "Season 2",
    "starts_at": "2026-04-01T00:00:00+00:00",
    "ends_at": "2026-07-01T00:00:00+00:00",
    "is_active": true
  }
]
```

---

### GET /api/seasons/current

Get the currently active season.

**Auth:** none (requires database)

**Response:** A single season object, or `null` if no season is active.

---

### GET /api/seasons/{id}/standings

Get the leaderboard for a specific season.

**Auth:** none (requires database)

**Query parameters:**
- `limit` (integer, optional) -- max results (default 50).

**Response:**
```json
[
  {
    "user_id": "...",
    "username": "PlayerOne",
    "avatar_url": "...",
    "rank_tier": "gold",
    "rank_points": 350,
    "rank_color": "#ffd700",
    "wins": 15,
    "losses": 8,
    "streak": 3,
    "peak_rank_points": 420,
    "win_rate": 0.652
  }
]
```

Rank tiers and their point thresholds:

| Tier | Min RP | Color |
|------|--------|-------|
| Bronze | 0 | #cd7f32 |
| Silver | 100 | #c0c0c0 |
| Gold | 250 | #ffd700 |
| Platinum | 500 | #e5e4e2 |
| Diamond | 1000 | #b9f2ff |
| Master | 2000 | #9b59b6 |
| Grandmaster | 4000 | #e74c3c |

---

### GET /api/seasons/{season_id}/standings/{user_id}

Get a specific player's stats for a specific season.

**Auth:** none (requires database)

**Response:**
```json
{
  "season": { ... },
  "standing": { ... }
}
```

---

### GET /api/match-history

Paginated match history.

**Auth:** none (requires database)

**Query parameters:**
- `user_id` (UUID, optional) -- filter to a specific player's matches.
- `season_id` (integer, optional) -- filter to a specific season.
- `limit` (integer, optional) -- max results (default 20, max 100).
- `offset` (integer, optional) -- pagination offset (default 0).

**Response:**
```json
[
  {
    "id": "...",
    "game_id": "...",
    "season_id": 2,
    "player_a": "...",
    "player_b": "...",
    "winner_id": "...",
    "player_a_rating_change": 12.5,
    "player_b_rating_change": -12.5,
    "player_a_rp_change": 25,
    "player_b_rp_change": -20,
    "turns_played": 18,
    "template": "small_earth",
    "played_at": "2026-04-15T12:00:00+00:00"
  }
]
```

---

## Ladder

### GET /api/ladder

Get the top players by Glicko-2 rating.

**Auth:** none (requires database)

**Query parameters:**
- `limit` (integer, optional) -- max results (default 50).

**Response:**
```json
[
  {
    "id": "...",
    "username": "PlayerOne",
    "avatar_url": "...",
    "rating": 1650.3,
    "rd": 45.2,
    "games_played": 100,
    "games_won": 62,
    "win_rate": 0.62
  }
]
```

---

### GET /api/users/{id}

Get a player's public profile.

**Auth:** none (requires database)

**Response:** Same schema as a single ladder entry.

---

## Spectate

### GET /api/games/active

List currently active multiplayer games (status: waiting, picking, or active).

**Auth:** none (requires database)

**Query parameters:**
- `limit` (integer, optional) -- max results (default 30).

**Response:**
```json
[
  {
    "id": "...",
    "template": "small_earth",
    "status": "active",
    "player_a_name": "PlayerOne",
    "player_b_name": "PlayerTwo",
    "turn": 12,
    "created_at": "...",
    "finished_at": null,
    "winner_name": null
  }
]
```

---

### GET /api/games/recent

List recently completed games.

**Auth:** none (requires database)

**Query parameters:**
- `limit` (integer, optional) -- max results (default 20).

**Response:** Same schema as active games, with `finished_at` and `winner_name` populated.

---

### GET /api/games/{id}/spectate

Get the full unfogged game state for spectating. Includes win probability.

**Auth:** optional (works for logged-in users and anonymous spectators)

**Response:**
```json
{
  "game": { ... },
  "state": { ... },
  "map": { ... },
  "pick_options": [...],
  "win_probability": {
    "player_0": 0.58,
    "player_1": 0.42
  }
}
```

---

## Arena Tournaments

### POST /api/arenas

Create an arena tournament.

**Auth:** required

**Request body:**
```json
{
  "name": "Friday Night Arena",
  "template": "small_earth",
  "start_time": "2026-04-18T20:00:00Z",
  "end_time": "2026-04-18T21:00:00Z",
  "time_control_secs": 300
}
```

`time_control_secs` defaults to 300 (5 minutes per player) if omitted.

**Response:**
```json
{
  "id": "...",
  "name": "Friday Night Arena",
  "template": "small_earth",
  "start_time": "...",
  "end_time": "...",
  "time_control_secs": 300,
  "status": "upcoming",
  "participant_count": 0,
  "created_at": "..."
}
```

---

### GET /api/arenas

List active and upcoming arena tournaments.

**Auth:** none (requires database)

**Response:** Array of arena objects.

---

### GET /api/arenas/{id}

Get arena details with the full participant leaderboard.

**Auth:** none (requires database)

**Response:**
```json
{
  "arena": { ... },
  "participants": [
    {
      "user_id": "...",
      "username": "PlayerOne",
      "avatar_url": "...",
      "score": 15,
      "games_played": 5,
      "wins": 4,
      "current_streak": 3,
      "rank": 1
    }
  ]
}
```

---

### POST /api/arenas/{id}/join

Join an arena tournament. Must be active or upcoming (not finished).

**Auth:** required

**Response:**
```json
{
  "success": true,
  "message": "joined arena 'Friday Night Arena'"
}
```

---

### GET /api/arenas/{id}/leaderboard

Get the ranked participant list for an arena.

**Auth:** none (requires database)

**Response:** Array of participant objects sorted by score descending.

---

## Puzzles

### GET /api/puzzle/today

Get today's daily puzzle. The puzzle is deterministically generated from the current date.

**Auth:** none

**Response:**
```json
{
  "id": "puzzle-19827",
  "day_seed": 19827,
  "description": "Capture the bonus in one turn",
  "hint": "Focus your deployment on the weakest border",
  "difficulty": "medium",
  "puzzle_type": "capture_bonus",
  "income": 7,
  "player": 0,
  "territories": [...],
  "bonuses": [...]
}
```

The optimal solution is not included in the response.

---

### POST /api/puzzle/submit

Submit a solution to a daily puzzle.

**Auth:** none

**Request body:**
```json
{
  "orders": [
    { "Deploy": { "territory": 2, "armies": 7 } },
    { "Attack": { "from": 2, "to": 5, "armies": 8 } }
  ],
  "day_seed": 19827
}
```

**Response:**
```json
{
  "correct": true,
  "message": "Correct! You found the optimal play.",
  "objective_met": true,
  "optimal_orders": [...]
}
```

---

## Stats and Achievements

### GET /api/stats

Get cumulative local play statistics.

**Auth:** none

**Response:**
```json
{
  "games_played": 15,
  "wins": 9,
  "losses": 6,
  "win_rate": 60.0,
  "total_turns": 180,
  "avg_game_length": 12.0,
  "streak": 3,
  "rating": 1580.2,
  "rd": 120.5,
  "rank_tier": "Gold",
  "rating_history": [1500, 1540, 1520, 1580],
  "start_win_probs": [0.5, 0.48, 0.52],
  "match_history": [
    {
      "game_number": 15,
      "result": "win",
      "turns": 11,
      "map": "Small Earth",
      "rating_after": 1580.2
    }
  ],
  "bonus_captures": [["North America", 5], ["Europe", 3]],
  "pick_choices": [["Alaska", 8], ["Brazil", 6]]
}
```

---

### GET /api/achievements

Get the achievement list with earned status.

**Auth:** none

**Response:**
```json
{
  "achievements": [
    {
      "id": "first_win",
      "name": "First Victory",
      "description": "Win your first game",
      "earned": true,
      "earned_at": "2026-04-15T12:00:00Z"
    }
  ],
  "earned": 5,
  "total": 12
}
```

---

## Lobby

### GET /api/lobby

List open multiplayer games waiting for an opponent to join.

**Auth:** none (requires database)

**Response:** Array of game objects with status `"waiting"`.

---

## WebSocket

### GET /ws

Upgrade to a WebSocket connection for real-time game updates.

The WebSocket hub broadcasts events to subscribed clients:
- **Game state updates** when turns resolve.
- **MatchFound** events when matchmaking pairs two players.
- **Turn notifications** when your opponent submits orders.

Connect after authentication to receive events for your games.
