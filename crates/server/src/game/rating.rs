/// Glicko-2 rating calculation.
///
/// Reference: Mark Glickman, "Example of the Glicko-2 system" (2013).

use std::f64::consts::PI;

/// System constant controlling volatility change speed.
const TAU: f64 = 0.5;

/// Convergence tolerance for the volatility iteration.
const EPSILON: f64 = 0.000001;

/// A player's Glicko-2 rating parameters.
#[derive(Debug, Clone, Copy)]
pub struct Rating {
    /// Rating on the Glicko-2 scale (default 1500 on Glicko scale).
    pub rating: f64,
    /// Rating deviation (default 350 on Glicko scale).
    pub rd: f64,
    /// Rating volatility (default 0.06).
    pub volatility: f64,
}

impl Default for Rating {
    fn default() -> Self {
        Self {
            rating: 1500.0,
            rd: 350.0,
            volatility: 0.06,
        }
    }
}

/// Outcome of a game from a player's perspective.
#[derive(Debug, Clone, Copy)]
pub enum Outcome {
    Win,
    Loss,
}

impl Outcome {
    fn score(self) -> f64 {
        match self {
            Outcome::Win => 1.0,
            Outcome::Loss => 0.0,
        }
    }
}

/// Convert from Glicko scale to Glicko-2 internal scale.
fn to_glicko2(rating: f64) -> f64 {
    (rating - 1500.0) / 173.7178
}

/// Convert from Glicko-2 internal scale back to Glicko scale.
fn from_glicko2(mu: f64) -> f64 {
    mu * 173.7178 + 1500.0
}

/// Convert RD from Glicko scale to Glicko-2 internal scale.
fn rd_to_glicko2(rd: f64) -> f64 {
    rd / 173.7178
}

/// Convert RD from Glicko-2 internal scale back to Glicko scale.
fn rd_from_glicko2(phi: f64) -> f64 {
    phi * 173.7178
}

/// The g function from the Glicko-2 algorithm.
fn g(phi: f64) -> f64 {
    1.0 / (1.0 + 3.0 * phi * phi / (PI * PI)).sqrt()
}

/// The E (expected score) function from the Glicko-2 algorithm.
fn expected(mu: f64, mu_j: f64, phi_j: f64) -> f64 {
    1.0 / (1.0 + (-g(phi_j) * (mu - mu_j)).exp())
}

/// Update a player's rating after a single game result against one opponent.
pub fn update_rating(player: Rating, opponent: Rating, outcome: Outcome) -> Rating {
    let mu = to_glicko2(player.rating);
    let phi = rd_to_glicko2(player.rd);
    let sigma = player.volatility;

    let mu_j = to_glicko2(opponent.rating);
    let phi_j = rd_to_glicko2(opponent.rd);

    let g_j = g(phi_j);
    let e_j = expected(mu, mu_j, phi_j);
    let s = outcome.score();

    // Step 3: Compute estimated variance v.
    let v = 1.0 / (g_j * g_j * e_j * (1.0 - e_j));

    // Step 4: Compute delta.
    let delta = v * g_j * (s - e_j);

    // Step 5: Compute new volatility sigma'.
    let a = (sigma * sigma).ln();
    let delta_sq = delta * delta;
    let phi_sq = phi * phi;

    let f = |x: f64| -> f64 {
        let ex = x.exp();
        let d = phi_sq + v + ex;
        (ex * (delta_sq - phi_sq - v - ex)) / (2.0 * d * d) - (x - a) / (TAU * TAU)
    };

    // Find initial bounds.
    let mut big_a = a;
    let mut big_b = if delta_sq > phi_sq + v {
        (delta_sq - phi_sq - v).ln()
    } else {
        let mut k = 1.0_f64;
        loop {
            let val = a - k * TAU;
            if f(val) > 0.0 {
                break val;
            }
            k += 1.0;
        }
    };

    // Iterate to convergence.
    let mut f_a = f(big_a);
    let mut f_b = f(big_b);

    while (big_b - big_a).abs() > EPSILON {
        let big_c = big_a + (big_a - big_b) * f_a / (f_b - f_a);
        let f_c = f(big_c);

        if f_c * f_b <= 0.0 {
            big_a = big_b;
            f_a = f_b;
        } else {
            f_a /= 2.0;
        }

        big_b = big_c;
        f_b = f_c;
    }

    let new_sigma = (big_a / 2.0).exp();

    // Step 6: Update phi to new pre-rating-period value.
    let phi_star = (phi_sq + new_sigma * new_sigma).sqrt();

    // Step 7: Update rating and RD.
    let new_phi = 1.0 / (1.0 / (phi_star * phi_star) + 1.0 / v).sqrt();
    let new_mu = mu + new_phi * new_phi * g_j * (s - e_j);

    Rating {
        rating: from_glicko2(new_mu),
        rd: rd_from_glicko2(new_phi),
        volatility: new_sigma,
    }
}

/// Update both players' ratings after a game completes.
/// Returns (winner_rating, loser_rating).
pub fn update_ratings_after_game(winner: Rating, loser: Rating) -> (Rating, Rating) {
    let new_winner = update_rating(winner, loser, Outcome::Win);
    let new_loser = update_rating(loser, winner, Outcome::Loss);
    (new_winner, new_loser)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_rating() {
        let r = Rating::default();
        assert!((r.rating - 1500.0).abs() < f64::EPSILON);
        assert!((r.rd - 350.0).abs() < f64::EPSILON);
        assert!((r.volatility - 0.06).abs() < f64::EPSILON);
    }

    #[test]
    fn test_winner_gains_rating() {
        let p1 = Rating::default();
        let p2 = Rating::default();
        let (w, l) = update_ratings_after_game(p1, p2);
        assert!(w.rating > 1500.0);
        assert!(l.rating < 1500.0);
    }

    #[test]
    fn test_rd_decreases() {
        let p1 = Rating::default();
        let p2 = Rating::default();
        let (w, l) = update_ratings_after_game(p1, p2);
        assert!(w.rd < 350.0);
        assert!(l.rd < 350.0);
    }

    #[test]
    fn test_upset_gives_more_points() {
        let strong = Rating {
            rating: 1800.0,
            rd: 50.0,
            volatility: 0.06,
        };
        let weak = Rating {
            rating: 1200.0,
            rd: 50.0,
            volatility: 0.06,
        };

        // Weak player wins (upset).
        let (new_weak, _new_strong) = update_ratings_after_game(weak, strong);
        let gain_upset = new_weak.rating - weak.rating;

        // Strong player wins (expected).
        let (new_strong2, _) = update_ratings_after_game(strong, weak);
        let gain_expected = new_strong2.rating - strong.rating;

        assert!(gain_upset > gain_expected);
    }
}
