# Product — strat.club

## Core User Story

**Play competitive 1v1 territory strategy games on a ranked ladder.**

That's it. Everything else is a distraction until this works perfectly.

## Current State: ALPHA

The core loop (pick → deploy → move → resolve → repeat) works locally against AI. Multiplayer infrastructure exists but is not deployed. No real users yet.

## Critical Path to Launch

1. **Deploy to strat.club** — Fly.io + Postgres + Discord OAuth
2. **Multiplayer works end-to-end** — two real humans play a game via the web
3. **Ladder is live** — Glicko-2 ratings, visible leaderboard
4. **Two boards playable** — Small Earth Strategic, MME Strategic

Everything else comes after these four things are done.

## Known Issues

### Performance
- ~~Hover connection lines are unusably slow~~ (disabled)
- Frontend is a single 3000+ line HTML file — hard to maintain, may have runtime perf issues from sheer size
- MCTS AI on Hard can block the server thread during computation

### Feature Bloat
The overnight build session added many features that don't serve the core user story:
- Daily puzzles — nice to have, not launch critical
- Arena tournaments — needs multiplayer first
- Achievement system — fires incorrectly (client-side band-aid applied, server-side root cause unfixed)
- Chat/emoji system — local AI chat is gimmicky
- Territory annotations — niche power-user feature
- Opening book/tips — premature before real meta develops
- Game export/import — niche
- Theme toggle — cosmetic
- Session stats bar — cosmetic

**None of these should get attention until the critical path is done.**

### Architecture Concerns
- Single HTML file is unsustainable — needs SvelteKit or similar
- Server main.rs is 1500+ lines — needs splitting into route modules
- Achievement checking logic is fundamentally broken server-side
- MapFile/Map/Board loading has three code paths (old format, new split, inline) — should converge

## What "Done" Looks Like for v1.0

- [ ] strat.club is live and accessible
- [ ] Discord sign-in works
- [ ] Player can queue for a match
- [ ] Two players get paired and play a multi-day async game
- [ ] 24h boot timer enforced
- [ ] Game results update Glicko-2 ratings
- [ ] Leaderboard shows top players
- [ ] Small Earth and MME boards are playable
- [ ] Fog of war works correctly in multiplayer
- [ ] Game replay works after completion
- [ ] No critical bugs in core game loop

## What We're NOT Building Right Now

- Mobile app
- More than 2 maps
- Tournaments/arenas
- Social features beyond basic chat
- Custom game settings UI (use defaults)
- AI improvements (local play is good enough)
- Map editor polish
- Landing page marketing
