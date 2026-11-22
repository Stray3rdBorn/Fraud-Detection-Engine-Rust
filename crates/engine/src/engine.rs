// settings, types, rules and state the engine ties together.
use crate::{
    config::Config,
    event::{Decision, Outcome, TransactionEvent},
    rules::{AccountAgeRule, AmountAnomalyRule, Rule, VelocityRule},
    state::UserState,
};

// runs every rule on a transaction and turns the hits into one decision.
pub struct Engine {
    // thresholds and weights.
    config: Config,
    // the rules, stored behind one common type so they can sit in one list.
    rules: Vec<Box<dyn Rule>>,
}

impl Engine {
    // builds an engine with all current rules.
    pub fn new(config: Config) -> Self {
        // the list of rules, new rules are added here.
        let rules: Vec<Box<dyn Rule>> = vec![
            Box::new(VelocityRule),
            Box::new(AmountAnomalyRule),
            Box::new(AccountAgeRule),
        ];
        // keep the settings and the rules together.
        Engine { config, rules }
    }

    // decides one transaction and returns the decision plus the updated user state.
    pub fn evaluate(&self, event: &TransactionEvent, state: &UserState) -> (Decision, UserState) {
        // ask every rule and keep only the ones that fired.
        let hits: Vec<_> = self
            .rules
            .iter()
            .filter_map(|rule| rule.check(event, state, &self.config))
            .collect();
        // the risk score is the sum of all weights.
        let score: u32 = hits.iter().map(|hit| hit.weight).sum();
        // the thresholds turn the score into an outcome.
        let outcome = if score >= self.config.block_threshold {
            // very risky.
            Outcome::Block
        } else if score >= self.config.review_threshold {
            // risky enough for a human to look.
            Outcome::Review
        } else {
            // normal.
            Outcome::Allow
        };
        // every reason carries its weight.
        let reasons = hits
            .iter()
            .map(|hit| format!("{} (+{})", hit.reason, hit.weight))
            .collect();
        // the new state is a copy of the old one with this transaction added.
        let mut next = state.clone();
        // record the transaction.
        next.record(event);
        // hand back both results.
        (
            Decision {
                event_id: event.event_id.clone(),
                outcome,
                score,
                reasons,
            },
            next,
        )
    }
}

// tests for the engine.
#[cfg(test)]
mod tests {
    // bring the engine and helpers into the tests.
    use super::*;
    use crate::event::sample_event;

    // an honest user with history gets allowed.
    #[test]
    fn honest_transaction_is_allowed() {
        // default engine.
        let engine = Engine::new(Config::default());
        // a user who spends 20 and has been around for a day.
        let mut state = UserState::default();
        // six earlier transactions spread over the day.
        for i in 0..6 {
            // add one transaction.
            state.record(&sample_event(i * 10_000, 20.0));
        }
        // a normal purchase.
        let (decision, _) = engine.evaluate(&sample_event(100_000, 22.0), &state);
        // nothing fires.
        assert_eq!(decision.score, 0);
        // the outcome is allow.
        assert!(matches!(decision.outcome, Outcome::Allow));
    }

    // a burst, a spike and a fresh account together are blocked.
    #[test]
    fn burst_spike_and_new_account_is_blocked() {
        // default engine.
        let engine = Engine::new(Config::default());
        // nine earlier transactions of 20 within the same minute.
        let mut state = UserState::default();
        // record each one.
        for _ in 0..9 {
            // add one transaction.
            state.record(&sample_event(100_000, 20.0));
        }
        // a large purchase from an account created one second ago.
        let mut event = sample_event(100_000, 500.0);
        // set the signup time.
        event.signup_time = Some(99_999);
        // decide it.
        let (decision, _) = engine.evaluate(&event, &state);
        // velocity 20 plus amount 40 plus account age 39.
        assert_eq!(decision.score, 99);
        // the outcome is block.
        assert!(matches!(decision.outcome, Outcome::Block));
        // three rules fired, so three reasons.
        assert_eq!(decision.reasons.len(), 3);
    }

    // the returned state includes the new transaction.
    #[test]
    fn state_is_updated_after_a_decision() {
        // default engine.
        let engine = Engine::new(Config::default());
        // decide one transaction on an empty state.
        let (_, next) = engine.evaluate(&sample_event(5_000, 30.0), &UserState::default());
        // the amount was recorded.
        assert_eq!(next.amounts.count(), 1);
        // the first seen time was set.
        assert_eq!(next.first_seen, Some(5_000));
    }
}