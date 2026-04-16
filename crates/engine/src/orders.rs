//! Player order types (deploy, attack, transfer, play card) and validation.

use serde::{Deserialize, Serialize};

use crate::cards::Card;
use crate::map::Map;
use crate::state::{GameState, PlayerId};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum Order {
    Deploy { territory: usize, armies: u32 },
    Attack { from: usize, to: usize, armies: u32 },
    Transfer { from: usize, to: usize, armies: u32 },
    PlayCard { card: Card, target: usize },
}

#[derive(Debug, thiserror::Error)]
pub enum OrderError {
    #[error("territory {0} is not owned by player {1}")]
    NotOwned(usize, PlayerId),
    #[error("territory {0} is out of bounds")]
    OutOfBounds(usize),
    #[error("territories {0} and {1} are not adjacent")]
    NotAdjacent(usize, usize),
    #[error("cannot attack own territory {0}")]
    AttackOwnTerritory(usize),
    #[error("cannot transfer to enemy territory {0}")]
    TransferToEnemy(usize),
    #[error("deployed {deployed} armies but only {available} available")]
    OverDeploy { deployed: u32, available: u32 },
    #[error("must deploy at least 1 army")]
    ZeroDeploy,
    #[error("must attack/transfer with at least 1 army")]
    ZeroArmies,
    #[error("player does not have card {0:?}")]
    CardNotInHand(Card),
}

/// Validate a full set of orders for a player in the Play phase.
pub fn validate_orders(
    orders: &[Order],
    player: PlayerId,
    state: &GameState,
    map: &Map,
) -> Result<(), OrderError> {
    let income = state.income(player, map);
    let mut total_deployed = 0u32;

    for order in orders {
        match order {
            Order::Deploy { territory, armies } => {
                check_bounds(*territory, map)?;
                check_owned(*territory, player, state)?;
                if *armies == 0 {
                    return Err(OrderError::ZeroDeploy);
                }
                total_deployed += armies;
            }
            Order::Attack { from, to, armies } => {
                check_bounds(*from, map)?;
                check_bounds(*to, map)?;
                check_owned(*from, player, state)?;
                if state.territory_owners[*to] == player {
                    return Err(OrderError::AttackOwnTerritory(*to));
                }
                if !map.are_adjacent(*from, *to) {
                    return Err(OrderError::NotAdjacent(*from, *to));
                }
                if *armies == 0 {
                    return Err(OrderError::ZeroArmies);
                }
            }
            Order::Transfer { from, to, armies } => {
                check_bounds(*from, map)?;
                check_bounds(*to, map)?;
                check_owned(*from, player, state)?;
                if state.territory_owners[*to] != player {
                    return Err(OrderError::TransferToEnemy(*to));
                }
                if !map.are_adjacent(*from, *to) {
                    return Err(OrderError::NotAdjacent(*from, *to));
                }
                if *armies == 0 {
                    return Err(OrderError::ZeroArmies);
                }
            }
            Order::PlayCard { card, target } => {
                check_bounds(*target, map)?;
                let hand = &state.hands[player as usize];
                if !hand.contains(card) {
                    return Err(OrderError::CardNotInHand(card.clone()));
                }
            }
        }
    }

    if total_deployed > income {
        return Err(OrderError::OverDeploy {
            deployed: total_deployed,
            available: income,
        });
    }

    Ok(())
}

fn check_bounds(territory: usize, map: &Map) -> Result<(), OrderError> {
    if territory >= map.territory_count() {
        return Err(OrderError::OutOfBounds(territory));
    }
    Ok(())
}

fn check_owned(territory: usize, player: PlayerId, state: &GameState) -> Result<(), OrderError> {
    if state.territory_owners[territory] != player {
        return Err(OrderError::NotOwned(territory, player));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::{Bonus, Map, MapSettings, PickingConfig, PickingMethod, Territory};
    use crate::state::GameState;

    fn test_map() -> Map {
        Map {
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

    fn setup_state(map: &Map) -> GameState {
        let mut state = GameState::new(map);
        state.territory_owners = vec![0, 0, 1, 1];
        state.territory_armies = vec![5, 5, 5, 5];
        state.phase = crate::state::Phase::Play;
        state.turn = 1;
        state
    }

    #[test]
    fn test_validate_catches_over_deployment() {
        let map = test_map();
        let state = setup_state(&map);
        let income = state.income(0, &map); // base 5 + bonus 2 = 7
        let orders = vec![Order::Deploy {
            territory: 0,
            armies: income + 1,
        }];
        let result = validate_orders(&orders, 0, &state, &map);
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), OrderError::OverDeploy { .. }));
    }

    #[test]
    fn test_validate_catches_out_of_bounds() {
        let map = test_map();
        let state = setup_state(&map);
        let orders = vec![Order::Deploy {
            territory: 999,
            armies: 1,
        }];
        let result = validate_orders(&orders, 0, &state, &map);
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), OrderError::OutOfBounds(999)));
    }

    #[test]
    fn test_validate_allows_valid_orders() {
        let map = test_map();
        let state = setup_state(&map);
        let income = state.income(0, &map);
        let orders = vec![
            Order::Deploy {
                territory: 0,
                armies: income,
            },
            Order::Attack {
                from: 1,
                to: 2,
                armies: 4,
            },
        ];
        let result = validate_orders(&orders, 0, &state, &map);
        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_catches_attack_own_territory() {
        let map = test_map();
        let state = setup_state(&map);
        let orders = vec![
            Order::Deploy {
                territory: 0,
                armies: 5,
            },
            Order::Attack {
                from: 0,
                to: 1,
                armies: 3,
            }, // 0 and 1 both owned by player 0
        ];
        let result = validate_orders(&orders, 0, &state, &map);
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            OrderError::AttackOwnTerritory(1)
        ));
    }

    #[test]
    fn test_validate_catches_transfer_to_enemy() {
        let map = test_map();
        let state = setup_state(&map);
        let orders = vec![
            Order::Deploy {
                territory: 1,
                armies: 5,
            },
            Order::Transfer {
                from: 1,
                to: 2,
                armies: 3,
            }, // 2 is owned by player 1
        ];
        let result = validate_orders(&orders, 0, &state, &map);
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            OrderError::TransferToEnemy(2)
        ));
    }
}
