//! Turn resolution: deploys, card plays, interleaved attacks/transfers, and elimination checks.

use rand::Rng;
use serde::{Deserialize, Serialize};

use crate::cards::{Card, apply_blockade, award_card_pieces};
use crate::combat::resolve_attack;
use crate::board::Board;
use crate::orders::Order;
use crate::state::{GameState, PlayerId};

/// An event that occurred during turn resolution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TurnEvent {
    Deploy {
        player: PlayerId,
        territory: usize,
        armies: u32,
    },
    Attack {
        player: PlayerId,
        from: usize,
        to: usize,
        armies: u32,
        defenders: u32,
        attackers_killed: u32,
        defenders_killed: u32,
        captured: bool,
        surviving_attackers: u32,
    },
    Transfer {
        player: PlayerId,
        from: usize,
        to: usize,
        armies: u32,
    },
    Blockade {
        player: PlayerId,
        territory: usize,
        new_armies: u32,
    },
    Capture {
        player: PlayerId,
        territory: usize,
    },
    Eliminated {
        player: PlayerId,
    },
    Victory {
        player: PlayerId,
    },
}

/// Result of resolving a turn.
pub struct TurnResult {
    pub state: GameState,
    pub events: Vec<TurnEvent>,
}

/// Resolve a full turn given both players' orders.
pub fn resolve_turn(
    state: &GameState,
    orders: [Vec<Order>; 2],
    board: &Board,
    rng: &mut impl Rng,
) -> TurnResult {
    let mut new_state = state.clone();
    let mut events = Vec::new();
    let mut territories_captured: [u32; 2] = [0; 2];

    // Track available armies per territory.
    let mut available: Vec<u32> = new_state.territory_armies.clone();

    // Phase 1: Deployments.
    for player in 0..2u8 {
        for order in &orders[player as usize] {
            if let Order::Deploy { territory, armies } = order {
                new_state.territory_armies[*territory] += armies;
                available[*territory] += armies;
                events.push(TurnEvent::Deploy {
                    player,
                    territory: *territory,
                    armies: *armies,
                });
            }
        }
    }

    // Phase 2: Reinforcement cards.
    for player in 0..2u8 {
        for order in &orders[player as usize] {
            if let Order::PlayCard {
                card: Card::Reinforcement(value),
                target,
            } = order
            {
                new_state.territory_armies[*target] += value;
                available[*target] += value;
                let hand = &mut new_state.hands[player as usize];
                if let Some(pos) = hand
                    .iter()
                    .position(|c| matches!(c, Card::Reinforcement(v) if *v == *value))
                {
                    hand.remove(pos);
                }
                events.push(TurnEvent::Deploy {
                    player,
                    territory: *target,
                    armies: *value,
                });
            }
        }
    }

    // Phase 3: Blockade cards.
    for player in 0..2u8 {
        for order in &orders[player as usize] {
            if let Order::PlayCard {
                card: Card::Blockade,
                target,
            } = order
            {
                let old = new_state.territory_armies[*target];
                apply_blockade(&mut new_state, player, *target);
                available[*target] = 0;
                events.push(TurnEvent::Blockade {
                    player,
                    territory: *target,
                    new_armies: old * 3,
                });
            }
        }
    }

    // Phase 4: Interleaved attack/transfer rounds.
    let mut move_queues: [Vec<Order>; 2] = [Vec::new(), Vec::new()];
    for player in 0..2u8 {
        for order in &orders[player as usize] {
            match order {
                Order::Attack { .. } | Order::Transfer { .. } => {
                    move_queues[player as usize].push(order.clone());
                }
                _ => {}
            }
        }
    }

    let first_player: PlayerId = if rng.gen_bool(0.5) { 0 } else { 1 };
    let player_order = [first_player, 1 - first_player];

    let mut indices = [0usize; 2];
    loop {
        let mut any_moved = false;
        for &player in &player_order {
            let pidx = player as usize;
            while indices[pidx] < move_queues[pidx].len() {
                let order = &move_queues[pidx][indices[pidx]];
                indices[pidx] += 1;

                match order {
                    Order::Attack { from, to, armies } => {
                        if new_state.territory_owners[*from] != player {
                            continue;
                        }
                        let usable = available[*from].min(new_state.territory_armies[*from]);
                        let actual_armies = (*armies).min(usable.saturating_sub(1));
                        if actual_armies == 0 {
                            continue;
                        }
                        available[*from] -= actual_armies;

                        if new_state.territory_owners[*to] == player {
                            // Became a transfer.
                            new_state.territory_armies[*from] -= actual_armies;
                            new_state.territory_armies[*to] += actual_armies;
                            available[*to] += actual_armies;
                            events.push(TurnEvent::Transfer {
                                player,
                                from: *from,
                                to: *to,
                                armies: actual_armies,
                            });
                        } else {
                            let defenders = new_state.territory_armies[*to];
                            let result = resolve_attack(actual_armies, defenders, board.settings());

                            new_state.territory_armies[*from] -= actual_armies;

                            if result.captured {
                                new_state.territory_owners[*to] = player;
                                new_state.territory_armies[*to] = result.surviving_attackers;
                                available[*to] = result.surviving_attackers;
                                territories_captured[pidx] += 1;
                                events.push(TurnEvent::Attack {
                                    player,
                                    from: *from,
                                    to: *to,
                                    armies: actual_armies,
                                    defenders,
                                    attackers_killed: result.attackers_killed,
                                    defenders_killed: result.defenders_killed,
                                    captured: true,
                                    surviving_attackers: result.surviving_attackers,
                                });
                                events.push(TurnEvent::Capture {
                                    player,
                                    territory: *to,
                                });
                            } else {
                                new_state.territory_armies[*to] = result.surviving_defenders;
                                events.push(TurnEvent::Attack {
                                    player,
                                    from: *from,
                                    to: *to,
                                    armies: actual_armies,
                                    defenders,
                                    attackers_killed: result.attackers_killed,
                                    defenders_killed: result.defenders_killed,
                                    captured: false,
                                    surviving_attackers: result.surviving_attackers,
                                });
                            }
                        }
                        any_moved = true;
                        break;
                    }
                    Order::Transfer { from, to, armies } => {
                        if new_state.territory_owners[*from] != player {
                            continue;
                        }
                        if new_state.territory_owners[*to] != player {
                            continue;
                        }
                        let usable = available[*from].min(new_state.territory_armies[*from]);
                        let actual_armies = (*armies).min(usable.saturating_sub(1));
                        if actual_armies == 0 {
                            continue;
                        }
                        available[*from] -= actual_armies;
                        new_state.territory_armies[*from] -= actual_armies;
                        new_state.territory_armies[*to] += actual_armies;
                        available[*to] += actual_armies;
                        events.push(TurnEvent::Transfer {
                            player,
                            from: *from,
                            to: *to,
                            armies: actual_armies,
                        });
                        any_moved = true;
                        break;
                    }
                    _ => continue,
                }
            }
        }
        if !any_moved {
            break;
        }
    }

    // Award card pieces.
    for player in 0..2u8 {
        award_card_pieces(
            &mut new_state,
            player,
            territories_captured[player as usize],
        );
    }

    // Check for elimination.
    new_state.check_elimination();
    if let Some(winner) = new_state.winner {
        let loser = 1 - winner;
        events.push(TurnEvent::Eliminated { player: loser });
        events.push(TurnEvent::Victory { player: winner });
    }

    new_state.turn += 1;

    TurnResult {
        state: new_state,
        events,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::board::Board;
    use crate::map::{Bonus, MapFile, MapSettings, PickingConfig, PickingMethod, Territory};
    use rand::SeedableRng;
    use rand::rngs::StdRng;

    fn test_map() -> MapFile {
        MapFile {
            id: "test".into(),
            name: "Test".into(),
            territories: vec![
                Territory {
                    id: 0,
                    name: "A".into(),
                    bonus_id: 0,
                    is_wasteland: false,
                    default_armies: 2,
                    adjacent: vec![1],
                    visual: None,
                },
                Territory {
                    id: 1,
                    name: "B".into(),
                    bonus_id: 0,
                    is_wasteland: false,
                    default_armies: 2,
                    adjacent: vec![0, 2],
                    visual: None,
                },
                Territory {
                    id: 2,
                    name: "C".into(),
                    bonus_id: 1,
                    is_wasteland: false,
                    default_armies: 2,
                    adjacent: vec![1, 3],
                    visual: None,
                },
                Territory {
                    id: 3,
                    name: "D".into(),
                    bonus_id: 1,
                    is_wasteland: false,
                    default_armies: 2,
                    adjacent: vec![2],
                    visual: None,
                },
            ],
            bonuses: vec![
                Bonus {
                    id: 0,
                    name: "Left".into(),
                    value: 2,
                    territory_ids: vec![0, 1],
                    visual: None,
                },
                Bonus {
                    id: 1,
                    name: "Right".into(),
                    value: 2,
                    territory_ids: vec![2, 3],
                    visual: None,
                },
            ],
            picking: PickingConfig {
                num_picks: 1,
                method: PickingMethod::RandomWarlords,
            },
            settings: MapSettings {
                luck_pct: 0,
                base_income: 5,
                wasteland_armies: 10,
                unpicked_neutral_armies: 4,
                fog_of_war: true,
                offense_kill_rate: 0.6,
                defense_kill_rate: 0.7,
            },
        }
    }

    #[test]
    fn test_deploy_and_attack() {
        let map = test_map();
        let board = Board::from_map(map);
        let mut state = GameState::new(&board);
        state.territory_owners = vec![0, 0, 1, 1];
        state.territory_armies = vec![1, 1, 1, 1];
        state.phase = crate::state::Phase::Play;
        state.turn = 1;

        let mut rng = StdRng::seed_from_u64(42);
        let p0_orders = vec![
            Order::Deploy {
                territory: 1,
                armies: 5,
            },
            Order::Attack {
                from: 1,
                to: 2,
                armies: 5,
            },
        ];
        let p1_orders = vec![Order::Deploy {
            territory: 2,
            armies: 5,
        }];

        let result = resolve_turn(&state, [p0_orders, p1_orders], &board, &mut rng);
        let new_state = &result.state;

        // Verify events were generated.
        assert!(result.events.len() >= 3); // 2 deploys + 1 attack
        assert_eq!(new_state.territory_owners[2], 1); // not captured
    }

    #[test]
    fn test_transfer() {
        let map = test_map();
        let board = Board::from_map(map);
        let mut state = GameState::new(&board);
        state.territory_owners = vec![0, 0, 1, 1];
        state.territory_armies = vec![5, 1, 1, 1];
        state.phase = crate::state::Phase::Play;
        state.turn = 1;

        let mut rng = StdRng::seed_from_u64(42);
        let p0_orders = vec![
            Order::Deploy {
                territory: 0,
                armies: 5,
            },
            Order::Transfer {
                from: 0,
                to: 1,
                armies: 9,
            },
        ];
        let p1_orders = vec![Order::Deploy {
            territory: 2,
            armies: 5,
        }];

        let result = resolve_turn(&state, [p0_orders, p1_orders], &board, &mut rng);
        let new_state = &result.state;

        assert_eq!(new_state.territory_armies[0], 1);
        assert_eq!(new_state.territory_armies[1], 10);
    }
}
