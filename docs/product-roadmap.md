# strat.club Product Roadmap -- Top 10 Feature Proposals

> Prepared 2026-04-15 | Competitive 1v1 Territory Strategy Game
>
> Guiding principle: **Fight for the users.** Every feature must earn its place by making the competitive experience fairer, faster, or more satisfying for players.

---

## Research Summary

This roadmap is informed by analysis of what the best competitive platforms get right and what territory strategy players consistently complain about:

**What works in top competitive platforms:**
- Glicko-2 rating with rating deviation (RD) that decays with inactivity, preventing stale ratings from poisoning matchmaking
- Near-instant re-queue / quick rematch so the loop between games is seconds, not minutes
- Tiered visible ranks (letter grades or icons) that give players identity and short-term goals beyond a raw number
- Daily puzzles, streaks, and seasonal resets that create habitual engagement loops
- Arena-style tournaments anyone can join at any time, no scheduling required
- Full replay archives attached to every ranked game, enabling self-study and community content
- Spectator modes that make watching almost as engaging as playing
- Open analysis tools (free computer analysis, opening explorers) that help players improve without paywalls

**What territory strategy players hate:**
- Lopsided matchmaking where one side is clearly stronger before the game begins
- Slow turns and excessive downtime waiting for opponents
- Being forced to commit to a single strategy too early with no room to adapt
- Steep skill cliffs that punish new players without teaching them anything
- Opaque balance -- not understanding why you lost or what you could have done differently

---

## Top 10 Features, Ranked by Impact

### 1. Glicko-2 Ranked Ladder with Visible Tier Ranks

**What it is:** Implement a Glicko-2 rating system with rating deviation (RD) tracking. Display player skill as both a numeric rating and a visible letter/icon rank (e.g., Bronze through Grandmaster) based on percentile in the active player pool.

**Why it matters:** A credible, transparent rating system is the backbone of any competitive game. Glicko-2 handles inactivity gracefully (RD increases, so returning players don't distort matchmaking), tracks rating confidence, and produces fairer matches than Elo. Visible tier ranks give players identity, bragging rights, and short-term climb goals that raw numbers alone cannot provide. Every top competitive platform -- from chess servers to puzzle fighters -- uses some variant of this. Without it, strat.club has no competitive credibility.

**Priority: MUST-HAVE**

---

### 2. Sub-5-Second Re-Queue and Instant Rematch

**What it is:** After a game ends, players can rematch the same opponent with one click or re-enter the matchmaking queue without returning to a lobby. The post-game screen shows the result, rating change, and a prominent "Play Again" button. Queue-to-game transition should take under 5 seconds at populated rating bands.

**Why it matters:** The single biggest killer of competitive session length is friction between games. TETR.IO's Quick Play lets eliminated players start a new run immediately without waiting -- and it is one of the most praised features in the community. Chess platforms pair you with a new opponent within seconds of your last move. Every second of dead time is a chance for the player to open another tab. For a 1v1 game, this is existential.

**Priority: MUST-HAVE**

---

### 3. Turn Timer with Adaptive Pace Controls

**What it is:** Configurable turn timers (e.g., 15s / 30s / 60s per turn, or a chess-clock bank system where each player gets a total time pool plus per-turn increment). Ranked queue lets players select their preferred pace. Animations and transitions are tuned to never exceed 1 second so the game feels snappy even in longer time controls.

**Why it matters:** The number-one complaint in turn-based strategy is waiting. Slow opponents kill engagement. A chess-clock model solves this elegantly: it respects thoughtful play while guaranteeing the game ends in bounded time. Offering pace variants (blitz, standard, classical) also segments the player base by preference rather than forcing everyone into one speed, exactly as chess platforms do with bullet/blitz/rapid/classical queues.

**Priority: MUST-HAVE**

---

### 4. Post-Game Analysis and Replay System

**What it is:** Every ranked game is recorded and available as a replayable, step-by-step review. The post-game screen highlights key turning points ("You lost territory advantage on turn 12"), shows a territory-control graph over time, and flags critical mistakes. Replays are shareable via link.

**Why it matters:** Players who understand why they lost stay engaged. Players who cannot figure out why they lost quit. Lichess offers free unlimited computer analysis on every game and it is consistently cited as the feature that converts casual players into dedicated improvers. For a territory strategy game, a territory-over-time graph and decision-point highlights are the equivalent of a chess engine evaluation bar. This also enables community content: streamers, guides, and coaching all depend on replays.

**Priority: MUST-HAVE**

---

### 5. Daily Challenge / Puzzle Mode with Streaks

**What it is:** Each day, present players with a short territory puzzle -- a board state where they must find the optimal sequence of moves to capture or defend a position. Track consecutive-day streaks. Award cosmetic rewards at streak milestones (7 days, 30 days, 100 days).

**Why it matters:** Chess.com's daily puzzle streak is one of the most effective retention mechanics in competitive gaming. It creates a low-commitment daily habit (2-3 minutes) that keeps players opening the app even on days they do not have time for a full game. Streaks tap into loss aversion -- players do not want to break a 30-day streak. The puzzles also serve as skill training, making players better and more confident in ranked play. Games utilizing daily engagement loops see a 30% uptick in weekly active retention.

**Priority: SHOULD-HAVE**

---

### 6. Arena Tournaments (Drop-in, Time-Boxed)

**What it is:** Scheduled or always-running tournaments where players can join and leave at any time during the tournament window (e.g., a 1-hour arena). Players are paired continuously -- finish a game, immediately get a new opponent. Points are earned per win with streaks granting bonus points. Leaderboard is live.

**Why it matters:** Lichess arena tournaments are its most popular feature precisely because they require zero commitment to enter. You show up, play as many or as few games as you want, and see where you land. This is far superior to bracket tournaments that require scheduling and punish no-shows. For a 1v1 game with short match times, arena format is a perfect fit. It also creates exciting spectator moments as top players jockey for position on the live leaderboard.

**Priority: SHOULD-HAVE**

---

### 7. Game Feel and "Juice" -- Animation, Audio, and Feedback Polish

**What it is:** Add satisfying visual and audio feedback to every meaningful game action: territory captures trigger a ripple/pulse effect, losing territory shows a visible crack or fade, turn submission has a tactile "stamp" feel, and rating changes after a match are animated with tension (number ticking up or down). Use easing functions on all animations. Ensure every interaction has sub-100ms visual response.

**Why it matters:** "Juice" -- the small effects that make a game feel alive -- is the difference between a prototype and a product. Research shows that responsive, well-animated games retain players significantly better even with identical mechanics. In a strategy game where individual actions are infrequent (compared to an action game), each action must feel weighty and consequential. The territory map is a canvas for this: watching your color spread across the board should feel triumphant, and watching it recede should feel urgent. This is not cosmetic polish -- it is core to the emotional loop of competition.

**Priority: SHOULD-HAVE**

---

### 8. Spectator Mode with Live Game Browser

**What it is:** A "Watch" tab where players can browse ongoing ranked games, sorted by player rating or game intensity. Spectators see both players' perspectives (or a neutral view). Top-rated games are featured. Optionally allow spectator chat.

**Why it matters:** Spectating serves three purposes: it lets waiting players stay engaged, it lets weaker players learn from stronger ones, and it builds community identity around top players. TETR.IO's spectator mode with dynamic audio has been praised for making watching feel nearly as engaging as playing. For strat.club, spectating high-level territory battles would showcase the strategic depth of the game and serve as organic marketing -- shareable moments that draw in new players.

**Priority: SHOULD-HAVE**

---

### 9. Seasonal Resets and Progression Tracks

**What it is:** Divide the competitive year into seasons (e.g., 3-month seasons). At the start of each season, perform a soft rating reset (compress ratings toward the mean, preserving relative order). Each season has a progression track with cosmetic rewards (borders, map skins, profile badges) earned by playing ranked games. End-of-season rewards based on peak rank achieved.

**Why it matters:** Seasons solve two critical problems. First, they give lapsed players a natural re-entry point -- "new season, fresh start." Second, they create urgency and goal-setting: players push to hit a target rank before the season ends. Chess.com's monthly Puzzle Battle leaderboard resets and TETR.IO's seasonal rank adjustments both use this pattern. Limited-time seasonal rewards drive a 23% engagement increase during promotional periods. For long-term health, seasons prevent rating stagnation where veteran players sit at a number that never changes.

**Priority: NICE-TO-HAVE**

---

### 10. New Player Onboarding with Guided Matches and Skill Calibration

**What it is:** First-time players go through 3-5 guided tutorial matches against AI opponents that teach core territory strategy concepts (expansion, defense, chokepoints). This is followed by 10 placement matches against real opponents to calibrate their initial Glicko-2 rating. During placement, matchmaking is wider but weighted toward other unrated players.

**Why it matters:** The steepest drop-off in competitive games happens in the first 3 games. If a new player gets crushed by a veteran because the system does not know their skill level, they leave and never return. A calibration period with high initial RD (Glicko-2's built-in mechanism for uncertainty) combined with guided introductory content ensures new players understand the game before being thrown into the deep end. This protects both new players (from frustration) and existing players (from lopsided matches). Reducing the skill cliff is the single most important thing for growing the player base beyond the hardcore early adopters.

**Priority: NICE-TO-HAVE** (becomes MUST-HAVE before any significant marketing push)

---

## Summary Table

| Rank | Feature | Priority |
|------|---------|----------|
| 1 | Glicko-2 Ranked Ladder with Visible Tiers | Must-have |
| 2 | Sub-5-Second Re-Queue and Instant Rematch | Must-have |
| 3 | Turn Timer with Adaptive Pace Controls | Must-have |
| 4 | Post-Game Analysis and Replay System | Must-have |
| 5 | Daily Challenge / Puzzle Mode with Streaks | Should-have |
| 6 | Arena Tournaments (Drop-in, Time-Boxed) | Should-have |
| 7 | Game Feel and "Juice" Polish | Should-have |
| 8 | Spectator Mode with Live Game Browser | Should-have |
| 9 | Seasonal Resets and Progression Tracks | Nice-to-have |
| 10 | New Player Onboarding and Skill Calibration | Nice-to-have |

---

## Implementation Note

The four must-haves (ranked ladder, fast re-queue, turn timers, replay system) form the competitive core. Without all four, strat.club is a toy. With all four, it is a platform. The should-haves (daily puzzles, arena tournaments, juice, spectating) are what turn a platform into a habit. The nice-to-haves (seasons, onboarding) are what turn a habit into growth.

Build the core first. Then make it sticky. Then make it grow.
