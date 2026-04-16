mod ai;
mod display;

use std::io::{self, BufRead, Write};
use std::path::PathBuf;

use rand::SeedableRng;
use rand::rngs::StdRng;

use strat_engine::map::Map;
use strat_engine::orders::Order;
use strat_engine::picking;
use strat_engine::state::{GameState, Phase};
use strat_engine::turn::resolve_turn;

const PLAYER: u8 = 0;
const AI: u8 = 1;

fn main() {
    let map_path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "maps/small_earth.json".to_string());
    let map = Map::load(&PathBuf::from(&map_path)).unwrap_or_else(|e| {
        eprintln!("Failed to load map '{}': {}", map_path, e);
        std::process::exit(1);
    });

    let seed: u64 = std::env::args()
        .nth(2)
        .and_then(|s| s.parse().ok())
        .unwrap_or_else(rand::random);

    println!("\x1b[1mStrat Club — Local Test Game\x1b[0m");
    println!(
        "Map: {} ({} territories, {} bonuses)",
        map.name,
        map.territory_count(),
        map.bonuses.len()
    );
    println!("Seed: {}\n", seed);

    let mut rng = StdRng::seed_from_u64(seed);
    let mut state = GameState::new(&map);

    // Picking phase.
    let options = picking::generate_pick_options(&map, &mut rng);
    display::print_pick_options(&options, &map);

    let player_picks = read_picks(&map, &options);
    let ai_picks = ai::generate_picks(&state, &map);

    picking::resolve_picks(
        &mut state,
        [&player_picks, &ai_picks],
        &map,
        picking::DEFAULT_STARTING_ARMIES,
    );
    println!("\n\x1b[1mPicking complete! Game begins.\x1b[0m");

    // Main game loop.
    loop {
        display::print_state(&state, &map, PLAYER);

        if state.phase == Phase::Finished {
            if state.winner == Some(PLAYER) {
                println!("\n\x1b[32;1m🏆 You win!\x1b[0m");
            } else {
                println!("\n\x1b[31;1m💀 You lose.\x1b[0m");
            }
            break;
        }

        display::print_help();
        let player_orders = read_orders(&state, &map);
        let ai_orders = ai::generate_orders(&state, AI, &map);

        let old_state = state.clone();
        let result = resolve_turn(&state, [player_orders, ai_orders], &map, &mut rng);
        state = result.state;
        display::print_turn_summary(&old_state, &state, &map);
    }
}

fn prompt(msg: &str) -> String {
    print!("{}", msg);
    io::stdout().flush().unwrap();
    let mut line = String::new();
    io::stdin().lock().read_line(&mut line).unwrap();
    line.trim().to_string()
}

fn read_picks(map: &Map, _options: &[usize]) -> Vec<usize> {
    let mut picks = Vec::new();
    let num = map.picking.num_picks;
    while picks.len() < num {
        let remaining = num - picks.len();
        let line = prompt(&format!("Pick territory ({} remaining): ", remaining));
        match line.parse::<usize>() {
            Ok(tid) if tid < map.territory_count() => {
                if picks.contains(&tid) {
                    println!("Already picked that territory.");
                } else if map.territories[tid].is_wasteland {
                    println!("Can't pick a wasteland.");
                } else {
                    println!("  → {}", map.territories[tid].name);
                    picks.push(tid);
                }
            }
            _ => println!(
                "Invalid territory ID. Enter a number 0-{}.",
                map.territory_count() - 1
            ),
        }
    }
    picks
}

fn read_orders(state: &GameState, map: &Map) -> Vec<Order> {
    let mut orders = Vec::new();
    let income = state.income(PLAYER, map);
    let mut deployed = 0u32;

    println!("\x1b[33mYou have {} armies to deploy.\x1b[0m", income);

    loop {
        let line = prompt("> ");
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.is_empty() {
            continue;
        }

        match parts[0] {
            "done" | "d!" => {
                if deployed < income {
                    println!(
                        "You must deploy all {} armies first ({} remaining).",
                        income,
                        income - deployed
                    );
                    continue;
                }
                break;
            }
            "map" | "m" => {
                display::print_state(state, map, PLAYER);
                continue;
            }
            "help" | "h" | "?" => {
                display::print_help();
                continue;
            }
            "undo" | "u" => {
                if let Some(removed) = orders.pop() {
                    if let Order::Deploy { armies, .. } = removed {
                        deployed -= armies;
                    }
                    println!("Removed last order. {} orders remaining.", orders.len());
                } else {
                    println!("No orders to undo.");
                }
                continue;
            }
            "d" => {
                if parts.len() != 3 {
                    println!("Usage: d <territory_id> <armies>");
                    continue;
                }
                let tid: usize = match parts[1].parse() {
                    Ok(v) => v,
                    Err(_) => {
                        println!("Invalid territory ID.");
                        continue;
                    }
                };
                let armies: u32 = match parts[2].parse() {
                    Ok(v) if v > 0 => v,
                    _ => {
                        println!("Invalid army count.");
                        continue;
                    }
                };
                if tid >= map.territory_count() || state.territory_owners[tid] != PLAYER {
                    println!("You don't own territory {}.", tid);
                    continue;
                }
                if deployed + armies > income {
                    println!(
                        "Can't deploy {} more (only {} remaining of {}).",
                        armies,
                        income - deployed,
                        income
                    );
                    continue;
                }
                deployed += armies;
                orders.push(Order::Deploy {
                    territory: tid,
                    armies,
                });
                println!(
                    "  Deploy {} to {} ({} remaining)",
                    armies,
                    map.territories[tid].name,
                    income - deployed
                );
            }
            "a" => {
                if parts.len() != 4 {
                    println!("Usage: a <from> <to> <armies>");
                    continue;
                }
                let from: usize = match parts[1].parse() {
                    Ok(v) => v,
                    Err(_) => {
                        println!("Invalid territory ID.");
                        continue;
                    }
                };
                let to: usize = match parts[2].parse() {
                    Ok(v) => v,
                    Err(_) => {
                        println!("Invalid territory ID.");
                        continue;
                    }
                };
                let armies: u32 = match parts[3].parse() {
                    Ok(v) if v > 0 => v,
                    _ => {
                        println!("Invalid army count.");
                        continue;
                    }
                };
                if from >= map.territory_count() || state.territory_owners[from] != PLAYER {
                    println!("You don't own territory {}.", from);
                    continue;
                }
                if to >= map.territory_count() {
                    println!("Invalid target territory.");
                    continue;
                }
                if !map.are_adjacent(from, to) {
                    println!("{} and {} are not adjacent.", from, to);
                    continue;
                }
                if state.territory_owners[to] == PLAYER {
                    println!("Can't attack your own territory. Use 't' for transfer.");
                    continue;
                }
                orders.push(Order::Attack { from, to, armies });
                println!(
                    "  Attack {} → {} with {} armies",
                    map.territories[from].name, map.territories[to].name, armies
                );
            }
            "t" => {
                if parts.len() != 4 {
                    println!("Usage: t <from> <to> <armies>");
                    continue;
                }
                let from: usize = match parts[1].parse() {
                    Ok(v) => v,
                    Err(_) => {
                        println!("Invalid territory ID.");
                        continue;
                    }
                };
                let to: usize = match parts[2].parse() {
                    Ok(v) => v,
                    Err(_) => {
                        println!("Invalid territory ID.");
                        continue;
                    }
                };
                let armies: u32 = match parts[3].parse() {
                    Ok(v) if v > 0 => v,
                    _ => {
                        println!("Invalid army count.");
                        continue;
                    }
                };
                if from >= map.territory_count() || state.territory_owners[from] != PLAYER {
                    println!("You don't own territory {}.", from);
                    continue;
                }
                if to >= map.territory_count() || state.territory_owners[to] != PLAYER {
                    println!("You don't own territory {} (or it's not yours).", to);
                    continue;
                }
                if !map.are_adjacent(from, to) {
                    println!("{} and {} are not adjacent.", from, to);
                    continue;
                }
                orders.push(Order::Transfer { from, to, armies });
                println!(
                    "  Transfer {} → {} with {} armies",
                    map.territories[from].name, map.territories[to].name, armies
                );
            }
            _ => {
                println!("Unknown command. Type 'help' for options.");
            }
        }
    }

    orders
}
