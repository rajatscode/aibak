//! Daily puzzle generation and solution checking (capture-the-bonus, defend-or-die, multi-front).

use serde::{Deserialize, Serialize};

use crate::board::Board;
use crate::combat::resolve_attack;
use crate::map::{Bonus, MapFile, MapSettings, PickingConfig, PickingMethod, Territory};
use crate::orders::Order;
use crate::state::{GameState, Phase, PlayerId};

/// Difficulty level for a daily puzzle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PuzzleDifficulty {
    Easy,
    Medium,
    Hard,
}

/// The type of puzzle challenge.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PuzzleType {
    /// Deploy and attack to complete a bonus this turn.
    CaptureTheBonus,
    /// Place armies optimally to survive the opponent's next attack.
    DefendOrDie,
    /// Attack on 2 fronts simultaneously with limited armies.
    MultiFront,
}

/// A daily puzzle: a pre-set game state where the player must find the winning move.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Puzzle {
    pub id: u32,
    pub state: GameState,
    pub board: Board,
    pub description: String,
    pub hint: String,
    pub optimal_orders: Vec<Order>,
    pub difficulty: PuzzleDifficulty,
    pub puzzle_type: PuzzleType,
    /// The player whose turn it is (always 0 in puzzles).
    pub player: PlayerId,
    /// Income available to deploy.
    pub income: u32,
}

/// Result of checking a player's submitted solution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PuzzleResult {
    pub correct: bool,
    pub message: String,
    pub optimal_orders: Vec<Order>,
    /// Did the player's solution achieve the objective?
    pub objective_met: bool,
}

/// Simple seeded pseudo-random number generator (deterministic, no external deps beyond seed).
struct SimpleRng {
    state: u64,
}

impl SimpleRng {
    fn new(seed: u32) -> Self {
        // Mix the seed to avoid correlated sequences for adjacent day numbers.
        let s = (seed as u64)
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        Self { state: s }
    }

    fn next_u32(&mut self) -> u32 {
        self.state = self
            .state
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        ((self.state >> 33) ^ (self.state >> 17)) as u32
    }

    fn next_range(&mut self, min: u32, max: u32) -> u32 {
        if min >= max {
            return min;
        }
        min + (self.next_u32() % (max - min))
    }

    #[allow(dead_code)]
    fn pick<T: Clone>(&mut self, items: &[T]) -> T {
        let idx = self.next_u32() as usize % items.len();
        items[idx].clone()
    }
}

/// Generate a deterministic puzzle for a given day (seed = day number).
/// Everyone gets the same puzzle on the same day.
pub fn daily_puzzle(day_seed: u32) -> Puzzle {
    let mut rng = SimpleRng::new(day_seed);

    // Cycle through puzzle types based on day.
    let puzzle_type = match day_seed % 3 {
        0 => PuzzleType::CaptureTheBonus,
        1 => PuzzleType::DefendOrDie,
        _ => PuzzleType::MultiFront,
    };

    // Difficulty cycles on a weekly basis: Easy Mon-Tue, Medium Wed-Thu, Hard Fri-Sun.
    let difficulty = match day_seed % 7 {
        0 | 1 => PuzzleDifficulty::Easy,
        2 | 3 => PuzzleDifficulty::Medium,
        _ => PuzzleDifficulty::Hard,
    };

    match puzzle_type {
        PuzzleType::CaptureTheBonus => generate_capture_bonus(day_seed, difficulty, &mut rng),
        PuzzleType::DefendOrDie => generate_defend_or_die(day_seed, difficulty, &mut rng),
        PuzzleType::MultiFront => generate_multi_front(day_seed, difficulty, &mut rng),
    }
}

fn default_settings() -> MapSettings {
    MapSettings {
        luck_pct: 0,
        base_income: 5,
        wasteland_armies: 10,
        unpicked_neutral_armies: 4,
        fog_of_war: false,
        offense_kill_rate: 0.6,
        defense_kill_rate: 0.7,
    }
}

#[allow(clippy::needless_range_loop)]
/// Generate a "Capture the Bonus" puzzle.
///
/// Layout: Player owns some territories of a bonus but not all.
/// Must deploy + attack to capture the remaining territory and complete the bonus.
fn generate_capture_bonus(id: u32, difficulty: PuzzleDifficulty, rng: &mut SimpleRng) -> Puzzle {
    // Map size scales with difficulty.
    let (bonus_size, extra_territories) = match difficulty {
        PuzzleDifficulty::Easy => (3, 2),
        PuzzleDifficulty::Medium => (3, 4),
        PuzzleDifficulty::Hard => (4, 4),
    };

    let total = bonus_size + extra_territories;

    // Build territory names.
    let names: Vec<String> = (0..total)
        .map(|i| {
            let region_names = [
                "Northwall",
                "Ironvale",
                "Dustmere",
                "Thornkeep",
                "Greymoor",
                "Ashford",
                "Stonehelm",
                "Blackrun",
                "Frostpeak",
                "Willowgate",
                "Ravencrest",
                "Dawnhollow",
            ];
            let idx = ((id as usize).wrapping_mul(7).wrapping_add(i)) % region_names.len();
            region_names[idx].to_string()
        })
        .collect();

    // Build adjacency: bonus territories form a chain, extras connect around them.
    let mut territories = Vec::new();
    for i in 0..total {
        let mut adj = Vec::new();
        if i > 0 {
            adj.push(i - 1);
        }
        if i + 1 < total {
            adj.push(i + 1);
        }
        // Add cross-links for non-trivial graph.
        if i == 0 && total > 2 {
            adj.push(2);
        }
        if i == total - 1 && i >= 3 {
            adj.push(i - 2);
        }
        adj.sort();
        adj.dedup();

        territories.push(Territory {
            id: i,
            name: names[i].clone(),
            bonus_id: if i < bonus_size { 0 } else { 1 },
            is_wasteland: false,
            default_armies: 2,
            adjacent: adj,
            visual: None,
        });
    }

    // Make adjacency symmetric.
    let mut adj_lists: Vec<Vec<usize>> = territories.iter().map(|t| t.adjacent.clone()).collect();
    for i in 0..total {
        let neighbors = adj_lists[i].clone();
        for &j in &neighbors {
            if !adj_lists[j].contains(&i) {
                adj_lists[j].push(i);
            }
        }
    }
    for i in 0..total {
        adj_lists[i].sort();
        adj_lists[i].dedup();
        territories[i].adjacent = adj_lists[i].clone();
    }

    let bonus_territory_ids: Vec<usize> = (0..bonus_size).collect();
    let bonus_value = match difficulty {
        PuzzleDifficulty::Easy => 3,
        PuzzleDifficulty::Medium => 4,
        PuzzleDifficulty::Hard => 5,
    };

    let bonuses = vec![
        Bonus {
            id: 0,
            name: "Target Bonus".into(),
            value: bonus_value,
            territory_ids: bonus_territory_ids,
            visual: None,
        },
        Bonus {
            id: 1,
            name: "Outer Region".into(),
            value: 2,
            territory_ids: (bonus_size..total).collect(),
            visual: None,
        },
    ];

    let map = MapFile {
        id: format!("puzzle_{}", id),
        name: format!("Puzzle #{}", id),
        territories,
        bonuses,
        picking: PickingConfig {
            num_picks: 1,
            method: PickingMethod::RandomWarlords,
        },
        settings: default_settings(),
    };
    let board = Board::from_map(map);

    // Set up game state: player owns all bonus territories except one.
    let mut state = GameState::new(&board);
    state.phase = Phase::Play;
    state.turn = 1;

    // Player 0 owns all bonus territories except the last one.
    let target_territory = bonus_size - 1;
    for i in 0..bonus_size - 1 {
        state.territory_owners[i] = 0;
        state.territory_armies[i] = rng.next_range(3, 6);
    }
    // The target territory is owned by the opponent.
    state.territory_owners[target_territory] = 1;
    let target_defenders = match difficulty {
        PuzzleDifficulty::Easy => 2,
        PuzzleDifficulty::Medium => rng.next_range(3, 5),
        PuzzleDifficulty::Hard => rng.next_range(4, 7),
    };
    state.territory_armies[target_territory] = target_defenders;

    // Opponent owns the extra territories.
    for i in bonus_size..total {
        state.territory_owners[i] = 1;
        state.territory_armies[i] = rng.next_range(2, 5);
    }

    // Calculate the income player 0 gets.
    let income = state.income(0, &board);

    // Find the optimal deploy territory: the one adjacent to the target.
    let deploy_territory = (0..bonus_size - 1)
        .find(|&i| board.map.are_adjacent(i, target_territory))
        .unwrap_or(0);

    // Calculate required attackers: need to kill all defenders.
    // offense kills = round(attackers * 0.6) >= target_defenders
    // defense kills = round(defenders * 0.7) <= attackers (need survivors)
    // We deploy all income to the deploy territory, then attack with everything.
    let existing_armies = state.territory_armies[deploy_territory];
    let total_attackable = existing_armies + income - 1; // leave 1 behind

    let optimal_orders = vec![
        Order::Deploy {
            territory: deploy_territory,
            armies: income,
        },
        Order::Attack {
            from: deploy_territory,
            to: target_territory,
            armies: total_attackable,
        },
    ];

    // Verify the attack actually works.
    let result = resolve_attack(total_attackable, target_defenders, board.settings());

    // If it doesn't capture, adjust defenders down so it does.
    let final_defenders = if !result.captured {
        // Find max defenders we can beat with total_attackable.
        let mut d = target_defenders;
        while d > 1 {
            d -= 1;
            let r = resolve_attack(total_attackable, d, board.settings());
            if r.captured {
                break;
            }
        }
        state.territory_armies[target_territory] = d;
        d
    } else {
        target_defenders
    };

    let description = format!(
        "Capture {} to complete the {} bonus (+{} income). You have {} armies to deploy.",
        board.map.territories[target_territory].name, board.map.bonuses[0].name, bonus_value, income
    );

    let hint = format!(
        "Deploy all {} armies to {} and attack {} with maximum force. \
         You need to overwhelm {} defenders.",
        income,
        board.map.territories[deploy_territory].name,
        board.map.territories[target_territory].name,
        final_defenders
    );

    Puzzle {
        id,
        state,
        board,
        description,
        hint,
        optimal_orders,
        difficulty,
        puzzle_type: PuzzleType::CaptureTheBonus,
        player: 0,
        income,
    }
}

/// Generate a "Defend or Die" puzzle.
#[allow(clippy::needless_range_loop)]
///
/// The opponent has a large army threatening the player's key territory.
/// Player must deploy optimally to survive the attack.
fn generate_defend_or_die(id: u32, difficulty: PuzzleDifficulty, rng: &mut SimpleRng) -> Puzzle {
    let total = match difficulty {
        PuzzleDifficulty::Easy => 6,
        PuzzleDifficulty::Medium => 7,
        PuzzleDifficulty::Hard => 8,
    };

    let names: Vec<String> = (0..total)
        .map(|i| {
            let region_names = [
                "Ember Keep",
                "Shadowfen",
                "Crystalridge",
                "Duskwood",
                "Galehaven",
                "Ironmarch",
                "Ravenspire",
                "Sunhollow",
                "Mistfall",
                "Boneyard",
                "Goldcrest",
                "Dragonmaw",
            ];
            let idx = ((id as usize).wrapping_mul(11).wrapping_add(i * 3)) % region_names.len();
            region_names[idx].to_string()
        })
        .collect();

    // Build a map: player's 3 territories on left, opponent's territories on right.
    // The key territory (id=2) is the border territory that the opponent will attack.
    let player_count = 3;

    let mut territories = Vec::new();
    for i in 0..total {
        let mut adj = Vec::new();
        if i > 0 {
            adj.push(i - 1);
        }
        if i + 1 < total {
            adj.push(i + 1);
        }
        // Cross-links.
        if i == 0 && total > 3 {
            adj.push(2);
        }
        if i == player_count - 1 && i + 1 < total {
            adj.push(i + 1);
        }
        if i >= player_count && i + 2 < total {
            adj.push(i + 2);
        }
        adj.sort();
        adj.dedup();

        territories.push(Territory {
            id: i,
            name: names[i].clone(),
            bonus_id: if i < player_count { 0 } else { 1 },
            is_wasteland: false,
            default_armies: 2,
            adjacent: adj,
            visual: None,
        });
    }

    // Symmetrize adjacency.
    let mut adj_lists: Vec<Vec<usize>> = territories.iter().map(|t| t.adjacent.clone()).collect();
    for i in 0..total {
        let neighbors = adj_lists[i].clone();
        for &j in &neighbors {
            if !adj_lists[j].contains(&i) {
                adj_lists[j].push(i);
            }
        }
    }
    for i in 0..total {
        adj_lists[i].sort();
        adj_lists[i].dedup();
        territories[i].adjacent = adj_lists[i].clone();
    }

    let bonuses = vec![
        Bonus {
            id: 0,
            name: "Homeland".into(),
            value: 3,
            territory_ids: (0..player_count).collect(),
            visual: None,
        },
        Bonus {
            id: 1,
            name: "Enemy Lands".into(),
            value: 3,
            territory_ids: (player_count..total).collect(),
            visual: None,
        },
    ];

    let map = MapFile {
        id: format!("puzzle_{}", id),
        name: format!("Puzzle #{}", id),
        territories,
        bonuses,
        picking: PickingConfig {
            num_picks: 1,
            method: PickingMethod::RandomWarlords,
        },
        settings: default_settings(),
    };
    let board = Board::from_map(map);

    let mut state = GameState::new(&board);
    state.phase = Phase::Play;
    state.turn = 1;

    // Player owns first 3 territories.
    for i in 0..player_count {
        state.territory_owners[i] = 0;
        state.territory_armies[i] = if i == player_count - 1 { 1 } else { 2 };
    }

    // Enemy owns the rest.
    let enemy_attack_territory = player_count; // territory just across the border
    let enemy_attack_strength = match difficulty {
        PuzzleDifficulty::Easy => rng.next_range(6, 9),
        PuzzleDifficulty::Medium => rng.next_range(9, 13),
        PuzzleDifficulty::Hard => rng.next_range(13, 18),
    };

    for i in player_count..total {
        state.territory_owners[i] = 1;
        state.territory_armies[i] = if i == enemy_attack_territory {
            enemy_attack_strength
        } else {
            rng.next_range(2, 4)
        };
    }

    let income = state.income(0, &board);
    let border_territory = player_count - 1; // territory 2

    // The optimal play: deploy ALL income to the border territory to survive.
    // defense kills = round(defenders * 0.7)
    // The enemy will attack with (enemy_attack_strength - 1).
    let enemy_attackers = enemy_attack_strength - 1;
    let defenders_after_deploy = state.territory_armies[border_territory] + income;

    // Verify: can we survive?
    let result = resolve_attack(enemy_attackers, defenders_after_deploy, board.settings());
    // If we can't survive, reduce enemy strength.
    let final_enemy_strength = if result.captured {
        // Find an enemy strength where deploying all to border survives.
        let mut s = enemy_attack_strength;
        while s > 3 {
            s -= 1;
            let r = resolve_attack(s - 1, defenders_after_deploy, board.settings());
            if !r.captured {
                break;
            }
        }
        state.territory_armies[enemy_attack_territory] = s;
        s
    } else {
        enemy_attack_strength
    };

    let optimal_orders = vec![Order::Deploy {
        territory: border_territory,
        armies: income,
    }];

    let description = format!(
        "The enemy has {} armies at {} ready to strike your border at {}. \
         Deploy your {} income wisely to survive the assault.",
        final_enemy_strength,
        board.map.territories[enemy_attack_territory].name,
        board.map.territories[border_territory].name,
        income
    );

    let hint = format!(
        "Deploy all {} armies to {} to maximize your defense. \
         The enemy will attack with {} armies -- you need enough defenders to hold.",
        income,
        board.map.territories[border_territory].name,
        final_enemy_strength - 1
    );

    Puzzle {
        id,
        state,
        board,
        description,
        hint,
        optimal_orders,
        difficulty,
        puzzle_type: PuzzleType::DefendOrDie,
        player: 0,
        income,
    }
}

#[allow(clippy::needless_range_loop, clippy::vec_init_then_push)]
/// Generate a "Multi-Front" puzzle.
///
/// Player must split armies across 2 attacks to capture 2 key territories.
fn generate_multi_front(id: u32, difficulty: PuzzleDifficulty, rng: &mut SimpleRng) -> Puzzle {
    let total = match difficulty {
        PuzzleDifficulty::Easy => 7,
        PuzzleDifficulty::Medium => 8,
        PuzzleDifficulty::Hard => 9,
    };

    let names: Vec<String> = (0..total)
        .map(|i| {
            let region_names = [
                "Fort Valor",
                "The Crossing",
                "Eagle's Perch",
                "Wolf Den",
                "Dragon Gate",
                "Iron Bridge",
                "Storm Pass",
                "Hawk Ridge",
                "Bear Hollow",
                "Lion's Mane",
                "Viper Pit",
                "Falcon's Rest",
            ];
            let idx = ((id as usize).wrapping_mul(13).wrapping_add(i * 5)) % region_names.len();
            region_names[idx].to_string()
        })
        .collect();

    // Map layout:
    //   Player owns center (0) and two arms (1, 2).
    //   Targets are (3) adjacent to 1 and (4) adjacent to 2.
    //   Enemy owns 3, 4, and extras 5+.
    let mut territories = Vec::new();

    // Territory 0: center hub.
    territories.push(Territory {
        id: 0,
        name: names[0].clone(),
        bonus_id: 0,
        is_wasteland: false,
        default_armies: 2,
        adjacent: vec![1, 2],
        visual: None,
    });
    // Territory 1: left arm.
    territories.push(Territory {
        id: 1,
        name: names[1].clone(),
        bonus_id: 0,
        is_wasteland: false,
        default_armies: 2,
        adjacent: vec![0, 3],
        visual: None,
    });
    // Territory 2: right arm.
    territories.push(Territory {
        id: 2,
        name: names[2].clone(),
        bonus_id: 0,
        is_wasteland: false,
        default_armies: 2,
        adjacent: vec![0, 4],
        visual: None,
    });
    // Territory 3: left target (enemy).
    territories.push(Territory {
        id: 3,
        name: names[3].clone(),
        bonus_id: 1,
        is_wasteland: false,
        default_armies: 2,
        adjacent: vec![1, 5],
        visual: None,
    });
    // Territory 4: right target (enemy).
    territories.push(Territory {
        id: 4,
        name: names[4].clone(),
        bonus_id: 2,
        is_wasteland: false,
        default_armies: 2,
        adjacent: vec![2, 5],
        visual: None,
    });

    // Extra enemy territories.
    for i in 5..total {
        let mut adj = vec![];
        if i == 5 {
            adj.push(3);
            adj.push(4);
        }
        if i > 5 {
            adj.push(i - 1);
        }
        if i + 1 < total {
            adj.push(i + 1);
        }
        territories.push(Territory {
            id: i,
            name: names[i].clone(),
            bonus_id: 2,
            is_wasteland: false,
            default_armies: 2,
            adjacent: adj,
            visual: None,
        });
    }

    // Symmetrize adjacency.
    let mut adj_lists: Vec<Vec<usize>> = territories.iter().map(|t| t.adjacent.clone()).collect();
    for i in 0..total {
        let neighbors = adj_lists[i].clone();
        for &j in &neighbors {
            if j < total && !adj_lists[j].contains(&i) {
                adj_lists[j].push(i);
            }
        }
    }
    for i in 0..total {
        adj_lists[i].sort();
        adj_lists[i].dedup();
        territories[i].adjacent = adj_lists[i].clone();
    }

    let bonuses = vec![
        Bonus {
            id: 0,
            name: "Command Center".into(),
            value: 3,
            territory_ids: vec![0, 1, 2],
            visual: None,
        },
        Bonus {
            id: 1,
            name: "Western Front".into(),
            value: 2,
            territory_ids: vec![3],
            visual: None,
        },
        Bonus {
            id: 2,
            name: "Eastern Front".into(),
            value: 2,
            territory_ids: (4..total).collect(),
            visual: None,
        },
    ];

    let map = MapFile {
        id: format!("puzzle_{}", id),
        name: format!("Puzzle #{}", id),
        territories,
        bonuses,
        picking: PickingConfig {
            num_picks: 1,
            method: PickingMethod::RandomWarlords,
        },
        settings: default_settings(),
    };
    let board = Board::from_map(map);

    let mut state = GameState::new(&board);
    state.phase = Phase::Play;
    state.turn = 1;

    // Player owns 0, 1, 2.
    for i in 0..3 {
        state.territory_owners[i] = 0;
    }
    state.territory_armies[0] = rng.next_range(2, 4);
    state.territory_armies[1] = rng.next_range(3, 6);
    state.territory_armies[2] = rng.next_range(3, 6);

    // Enemy targets.
    let left_defenders = match difficulty {
        PuzzleDifficulty::Easy => 2,
        PuzzleDifficulty::Medium => rng.next_range(2, 4),
        PuzzleDifficulty::Hard => rng.next_range(3, 5),
    };
    let right_defenders = match difficulty {
        PuzzleDifficulty::Easy => 2,
        PuzzleDifficulty::Medium => rng.next_range(2, 4),
        PuzzleDifficulty::Hard => rng.next_range(3, 5),
    };

    state.territory_armies[3] = left_defenders;
    state.territory_armies[4] = right_defenders;

    for i in 3..total {
        state.territory_owners[i] = 1;
        if i >= 5 {
            state.territory_armies[i] = rng.next_range(2, 4);
        }
    }

    let income = state.income(0, &board);

    // Calculate optimal split: find minimum armies to capture each target.
    // For left: need attackers such that resolve_attack(attackers, left_defenders) captures.
    let left_min = min_attackers_to_capture(left_defenders, board.settings());
    let right_min = min_attackers_to_capture(right_defenders, board.settings());

    // Deploy armies to make both attacks work.
    // Left arm (territory 1) needs: left_min + 1 - current_armies
    let left_existing = state.territory_armies[1];
    let left_need = (left_min + 1).saturating_sub(left_existing);
    let right_existing = state.territory_armies[2];
    let right_need = (right_min + 1).saturating_sub(right_existing);

    // If we can't afford both, reduce defenders.
    let total_needed = left_need + right_need;
    if total_needed > income {
        // Reduce defenders to make it feasible.
        state.territory_armies[3] = 2;
        state.territory_armies[4] = 2;
    }

    // Recalculate after potential adjustment.
    let left_defenders_final = state.territory_armies[3];
    let right_defenders_final = state.territory_armies[4];
    let left_min = min_attackers_to_capture(left_defenders_final, board.settings());
    let _right_min = min_attackers_to_capture(right_defenders_final, board.settings());

    let left_existing = state.territory_armies[1];
    let right_existing = state.territory_armies[2];
    let left_deploy = (left_min + 1).saturating_sub(left_existing);
    let right_deploy = income.saturating_sub(left_deploy);

    let mut optimal_orders = Vec::new();
    if left_deploy > 0 {
        optimal_orders.push(Order::Deploy {
            territory: 1,
            armies: left_deploy,
        });
    }
    if right_deploy > 0 {
        optimal_orders.push(Order::Deploy {
            territory: 2,
            armies: right_deploy,
        });
    }
    // If no deploy needed on right, put remainder on left.
    if left_deploy + right_deploy < income {
        let remainder = income - left_deploy - right_deploy;
        if remainder > 0 {
            // Add to left deploy.
            if let Some(Order::Deploy { armies, .. }) = optimal_orders.first_mut() {
                *armies += remainder;
            }
        }
    }

    let left_attack_armies = (left_existing + left_deploy).saturating_sub(1);
    let right_attack_armies = (right_existing + right_deploy).saturating_sub(1);

    optimal_orders.push(Order::Attack {
        from: 1,
        to: 3,
        armies: left_attack_armies,
    });
    optimal_orders.push(Order::Attack {
        from: 2,
        to: 4,
        armies: right_attack_armies,
    });

    let description = format!(
        "Attack on two fronts! Capture both {} ({} defenders) and {} ({} defenders) \
         in a single turn. You have {} armies to deploy.",
        board.map.territories[3].name,
        left_defenders_final,
        board.map.territories[4].name,
        right_defenders_final,
        income
    );

    let hint = format!(
        "Split your {} deploy armies between {} and {}. \
         Each attack needs enough force to overwhelm the defenders.",
        income, board.map.territories[1].name, board.map.territories[2].name,
    );

    Puzzle {
        id,
        state,
        board,
        description,
        hint,
        optimal_orders,
        difficulty,
        puzzle_type: PuzzleType::MultiFront,
        player: 0,
        income,
    }
}

/// Find the minimum number of attackers needed to capture a territory with `defenders` armies.
fn min_attackers_to_capture(defenders: u32, settings: &MapSettings) -> u32 {
    if defenders == 0 {
        return 1;
    }
    for a in 1..=100 {
        let r = resolve_attack(a, defenders, settings);
        if r.captured {
            return a;
        }
    }
    100
}

/// Check whether a player's submitted orders achieve the puzzle objective and match the optimal solution.
pub fn check_solution(puzzle: &Puzzle, submitted_orders: &[Order]) -> PuzzleResult {
    // Simulate the submitted orders to see the outcome.
    let settings = puzzle.board.settings();

    // First, validate deploy amounts.
    let mut total_deployed = 0u32;
    for order in submitted_orders {
        if let Order::Deploy { armies, .. } = order {
            total_deployed += armies;
        }
    }
    if total_deployed > puzzle.income {
        return PuzzleResult {
            correct: false,
            message: format!(
                "You deployed {} armies but only have {} income.",
                total_deployed, puzzle.income
            ),
            optimal_orders: puzzle.optimal_orders.clone(),
            objective_met: false,
        };
    }

    // Simulate the orders on a copy of the state.
    let mut sim_state = puzzle.state.clone();

    // Apply deployments.
    for order in submitted_orders {
        if let Order::Deploy { territory, armies } = order
            && *territory < sim_state.territory_armies.len()
            && sim_state.territory_owners[*territory] == puzzle.player
        {
            sim_state.territory_armies[*territory] += armies;
        }
    }

    // Apply attacks in order.
    for order in submitted_orders {
        if let Order::Attack { from, to, armies } = order {
            if *from >= sim_state.territory_armies.len() || *to >= sim_state.territory_armies.len()
            {
                continue;
            }
            if sim_state.territory_owners[*from] != puzzle.player {
                continue;
            }
            let available = sim_state.territory_armies[*from].saturating_sub(1);
            let actual = (*armies).min(available);
            if actual == 0 {
                continue;
            }

            let defenders = sim_state.territory_armies[*to];
            let result = resolve_attack(actual, defenders, settings);

            sim_state.territory_armies[*from] -= actual;
            if result.captured {
                sim_state.territory_owners[*to] = puzzle.player;
                sim_state.territory_armies[*to] = result.surviving_attackers;
            } else {
                sim_state.territory_armies[*to] = result.surviving_defenders;
            }
        }
    }

    // Apply transfers.
    for order in submitted_orders {
        if let Order::Transfer { from, to, armies } = order {
            if *from >= sim_state.territory_armies.len() || *to >= sim_state.territory_armies.len()
            {
                continue;
            }
            if sim_state.territory_owners[*from] != puzzle.player
                || sim_state.territory_owners[*to] != puzzle.player
            {
                continue;
            }
            let available = sim_state.territory_armies[*from].saturating_sub(1);
            let actual = (*armies).min(available);
            if actual > 0 {
                sim_state.territory_armies[*from] -= actual;
                sim_state.territory_armies[*to] += actual;
            }
        }
    }

    // Check objective based on puzzle type.
    let objective_met = match puzzle.puzzle_type {
        PuzzleType::CaptureTheBonus => {
            // Check if the target bonus is complete.
            puzzle.board.map.bonuses[0]
                .territory_ids
                .iter()
                .all(|&tid| sim_state.territory_owners[tid] == puzzle.player)
        }
        PuzzleType::DefendOrDie => {
            // For defend: check if deploying to the border makes it survivable.
            // The optimal play is to deploy all to the border. The player's solution
            // should result in enough armies on the border territory.
            let border = 2; // player_count - 1
            let border_armies = sim_state.territory_armies[border];
            // The enemy territory adjacent to border.
            let enemy_territory = 3;
            let enemy_armies = sim_state.territory_armies[enemy_territory];
            let enemy_attackers = enemy_armies.saturating_sub(1);
            if enemy_attackers == 0 {
                true
            } else {
                let result = resolve_attack(enemy_attackers, border_armies, settings);
                !result.captured
            }
        }
        PuzzleType::MultiFront => {
            // Check if both targets (3 and 4) are captured.
            sim_state.territory_owners[3] == puzzle.player
                && sim_state.territory_owners[4] == puzzle.player
        }
    };

    // Check if the solution matches the optimal one (order-independent comparison).
    let orders_match = orders_equivalent(submitted_orders, &puzzle.optimal_orders);

    if objective_met && orders_match {
        PuzzleResult {
            correct: true,
            message: "Perfect! You found the optimal solution.".into(),
            optimal_orders: puzzle.optimal_orders.clone(),
            objective_met: true,
        }
    } else if objective_met {
        PuzzleResult {
            correct: true,
            message: "You completed the objective! Though there may be a more efficient solution."
                .into(),
            optimal_orders: puzzle.optimal_orders.clone(),
            objective_met: true,
        }
    } else {
        PuzzleResult {
            correct: false,
            message: "Your moves didn't achieve the objective. Try again!".into(),
            optimal_orders: puzzle.optimal_orders.clone(),
            objective_met: false,
        }
    }
}

/// Check if two sets of orders are functionally equivalent (same deploys and attacks, any order).
fn orders_equivalent(a: &[Order], b: &[Order]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut b_matched = vec![false; b.len()];
    for order_a in a {
        let mut found = false;
        for (i, order_b) in b.iter().enumerate() {
            if !b_matched[i] && order_a == order_b {
                b_matched[i] = true;
                found = true;
                break;
            }
        }
        if !found {
            return false;
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_daily_puzzle_deterministic() {
        let p1 = daily_puzzle(42);
        let p2 = daily_puzzle(42);
        assert_eq!(p1.id, p2.id);
        assert_eq!(p1.description, p2.description);
        assert_eq!(p1.optimal_orders, p2.optimal_orders);
    }

    #[test]
    fn test_daily_puzzle_different_days() {
        let p1 = daily_puzzle(1);
        let p2 = daily_puzzle(2);
        assert_ne!(p1.id, p2.id);
    }

    #[test]
    fn test_puzzle_types_cycle() {
        let p0 = daily_puzzle(0);
        let p1 = daily_puzzle(1);
        let p2 = daily_puzzle(2);
        assert_eq!(p0.puzzle_type, PuzzleType::CaptureTheBonus);
        assert_eq!(p1.puzzle_type, PuzzleType::DefendOrDie);
        assert_eq!(p2.puzzle_type, PuzzleType::MultiFront);
    }

    #[test]
    fn test_optimal_solution_works() {
        for seed in 0..30 {
            let puzzle = daily_puzzle(seed);
            let result = check_solution(&puzzle, &puzzle.optimal_orders);
            assert!(
                result.objective_met,
                "Optimal solution failed for puzzle seed {}: {}",
                seed, result.message
            );
        }
    }

    #[test]
    fn test_wrong_solution_fails() {
        let puzzle = daily_puzzle(0); // CaptureTheBonus
        // Submit no orders.
        let result = check_solution(&puzzle, &[]);
        assert!(!result.objective_met);
    }

    #[test]
    fn test_min_attackers() {
        let settings = default_settings();
        // 2 defenders: need to kill 2. round(a * 0.6) >= 2 and a - round(2*0.7) > 0.
        // round(2*0.7) = round(1.4) = 1. So need a >= 2 for kills and a - 1 > 0 -> a >= 2.
        // round(2*0.6) = round(1.2) = 1. Not enough. round(3*0.6) = round(1.8) = 2. And 3-1=2>0.
        // So min = 3.
        assert_eq!(min_attackers_to_capture(2, &settings), 3);
        // 1 defender: round(a*0.6) >= 1 -> a >= 1 (round(0.6)=1). round(1*0.7)=1. a-1>=1 -> a>=2.
        assert_eq!(min_attackers_to_capture(1, &settings), 2);
    }
}
