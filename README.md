# strat.club

**Competitive 1v1 territory strategy. Deterministic combat. No luck. Pure skill.**

<!-- badges -->
![License: Barbarian States v1.0](https://img.shields.io/badge/license-Barbarian%20States%20v1.0-blue)
![Rust](https://img.shields.io/badge/rust-2024%20edition-orange)
![Status](https://img.shields.io/badge/status-active%20development-green)

**[Play at strat.club](https://strat.club)**

---

[screenshot placeholder]

---

## Features

### Core Game
- **Deterministic combat** -- 0% luck. Every attack has a known, exact outcome. No dice, no randomness.
- **Fog of war** -- you only see territories you own and their neighbors. The rest is hidden.
- **ABBA snake draft** -- fair territory selection with alternating pick order (A, B, B, A, ...).
- **Reinforcement and Blockade cards** -- earned through territory captures, adding tactical depth.
- **Continent bonuses** -- control all territories in a region for extra income each turn.

### AI Opponents
- **Easy** -- random deployment and attacks. Good for learning.
- **Medium** -- greedy heuristic with bonus-completion priority, counter-expansion, and multi-step attack planning.
- **Hard** -- Monte Carlo Tree Search (MCTS) with UCB1 selection and 500ms time budget. Plays at a strong level.

### Map Editor
- Create custom maps with auto-generated territory shapes.
- Force-directed layout with organic territory boundaries.
- Validates connectivity and bidirectional adjacency on save.

### Competitive Multiplayer
- **Discord OAuth2** authentication.
- **Glicko-2 rated ladder** with rating deviation tracking.
- **Seasonal leagues** with rank tiers: Bronze, Silver, Gold, Platinum, Diamond, Master, Grandmaster.
- **Matchmaking queue** with rating-based pairing.
- **24-hour boot timers** for inactive players.
- **Arena tournaments** -- drop-in, time-boxed competitions with live leaderboards.

### Analysis and Spectating
- **Live win probability** -- three-layer evaluation (material heuristic, 1-ply lookahead, full Monte Carlo).
- **Post-game analysis** -- turning-point detection, territory-control timeline, mistake highlighting.
- **Turn replay** -- step through any completed game with full board visibility.
- **Spectator mode** -- browse and watch active games in real time.

### Daily Puzzles
- A new territory puzzle every day, seeded by date.
- Multiple puzzle types with difficulty ratings and hints.
- Submit solutions and compare against the optimal play.

### Player Profiles and Stats
- Rating history chart and match history.
- Win rate, streak tracking, bonus capture frequency.
- Achievement system with progression milestones.

---

## Quick Start

```bash
# 1. Clone and build
git clone https://github.com/your-org/strat-club.git && cd strat-club

# 2. Run the server (single-player vs AI, no database needed)
cargo run --bin strat-server

# 3. Open your browser
open http://localhost:3000
```

That's it. No database, no environment variables, no configuration required for local play.

### Run with a different map

```bash
cargo run --bin strat-server -- maps/mme.json
```

### Enable multiplayer

Multiplayer requires PostgreSQL and a Discord application:

```bash
cp .env.example .env   # fill in DATABASE_URL, Discord client ID/secret, JWT secret
cargo run --bin strat-server
```

### Run tests

```bash
cargo test
```

---

## Game Guide

### How a game works

1. **Pick phase** -- One territory per continent is offered at random. Players draft 4 starting territories using ABBA snake order.
2. **Deploy phase** -- Place your income (base 5 + continent bonuses) on your territories. Click to add +1, Shift+click for +5.
3. **Move phase** -- Attack enemy or neutral territories, or transfer armies between your own. Queue your orders, then end your turn.
4. **Win condition** -- Capture all enemy territories.

### Combat rules (0% luck)

Combat is fully deterministic:
- Each attacking army kills 0.6 defenders (offense kill rate 60%).
- Each defending army kills 0.7 attackers (defense kill rate 70%).
- Results are rounded (0.5 rounds up), producing exact outcomes every time.

**Key breakpoints:**

| Attackers | Defenders | Result |
|-----------|-----------|--------|
| 2 | 1 | Capture (1 survivor) |
| 3 | 2 | Capture (2 survivors) |
| 5 | 3 | Capture (3 survivors) |
| 17 | 10 | Capture (10 survivors) |

Rule of thumb: you need roughly 1.7x the defenders to capture a territory.

For the full interactive tutorial with a combat calculator, visit [/tutorial](http://localhost:3000/tutorial).

---

## Architecture

```
strat-club/
  crates/
    engine/       Pure game logic (zero IO dependencies)
                    combat, turns, fog, picking, cards
                    AI (greedy + MCTS), win probability analysis
                    daily puzzles, game analysis

    server/       Axum web server
                    api/        REST endpoints (games, orders, maps, queue, league, etc.)
                    auth/       Discord OAuth2 + JWT sessions
                    db/         PostgreSQL (users, games, seasons, standings, arenas)
                    game/       Game manager, matchmaking, rating, timers, tournaments
                    ws/         WebSocket hub for live updates

    cli/          CLI game runner (play in terminal)

    static/       Embedded HTML/JS pages
                    game, editor, tutorial, profile, puzzle
                    games browser, landing page

  maps/           JSON map definitions
                    small_earth.json    (42 territories)
                    mme.json            (89 territories, Modified Medium Earth)
                    custom/             User-created maps
```

The `engine` crate is pure functions: game state in, new game state out. No network, no filesystem, no randomness source baked in. This makes it fully unit-testable and opens the door to WASM compilation for client-side replay.

The `server` crate handles all IO: HTTP, WebSocket, database, and Discord OAuth. It embeds the static HTML frontends at compile time.

---

## Tech Stack

| Layer | Technology |
|-------|-----------|
| Backend | Rust (Axum), Tokio async runtime |
| Database | PostgreSQL (via sqlx) |
| Auth | Discord OAuth2 + JWT |
| Rating | Glicko-2 (Mark Glickman's algorithm) |
| AI | MCTS with UCB1 + greedy rollouts |
| Frontend | Vanilla HTML/JS, SVG map rendering |
| Deployment | Docker, Fly.io |

---

## API Reference

Full API documentation with request/response schemas and examples is available in [docs/API.md](docs/API.md).

---

## Contributing

1. Fork the repository.
2. Create a feature branch: `git checkout -b my-feature`.
3. Make your changes. Run `cargo test` and `cargo clippy` before committing.
4. Submit a pull request with a clear description of the change.

### Project conventions

- The `engine` crate must remain free of IO dependencies. All randomness is injected via `impl Rng` parameters.
- Use "Deploy" and "Move" for game phase names.
- Never reference competing products by name in code, comments, or documentation.

---

## License

[Barbarian States License v1.0](LICENSE)

MIT-style permissions with one restriction: agents of authoritarian governments (as defined by international human rights assessments) are prohibited from using this software. Private citizens and residents of any country acting in their personal capacity are unrestricted. Pro-democracy activists, journalists, and human rights defenders receive additional broad permissions.

Copyright (c) 2026 Rajat Mehndiratta
