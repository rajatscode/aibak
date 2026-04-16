# strat-club

Turn-based territory strategy game. 1v1 online, Glicko-rated ladder, Discord auth.

## Architecture

Rust workspace with three crates:

- `crates/engine` — Pure game logic (no IO, no async). Combat, turns, fog, picking, cards, AI.
- `crates/server` — Axum web server. Modules: `api/`, `auth/`, `db/`, `game/`, `ws/`.
- `crates/cli` — CLI game runner for local testing.

Frontend: Single-file HTML at `crates/static/index.html` (embedded via `include_str!`).
Maps: JSON files in `maps/` (Small Earth 42 territories, MME 89 territories).

## Development

```bash
# Run locally (no DB needed for single-player)
cargo run --bin strat-server

# Run with multiplayer (needs PostgreSQL)
cp .env.example .env  # fill in credentials
cargo run --bin strat-server

# Run tests
cargo test

# Check for warnings
cargo clippy
```

## Key Commands

- `cargo test` — run all tests
- `cargo clippy` — lint
- `cargo fmt` — format

## Templates

- **Small Earth**: 42 territories, 6 bonuses (classic Risk layout with SVG paths)
- **Modified Medium Earth (MME)**: 89 territories, 22 bonuses (no SVG paths yet)

## Game Mechanics

- Random Warlords picking (1 territory per bonus offered)
- 0% luck deterministic combat (60% offense / 70% defense kill rates)
- Fog of war (see owned + adjacent territories only)
- Reinforcement and Blockade cards
- 5 starting armies per picked territory
- Base income: 5 armies/turn + bonus income

## Legal

NEVER reference competing products by name in code, comments, docs, or commits.
Use "Deploy" and "Move" phases (not "attack/transfer").

## Environment Variables (for multiplayer)

- `DATABASE_URL` — PostgreSQL connection string
- `DISCORD_CLIENT_ID` — Discord OAuth2 app ID
- `DISCORD_CLIENT_SECRET` — Discord OAuth2 secret
- `DISCORD_REDIRECT_URI` — OAuth2 callback URL (default: http://localhost:3000/api/auth/discord/callback)
- `JWT_SECRET` — Secret for signing JWT tokens
