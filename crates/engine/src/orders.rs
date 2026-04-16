use serde::{Deserialize, Serialize};

use crate::cards::Card;
use crate::map::Map;
use crate::state::{GameState, PlayerId};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum Order {
    Deploy {
        territory: usize,
        armies: u32,
    },
    Attack {
        from: usize,
        to: usize,
        armies: u32,
    },
    Transfer {
        from: usize,
        to: usize,
        armies: u32,
    },
    PlayCard {
        card: Card,
        target: usize,
    },
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
