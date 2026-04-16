# strat.club Game Design

This document explains the design decisions behind strat.club's game mechanics, AI, rating system, and planned future directions.

---

## Why Deterministic Combat (0% Luck)

Most territory strategy games use dice rolls or random number generation to resolve combat. strat.club does not. Every attack produces an exact, predictable result.

### How it works

- **Offense kill rate: 60%** -- each attacking army has a 0.6 probability of killing one defender.
- **Defense kill rate: 70%** -- each defending army has a 0.7 probability of killing one attacker.
- **At 0% luck, these probabilities become deterministic:** kills = round(armies * rate), where 0.5 rounds up.

This means:
- 2 attackers vs 1 defender: 2 * 0.6 = 1.2, rounds to 1 kill. Defender: 1 * 0.7 = 0.7, rounds to 1 kill. Attacker captures with 1 survivor.
- 3 vs 2: attackers kill 2 (3 * 0.6 = 1.8 rounds to 2), defenders kill 1 (2 * 0.7 = 1.4 rounds to 1). Capture with 2 survivors.
- 5 vs 3: attackers kill 3 (5 * 0.6 = 3.0), defenders kill 2 (3 * 0.7 = 2.1 rounds to 2). Capture with 3 survivors.

The general rule: you need roughly 1.7x the defenders to guarantee a capture.

### Why this matters

**Luck-based combat punishes correct play.** When a player makes the right strategic decision but loses because of bad dice, they learn nothing. Worse, they may learn the wrong lesson ("I shouldn't have attacked there") when the attack was mathematically sound.

**Deterministic combat rewards calculation.** Every player can count the exact armies needed to take a territory. This turns the game from a gamble into a planning exercise, similar to chess -- you can see several moves ahead because the board state after each move is fully predictable.

**It raises the skill ceiling.** When outcomes are guaranteed, the difference between a good player and a great player is not who gets luckier but who plans further ahead, manages their army economy better, and reads the fog more accurately.

**It creates more meaningful spectating.** Observers can verify whether a player made the optimal move because the results are calculable. There is no "they just got lucky" narrative -- only "they outplayed."

### The tradeoff

Deterministic combat can feel unforgiving. A 2v1 always captures, so a single misplaced army can cascade into a lost position. This is intentional: strat.club is a competitive game where mistakes should cost you, and where paying attention to the exact numbers is the core skill.

---

## Why ABBA Snake Draft for Picks

The picking phase uses Random Warlords with an ABBA snake draft order: A, B, B, A, A, B, B, A, ...

### The first-pick advantage problem

In a simple alternating draft (A, B, A, B), the first player always gets the single best available territory. This creates a measurable first-pick advantage.

### How ABBA fixes it

The snake pattern gives player B two consecutive picks after A's first pick, then A gets two, and so on. This means:
- A gets pick 1 (the best territory).
- B gets picks 2 and 3 (the second and third best).
- A gets pick 4 (the fourth best).

Across 4 picks each on a balanced map, A gets picks 1, 4, 5, 8 and B gets picks 2, 3, 6, 7. The total "pick quality" is roughly equal.

### Why Random Warlords

Instead of letting players pick from the entire map, one territory per bonus (continent) is offered at random. This serves two purposes:

1. **Prevents memorized openings.** If the same territories were always available, optimal pick orders would become scripted. Random offerings ensure every game starts differently.
2. **Guarantees geographic spread.** Since exactly one territory per bonus is offered, players cannot stack picks in a single region. Every player starts with a presence across the map, creating multiple fronts from turn one.

---

## How Bonuses Work

Bonuses (continents) are the primary income engine and the central strategic objective.

### Mechanics

- Each bonus is a named group of territories (e.g., "North America" with 9 territories).
- Control all territories in a bonus to earn its value in extra armies each turn, on top of the base income of 5.
- Partial control of a bonus gives nothing -- it is all or nothing.

### Why this creates interesting decisions

- **Small bonuses are easier to complete but worth less.** A 3-territory bonus worth 2 income is easier to secure than a 6-territory bonus worth 5, but you get less return. Deciding which bonus to pursue is the first strategic decision of the game.
- **Holding a bonus means defending its borders.** Every bonus has territories adjacent to the outside. The more border territories, the harder (and more expensive) it is to defend. This creates a natural tension between expansion and consolidation.
- **Denying enemy bonuses is as important as completing your own.** If your opponent is one territory away from completing a high-value bonus, capturing or holding that single territory can be worth more than pursuing your own bonus.

### AI bonus evaluation

The medium AI scores bonuses using a weighted formula considering:
- **Completion percentage** -- how close the AI is to owning all territories.
- **Efficiency** -- bonus value divided by territory count (income per territory).
- **Affordability** -- cost to capture remaining territories relative to current income.
- **Contestation** -- penalty if the opponent also has presence in the bonus.

This allows the AI to dynamically choose which bonus to pursue each turn based on the evolving board state.

---

## How Fog of War Creates Strategic Depth

With fog of war enabled (the default), each player can only see:
- Territories they own.
- Territories adjacent to territories they own.

Everything else is hidden. You do not know the opponent's army counts, deployments, or movements in distant parts of the map.

### What fog adds to the game

**Information becomes a resource.** Expanding toward the opponent reveals their position, but also exposes your forces. A player with more territory sees more of the map, creating a natural advantage for the leading player -- but also giving the trailing player the advantage of hidden information.

**Defensive positioning matters.** Without fog, you can perfectly counter every attack because you see it coming. With fog, you must predict where the opponent will strike and pre-position armies accordingly. This transforms defense from a reactive calculation into a strategic prediction.

**Feints and misdirection become possible.** Building up armies on a visible border can deter an opponent from attacking that front, even if you plan to attack somewhere else entirely. The opponent must decide: is this a real threat or a bluff?

**It rewards map reading.** Experienced players learn to infer enemy positions from the territories they can see. If a neutral territory near the opponent's likely position has been captured, it suggests the opponent is expanding in that direction. This deductive reasoning is one of the deepest skills in the game.

### Implementation

The fog system filters all game state before sending it to a player. Non-visible territories appear as neutral with default army counts. Turn events (attacks, deployments) are filtered so players only see events involving territories they could observe before or after the turn resolved. Even card information is hidden -- you cannot see how many cards or card pieces your opponent has.

---

## AI Difficulty Levels

strat.club includes three AI difficulty levels, each using a fundamentally different approach.

### Easy (Random)

- Deploys all income on a single random territory.
- Attacks one random adjacent enemy if possible.
- No strategic planning whatsoever.

This exists for brand-new players who need to learn the interface and basic mechanics without pressure. The easy AI will make obviously terrible moves and can be beaten by anyone who understands the rules.

### Medium (Greedy Heuristic)

A multi-step planning AI that evaluates the board and makes locally optimal decisions each turn.

Key capabilities:
- **Bonus-completion targeting.** Scores all bonuses by completion percentage, efficiency, and affordability. Prioritizes the most achievable high-value bonus.
- **Counter-expansion.** Detects when the opponent is close to completing a bonus and attacks the key territory to deny it.
- **Situational deployment.** Stacks armies on one front when behind (breakthrough strategy), spreads across borders when dominant (defensive strategy).
- **Multi-step attack chains.** Plans sequences of attacks that capture multiple territories using the surviving attackers from each battle.
- **BFS-based transfers.** Moves interior armies toward the most threatened border using breadth-first search pathfinding.
- **Opening-specific logic.** In the first 3 turns, focuses exclusively on completing the nearest bonus rather than taking opportunistic captures.
- **Endgame cleanup.** When owning more than 60% of the map, switches to a spread strategy that attacks from every border simultaneously.

The medium AI is a competent opponent that understands bonus control, army economy, and positional play. It makes the correct move most of the time but cannot look ahead beyond the current turn.

### Hard (MCTS)

Monte Carlo Tree Search with the following configuration:
- **Time budget: 500ms** per turn.
- **UCB1 selection** with exploration constant of 1.41 (approximately the square root of 2).
- **Rollout depth: 10 turns** maximum.
- **Greedy rollouts.** During the simulation phase, both players use the medium AI's greedy heuristic to generate plausible moves (rather than random moves). This produces more realistic game trajectories.
- **Heuristic board evaluation** at leaf nodes, using a weighted score based on territory count, income, army count, and bonus control.

MCTS explores a tree of possible future game states, spending more time analyzing promising lines of play. Because it can effectively "look ahead" multiple turns, it can find strategies the greedy AI misses -- like sacrificing a territory now to set up a bonus capture two turns later.

The hard AI plays at a strong level and is challenging for intermediate players. Expert players will eventually learn its patterns and find exploits, but it provides a meaningful training opponent for developing strategic skills.

---

## Rating System: Glicko-2

strat.club uses the Glicko-2 rating system (Mark Glickman, 2013) for competitive play.

### Why Glicko-2 over Elo

**Rating deviation (RD).** Every player has not just a rating but an RD value that represents how uncertain the system is about their true skill. A new player has an RD of 350 (very uncertain); an active player might have an RD of 50 (very confident). This means:

- New players' ratings adjust quickly (because the system knows it does not have enough data yet).
- Established players' ratings adjust slowly (because the system is confident in their level).
- Returning players after a long absence have their RD increased, so the system re-calibrates them faster.

**Volatility tracking.** Glicko-2 tracks a third parameter: rating volatility, which measures how consistently a player performs. A player with erratic results (big wins followed by big losses) has higher volatility, causing their RD to increase slightly between rating periods.

**Fairer matchmaking.** Because Glicko-2 quantifies uncertainty, matchmaking can account for it. Two players with similar ratings but different RDs represent very different confidence levels. The system naturally handles smurfs (new accounts by strong players) by giving them high RD, which means large rating swings that quickly converge to their true skill.

### Default parameters

| Parameter | Default | Description |
|-----------|---------|-------------|
| Rating | 1500 | Starting rating for all players |
| RD | 350 | Starting rating deviation (high uncertainty) |
| Volatility | 0.06 | Starting volatility |
| Tau | 0.5 | System constant (controls volatility change speed) |

### Upset bonus

When a lower-rated player beats a higher-rated player, the rating gain is larger than a "routine" win. This is built into Glicko-2's expected-score formula: the expected score for the weaker player is low, so exceeding it produces a bigger adjustment. A 1200-rated player beating an 1800-rated player gains far more points than an 1800-rated player beating a 1200-rated player.

---

## Seasonal League and Rank Points

In addition to the hidden Glicko-2 rating, competitive play features a visible seasonal ranking system.

### Rank points (RP)

RP is a separate, more visible progression metric that resets each season:

- **Base gain: +25 RP** for a win.
- **Streak bonus: +5 RP per win streak** (capped at +25 for a 5+ streak).
- **Tier modifier: +/- 5 RP** based on whether the opponent is a higher or lower tier.
- **Minimum gain: +5 RP** (even in the worst case, you gain something for winning).
- **Base loss: -20 RP** for a loss.
- **Minimum loss: -5 RP** (losses are never less than 5).

### Why two systems

Glicko-2 is mathematically optimal for estimating true skill, but its outputs are unintuitive for players. A rating of 1523.4 with an RD of 67.2 is meaningful to a statistician but not to a casual player. Rank points and tier names (Bronze through Grandmaster) give players a clear, visible goal to work toward, while the hidden Glicko-2 rating drives matchmaking accuracy behind the scenes.

---

## Win Probability Engine

strat.club computes win probability using a three-layer evaluation system, each trading speed for accuracy.

### Layer 1: Material Evaluation (<1ms)

A logistic function over a weighted advantage score derived from five features:

| Feature | Weight | Description |
|---------|--------|-------------|
| Income ratio | 1.6 | ln(my_income / opponent_income) |
| Territory ratio | 0.5 | ln(my_territories / opponent_territories) |
| Army ratio | 0.4 | ln(my_armies / opponent_armies) |
| Bonus control | 0.8 | Complete bonuses valued at 1.5x, partial at quadratic fraction |
| Defensive position | 0.3 | Fraction of border territories where enemy armies exceed yours |

The logistic function is calibrated so that:
- Equal position produces approximately 50%.
- 2x income advantage produces approximately 75%.
- 3x income advantage produces approximately 90%.
- Total elimination produces exactly 0% or 100%.

This layer is used for the in-game win probability bar because it updates instantly.

### Layer 2: 1-Ply Lookahead (<50ms)

Generates the best orders for both players using the greedy AI, then simulates the next turn twice (once with each possible move order, since simultaneous resolution means one player's attacks resolve first). Averages the resulting material evaluations.

This produces a more stable estimate because it accounts for the immediate consequences of each side's best plan. Used for the win probability chart after each turn.

### Layer 3: Full Monte Carlo (<500ms)

Runs 200 AI-vs-AI simulations (up to 30 turns each), evaluating the final position with the material evaluation function rather than a simple win/loss count. This produces smoother, more accurate estimates than counting discrete outcomes.

To ensure simulation diversity, 80% of moves use the greedy AI and 20% use a randomly perturbed deployment with greedy attacks. This prevents all simulations from following the same path.

Used for the deep analysis endpoint (`/api/game/analysis`).

---

## Future Combat System Ideas

The current combat system is simple and works well for competitive play, but there are several directions under consideration for future maps or game modes.

### Diminishing Returns on Large Attacks

Currently, attacking with 100 armies vs 1 defender kills the defender and costs exactly 1 attacker, with 99 surviving. This makes large army stacks extremely efficient -- there is no incentive to spread out.

A diminishing-returns model could scale the offense kill rate down as the attacker-to-defender ratio increases. For example, the first 10 attackers kill at 60%, but additional attackers beyond 2x the defenders kill at only 30%. This would reward splitting attacks across multiple fronts rather than concentrating everything on one territory.

### Terrain Modifiers

Different territory types could modify the offense/defense kill rates:
- **Mountains:** defense kill rate increased to 80% (hard to attack).
- **Plains:** offense kill rate increased to 70% (easy to cross).
- **Fortified cities:** attackers need 2.5x defenders instead of 1.7x.

This would add a map-reading layer where players must consider not just adjacency but terrain when planning attacks.

### Supply Lines

Territories far from a player's "core" (connected to the bulk of their territory through a chain of owned territories) could fight at reduced effectiveness. This would discourage overextension and reward building a contiguous empire rather than taking isolated outposts.

### Multi-Stage Battles

Instead of resolving in a single round, large battles could play out over 2-3 rounds of combat within the same turn. After each round, the attacker could choose to retreat or continue. This would add a new decision point without introducing randomness.

---

## Design Principles

These principles guide all game design decisions in strat.club:

1. **Skill over luck.** Every mechanic should reward good decisions and punish bad ones. If a player loses, it should always be because of a mistake they can identify and learn from.

2. **Readable board states.** A player should be able to look at the board and understand the position. Fog of war creates hidden information, but visible information should be unambiguous.

3. **Fast games, deep strategy.** Games should take 10-20 minutes, not 2 hours. Depth comes from fog of war, bonus management, and multi-turn planning -- not from map size or turn count.

4. **Fairness from the first move.** The pick system, map design, and combat rules are all calibrated so that both players have an equal chance at the start. No first-mover advantage, no map-knowledge gatekeeping.

5. **Transparent systems.** Combat formulas, rating algorithms, and matchmaking logic are documented and open-source. Players should never wonder "how does this work" -- they should be able to read the code.
