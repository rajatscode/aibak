use strat_engine::map::Map;
use strat_engine::state::{GameState, PlayerId};

const RESET: &str = "\x1b[0m";
const BOLD: &str = "\x1b[1m";
const RED: &str = "\x1b[31m";
const BLUE: &str = "\x1b[34m";
const GRAY: &str = "\x1b[90m";
const GREEN: &str = "\x1b[32m";
const YELLOW: &str = "\x1b[33m";

fn owner_color(owner: PlayerId) -> &'static str {
    match owner {
        0 => BLUE,
        1 => RED,
        _ => GRAY,
    }
}

fn owner_label(owner: PlayerId) -> &'static str {
    match owner {
        0 => "You",
        1 => "AI",
        _ => "---",
    }
}

/// Print the game state as a text-based territory list grouped by bonus.
pub fn print_state(state: &GameState, map: &Map, player: PlayerId) {
    let visible = strat_engine::fog::visible_territories(state, player, map);

    println!("\n{BOLD}═══ Turn {} ═══{RESET}", state.turn);
    println!(
        "{BLUE}You:{RESET} {} territories, income {GREEN}{}{RESET}  |  {RED}AI:{RESET} {} territories",
        state.territory_count_for(player),
        state.income(player, map),
        state.territory_count_for(1 - player),
    );
    println!();

    for bonus in &map.bonuses {
        let my_count = bonus
            .territory_ids
            .iter()
            .filter(|&&tid| state.territory_owners[tid] == player)
            .count();
        let total = bonus.territory_ids.len();
        let bonus_status = if my_count == total {
            format!("{GREEN}✓ +{}{RESET}", bonus.value)
        } else {
            format!("{}/{}", my_count, total)
        };

        println!("{BOLD}{}{RESET} (bonus: {} | {})", bonus.name, bonus.value, bonus_status);
        for &tid in &bonus.territory_ids {
            let t = &map.territories[tid];
            if !visible.contains(&tid) {
                println!("  {GRAY}{:>2}. {:25} ???{RESET}", tid, t.name);
            } else {
                let color = owner_color(state.territory_owners[tid]);
                let label = owner_label(state.territory_owners[tid]);
                println!(
                    "  {color}{:>2}. {:25} [{:>3}] {}{RESET}",
                    tid, t.name, state.territory_armies[tid], label
                );
            }
        }
        println!();
    }
}

/// Print available actions for the player.
pub fn print_help() {
    println!("{BOLD}Commands:{RESET}");
    println!("  {GREEN}d <territory_id> <armies>{RESET}  — Deploy armies");
    println!("  {GREEN}a <from> <to> <armies>{RESET}     — Attack");
    println!("  {GREEN}t <from> <to> <armies>{RESET}     — Transfer");
    println!("  {GREEN}done{RESET}                       — End turn");
    println!("  {GREEN}map{RESET}                        — Show map again");
    println!("  {GREEN}help{RESET}                       — Show this help");
    println!();
}

/// Print picking options.
pub fn print_pick_options(options: &[usize], map: &Map) {
    println!("\n{BOLD}═══ Territory Picking ═══{RESET}");
    println!("Pick {} territories (enter IDs one per line):\n", map.picking.num_picks);
    for &tid in options {
        let t = &map.territories[tid];
        let bonus = &map.bonuses[t.bonus_id];
        let wl = if t.is_wasteland { " [WASTELAND]" } else { "" };
        println!(
            "  {YELLOW}{:>2}{RESET}. {:25} ({}){wl}",
            tid, t.name, bonus.name
        );
    }
    println!();
}

/// Print turn results summary.
pub fn print_turn_summary(old: &GameState, new: &GameState, _map: &Map) {
    let gained = new.territory_count_for(0) as i32 - old.territory_count_for(0) as i32;
    if gained > 0 {
        println!("{GREEN}You gained {} territory(ies) this turn.{RESET}", gained);
    } else if gained < 0 {
        println!("{RED}You lost {} territory(ies) this turn.{RESET}", -gained);
    }
}
