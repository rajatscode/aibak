# strat.club

Competitive territory strategy game. 1v1, Glicko-rated ladder with seasons, Discord auth.

## Architecture

Rust workspace:
- `crates/engine` — Pure game logic (no IO). Combat, turns, fog, picking, cards, AI (greedy + MCTS), win probability analysis, openings book.
- `crates/server` — Axum web server. Modules: `api/`, `auth/`, `config/`, `db/`, `game/` (matchmaking, rating, league, achievements, tournaments, timers), `ws/`.
- `crates/cli` — CLI game runner.
- `crates/static/` — Embedded HTML pages (index, game, games, ladder, feedback, landing).
- `maps/` — JSON map definitions (Small Earth, Big Earth).

## Pages

| Route | Description |
|-------|-------------|
| `/` | Main game (local play vs AI) |
| `/games` | Multiplayer hub: find/create/join games, spectate |
| `/game/{id}` | Multiplayer game board (WebSocket, fog of war) |
| `/ladder` | Leaderboard with tier badges (Bronze → Grandmaster) |
| `/feedback` | User feedback with voting |
| `/landing` | Marketing landing page |

## Running

```bash
# Local play (no DB needed)
cargo run --bin strat-server

# With multiplayer
cp .env.example .env  # fill in credentials
cargo run --bin strat-server

# Tests
cargo test

# Specific map
cargo run --bin strat-server -- maps/big_earth.json
```

## Game Mechanics

- Random Warlords picking (1 per bonus, ABBA snake draft)
- 0% luck deterministic combat (60% offense / 70% defense kill rates)
- Fog of war
- Reinforcement + Blockade cards
- 5 starting armies, 5 base income

## AI

- **Easy**: Random deployment + attacks
- **Medium**: Greedy heuristic with bonus-completion priority
- **Hard**: MCTS with UCB1 selection, 500ms time budget

## Win Probability

Three-layer evaluation:
1. `quick_win_probability` (<1ms) — logistic function over material evaluation
2. `win_probability_with_lookahead` (<50ms) — 1-ply deterministic search
3. `full_win_probability` (<500ms) — Monte Carlo with calibrated output

## Multiplayer Stack

- Discord OAuth2 + JWT sessions
- PostgreSQL (users, games, orders, seasons, standings, match history)
- WebSocket for live game updates
- Matchmaking queue with rating-based pairing
- Glicko-2 ratings + seasonal league (Bronze → Grandmaster)
- Achievements system (First Blood, Explorer, Speedrun, Underdog, Bonus Hunter)
- Arena tournaments with streak bonuses and berserk mode
- 24h boot timers

## API Routes

```
Local:     GET /, POST /api/new, GET /api/game, POST /api/picks, POST /api/orders
           GET /api/game/analysis, POST /api/difficulty, GET /api/stats
           GET /api/achievements, GET /api/game/replay/{turn}
Auth:      GET /api/auth/discord, /callback, POST /logout, GET /me
Games:     POST /api/games, GET /api/games, GET /:id, POST /:id/join, picks, orders
Feedback:  POST /api/feedback, GET /api/feedback, POST /api/feedback/:id/vote
           DELETE /api/feedback/:id (rate limited: 5/hr submit, 30/hr vote)
Maps:      GET /api/maps, POST /api/maps, DELETE /api/maps/:id
Queue:     POST /api/queue/join, POST /api/queue/leave, GET /api/queue/status
League:    GET /api/seasons, /current, /:id/standings, GET /api/match-history
Ladder:    GET /api/ladder, GET /api/users/:id
Spectate:  GET /api/games/:id/spectate
WebSocket: GET /ws
```

## Legal

NEVER reference competing products. Use "Deploy" and "Move" phases.
Barbarian States License v1.0.
