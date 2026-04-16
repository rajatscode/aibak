# Aibak

A turn-based territory strategy game. Conquer the map, outmaneuver your opponent, and climb the ladder.

**[Play at strat.club](https://strat.club)** *(coming soon)*

## Features

- **Deterministic combat** — no dice, no luck. Pure strategy.
- **Fog of war** — you only see territories adjacent to your own.
- **MCTS AI** — Monte Carlo Tree Search opponent with three difficulty levels.
- **Live win probability** — see your chances shift in real-time as the game evolves.
- **Glicko-2 rated ladder** — competitive matchmaking with Discord authentication.
- **Map editor** — create custom maps with auto-generated territory shapes.
- **Two official maps** — Medium Earth (42 territories) and Modified Medium Earth (89 territories).

## How to Play

1. **Pick phase** — Choose 4 starting territories from the random offerings (one per continent). Snake draft order (ABBA).
2. **Deploy phase** — Place your income armies on your territories. Click to add +1, Shift+click for +5.
3. **Move phase** — Attack enemy/neutral territories or transfer armies between your own. Queue orders, then end turn.
4. **Repeat** — Capture all enemy territories to win.

### Combat Rules

At 0% luck, combat is deterministic:
- Each attacking army has a 60% chance to eliminate a defender
- Each defending army has a 70% chance to eliminate an attacker
- Results are rounded (0.5 rounds up), producing exact, predictable outcomes

**Key breakpoints:** 2 armies beats 1. 3 beats 2. 5 beats 3. You need roughly 1.7x the defenders to capture.

### Bonuses

Control all territories in a continent to earn bonus armies each turn. Small continents are easier to hold but worth less.

## Development

```bash
# Run locally (single-player vs AI, no database needed)
cargo run --bin strat-server
# Then open http://localhost:3000

# Run on a different map
cargo run --bin strat-server -- maps/mme.json

# Map editor
# Open http://localhost:3000/editor

# Run tests
cargo test

# With multiplayer (needs PostgreSQL + Discord app)
cp .env.example .env  # fill in credentials
cargo run --bin strat-server
```

## Architecture

```
crates/
  engine/     Pure game logic (combat, turns, fog, AI, MCTS, analysis)
  server/     Axum web server (API, auth, DB, WebSocket, game manager)
  cli/        CLI game runner
  static/     Embedded HTML frontends (game + map editor)
maps/         JSON map definitions
```

The engine crate has zero IO dependencies — it's pure functions that take game state and produce new game state. This makes it fully unit-testable and opens the door to WASM compilation for client-side replay.

## Tech Stack

- **Backend:** Rust (Axum) + PostgreSQL
- **Auth:** Discord OAuth2 + JWT
- **Rating:** Glicko-2
- **AI:** Monte Carlo Tree Search with greedy rollouts
- **Frontend:** Vanilla HTML/JS with SVG map rendering

## License

[Barbarian States License v1.0](LICENSE) — MIT-style permissions with restrictions on use by agents of authoritarian governments.
