# strat.club QA Report

**Date:** 2026-04-15
**Scope:** Full codebase quality sweep

---

## 1. HTML Pages -- Render & JS Validation

All 7 HTML pages were reviewed for structural validity and JavaScript correctness.

| Page | DOCTYPE | Tags | JS Functions | Status |
|------|---------|------|-------------|--------|
| index.html | OK | Balanced | All 41 onclick handlers have matching function definitions | PASS |
| editor.html | OK | Balanced | All onclick handlers (`toggleHelp`, `addBonus`, `deleteSelected`, `autoLayout`, `generateShapes`, `autoClusters`, `validateGraph`, `doExport`, `showImport`) resolve correctly | PASS |
| tutorial.html | OK | Balanced | Step navigation (`goStep`), keyboard listeners, `svgMap` helper all defined | PASS |
| landing.html | OK | Balanced | IntersectionObserver scroll-reveal; no onclick handlers to validate | PASS |
| profile.html | OK | Balanced | Async `loadProfile()` with proper error handling; all builder functions defined | PASS |
| games.html | OK | Balanced | Tab switching, spectator mode, replay stepper; all onclick handlers (`spectateLocal`, `exitSpectator`, `stepTurn`) defined | PASS |
| puzzle.html | OK | Balanced | Territory click handling, deploy/attack state machine, submit/reset flow all defined | PASS |

**Findings:** No unclosed tags, no missing function references, no obvious JS errors detected in any page.

---

## 2. Server Routes -- Compilation & Wiring

**`cargo check -p strat-server`:** Compiles cleanly with zero errors.

All routes in the Axum router (`main.rs:1468-1557`) were verified against their handler function definitions:

### Local Play Routes
- `GET /` -> `index` (main.rs:715)
- `GET /favicon.ico`, `/favicon.svg` -> `favicon` (main.rs:723)
- `GET /api/game` -> `get_local_game` (main.rs:325)
- `POST /api/picks` -> `submit_local_picks` (main.rs:330)
- `POST /api/orders` -> `submit_local_orders` (main.rs:389)
- `POST /api/new` -> `new_local_game` (main.rs:584)
- `GET /api/game/replay/{turn}` -> `get_replay_turn` (main.rs:655)
- `GET /api/game/analysis` -> `get_analysis` (main.rs:868)
- `GET /api/game/post-analysis` -> `get_post_analysis` (main.rs:1121)
- `GET /api/game/export` -> `export_game` (main.rs:881)
- `POST /api/game/import` -> `import_game` (main.rs:973)
- `POST /api/difficulty` -> `set_difficulty` (main.rs:1146)
- `GET /api/stats` -> `get_local_stats` (main.rs:1174)
- `GET /api/achievements` -> `get_achievements` (main.rs:1246)
- `GET /api/openings` -> `get_openings` (main.rs:1345)

### Page Routes
- `GET /landing` -> `landing` (main.rs:1257)
- `GET /profile` -> `profile_page` (main.rs:1170)
- `GET /editor` -> `editor` (main.rs:719)
- `GET /tutorial` -> `tutorial_page` (main.rs:1164)
- `GET /puzzle` -> `puzzle_page` (main.rs:1267)
- `GET /games` -> `games_page` (main.rs:1160)
- `GET /app` -> `app_placeholder` (main.rs:1261)

### Puzzle API
- `GET /api/puzzle/today` -> `get_today_puzzle` (main.rs:1272)
- `POST /api/puzzle/submit` -> `submit_puzzle` (main.rs:1327)

### Spectate API
- `GET /api/games/active` -> `api::spectate::active_games`
- `GET /api/games/recent` -> `api::spectate::recent_games`
- `GET /api/games/{id}/spectate` -> `api::spectate::spectate_game`

### Auth Routes
- `GET /api/auth/discord` -> `auth_discord_redirect`
- `GET /api/auth/discord/callback` -> `auth_discord_callback`
- `POST /api/auth/logout` -> `auth_logout`
- `GET /api/auth/me` -> `auth_me`

### Multiplayer Game API
- `POST /api/games` -> `api::games::create_game`
- `GET /api/games` -> `api::games::list_games`
- `GET /api/games/{id}` -> `api::games::get_game`
- `POST /api/games/{id}/join` -> `api::games::join_game`
- `POST /api/games/{id}/picks` -> `api::orders::submit_picks`
- `POST /api/games/{id}/orders` -> `api::orders::submit_orders`

### Ladder, League, Queue, Arena, Maps, WebSocket
All handler functions verified present in their respective modules.

**Result:** All 50+ routes compile and are wired to existing handler functions. **PASS**

---

## 3. Engine Module Exports

**File:** `crates/engine/src/lib.rs`

| Module file | `pub mod` in lib.rs | Status |
|-------------|---------------------|--------|
| ai.rs | `pub mod ai` | PASS |
| analysis.rs | `pub mod analysis` | PASS |
| cards.rs | `pub mod cards` | PASS |
| combat.rs | `pub mod combat` | PASS |
| fog.rs | `pub mod fog` | PASS |
| game_analysis.rs | `pub mod game_analysis` | PASS |
| map.rs | `pub mod map` | PASS |
| mcts.rs | `pub mod mcts` | PASS |
| openings.rs | `pub mod openings` | PASS |
| orders.rs | `pub mod orders` | PASS |
| picking.rs | `pub mod picking` | PASS |
| puzzle.rs | `pub mod puzzle` | PASS |
| state.rs | `pub mod state` | PASS |
| turn.rs | `pub mod turn` | PASS |

**Result:** All 14 `.rs` files in `crates/engine/src/` have corresponding `pub mod` entries. **PASS**

---

## 4. Map Data Integrity

Adjacency validation (bidirectional check) on both map files:

| Map | Territories | Adjacency Errors |
|-----|-------------|-----------------|
| maps/small_earth.json | 42 | 0 |
| maps/big_earth.json | 89 | 0 |

**Result:** Both maps have fully symmetric adjacency graphs. **PASS**

---

## 5. Branding Check -- Legacy Name References

Searched `crates/static/`, `docs/`, and `README.md` for any legacy project name references.

**Result:** No references found. All user-facing content uses "strat.club" branding. **PASS**

---

## 6. Test Suite

```
cargo test
```

**Result:** 19 tests passed, 0 failed, 0 ignored.

Tests cover:
- Rating system (Glicko-2): default rating, RD decrease, winner gains, upset bonus
- League system: RP changes, tier boundaries, streak bonuses, minimum RP floor
- Achievements: first blood, explorer, speedrun, underdog, bonus hunter, no duplicates
- Tournament: win points, streak bonuses, berserk bonus

**PASS**

---

## 7. Compiler Warnings

```
cargo build 2>&1 | grep "warning:" | grep -v "generated"
```

**Result:** Zero warnings. **PASS**

---

## Summary

| Check | Result |
|-------|--------|
| HTML pages valid & JS correct | PASS |
| Server routes compile & wired | PASS |
| Engine modules all exported | PASS |
| Map adjacency integrity | PASS |
| No legacy name references | PASS |
| Test suite (19/19) | PASS |
| Compiler warnings | PASS (0 warnings) |

**Overall: All checks pass. The codebase is clean and ready for deployment.**
