// settings, event and state types the rules read from.
use crate::{config::Config, event::TransactionEvent, state::UserState};

// what a rule reports when it fires.
pub struct Hit {
    // how many points the rule adds to the risk score.
    pub weight: u32,
    // short human readable reason, built from numbers only.
    pub reason: String,
}

// the contract every rule follows, so the engine can run them all the same way.
pub trait Rule: Send + Sync {
    // returns a hit when the rule fires and nothing when the transaction looks normal.
    fn check(&self, event: &TransactionEvent, state: &UserState, config: &Config) -> Option<Hit>;
}

// flags users who make many transactions within one hour.
pub struct VelocityRule;

impl Rule for VelocityRule {
    fn check(&self, event: &TransactionEvent, state: &UserState, config: &Config) -> Option<Hit> {
        // earlier transactions in the last hour plus this one.
        let count = state.velocity.count(event.timestamp) + 1;
        // up to the free amount nothing fires.
        if count <= config.velocity_free {
            // normal activity.
            return None;
        }
        // weight grows with every transaction above the free amount and stops at the cap.
        let weight = (count - config.velocity_free)
            .saturating_mul(config.velocity_per_extra)
            .min(config.velocity_max_weight);
        // report the hit with the count as the reason.
        Some(Hit {
            weight,
            reason: format!("velocity: {count} transactions in the last hour"),
        })
    }
}

// flags amounts far above the user's own normal.
pub struct AmountAnomalyRule;

impl Rule for AmountAnomalyRule {
    fn check(&self, event: &TransactionEvent, state: &UserState, config: &Config) -> Option<Hit> {
        // users with too little history have no trustworthy average yet.
        if state.amounts.count() < config.amount_min_history {
            // add a small default caution instead.
            return Some(Hit {
                weight: config.amount_cold_weight,
                reason: "amount: not enough history for this user".to_string(),
            });
        }
        // the spread never counts as smaller than the floor, which avoids divisions by tiny numbers.
        let spread = state.amounts.std_dev().max(config.amount_min_spread);
        // how many spreads this amount sits above the average.
        let z = (event.amount - state.amounts.mean()) / spread;
        // below the starting point the amount is normal.
        if z < config.amount_z_start {
            // normal amount.
            return None;
        }
        // weight starts at one step and grows for every extra spread, then stops at the cap.
        let weight = (((z - config.amount_z_start + 1.0) * config.amount_per_z) as u32)
            .min(config.amount_max_weight);
        // report the hit with the distance as the reason.
        Some(Hit {
            weight,
            reason: format!("amount: {z:.1} spreads above this user's average"),
        })
    }
}

// flags very new accounts.
pub struct AccountAgeRule;

impl Rule for AccountAgeRule {
    fn check(&self, event: &TransactionEvent, state: &UserState, config: &Config) -> Option<Hit> {
        // start from the signup time, or from the first seen time when signup is unknown.
        let start = event.signup_time.or(state.first_seen)?;
        // age in seconds, a start in the future counts as zero.
        let age = (event.timestamp - start).max(0);
        // accounts past the window are not new.
        if age >= config.account_age_window {
            // old enough.
            return None;
        }
        // the younger the account, the closer the weight gets to the maximum.
        let weight = ((config.account_age_window - age) as u64
            * config.account_age_max_weight as u64
            / config.account_age_window as u64) as u32;
        // report the hit with the age as the reason.
        Some(Hit {
            weight,
            reason: format!("account age: {age} seconds old"),
        })
    }
}

// tests for the rules.
#[cfg(test)]
mod tests {
    // bring the rules and helpers into the tests.
    use super::*;
    use crate::event::sample_event;

    // builds a state that has seen the given amounts, all at time zero.
    fn state_with(amounts: &[f64]) -> UserState {
        // start empty.
        let mut state = UserState::default();
        // record every amount as a transaction.
        for &amount in amounts {
            // add one transaction.
            state.record(&sample_event(0, amount));
        }
        // hand back the filled state.
        state
    }

    // the tenth transaction in an hour fires with weight 20.
    #[test]
    fn velocity_fires_above_the_free_amount() {
        // default settings.
        let config = Config::default();
        // nine earlier transactions at the same time.
        let mut state = UserState::default();
        // record each one.
        for _ in 0..9 {
            // add one transaction.
            state.record(&sample_event(1_000, 10.0));
        }
        // the new transaction is the tenth.
        let hit = VelocityRule
            .check(&sample_event(1_000, 10.0), &state, &config)
            .unwrap();
        // five above the free amount, four points each.
        assert_eq!(hit.weight, 20);
        // an empty state stays silent.
        assert!(VelocityRule
            .check(&sample_event(1_000, 10.0), &UserState::default(), &config)
            .is_none());
    }

    // new users get the small caution, spikes get the cap, normal amounts get nothing.
    #[test]
    fn amount_rule_handles_cold_start_spike_and_normal() {
        // default settings.
        let config = Config::default();
        // a user without history gets the cold weight.
        let cold = AmountAnomalyRule
            .check(&sample_event(0, 50.0), &UserState::default(), &config)
            .unwrap();
        // the cold weight is 10.
        assert_eq!(cold.weight, 10);
        // a user who always spends 20.
        let state = state_with(&[20.0; 6]);
        // an amount of 500 is far above the average, so the cap applies.
        let spike = AmountAnomalyRule
            .check(&sample_event(0, 500.0), &state, &config)
            .unwrap();
        // the cap is 40.
        assert_eq!(spike.weight, 40);
        // an amount of 25 is normal.
        assert!(AmountAnomalyRule
            .check(&sample_event(0, 25.0), &state, &config)
            .is_none());
    }

    // a fresh signup scores high, an old or unknown account scores nothing.
    #[test]
    fn account_age_rule_scores_new_accounts_only() {
        // default settings.
        let config = Config::default();
        // signup one second before the transaction.
        let mut event = sample_event(100_000, 10.0);
        // set the signup time.
        event.signup_time = Some(99_999);
        // one second old gives 39 of 40.
        let hit = AccountAgeRule
            .check(&event, &UserState::default(), &config)
            .unwrap();
        // 3599 of 3600 of the maximum, rounded down.
        assert_eq!(hit.weight, 39);
        // signup one day before the transaction.
        event.signup_time = Some(100_000 - 86_400);
        // an old account does not fire.
        assert!(AccountAgeRule
            .check(&event, &UserState::default(), &config)
            .is_none());
        // no signup time and no history gives nothing.
        assert!(AccountAgeRule
            .check(&sample_event(100_000, 10.0), &UserState::default(), &config)
            .is_none());
    }
}