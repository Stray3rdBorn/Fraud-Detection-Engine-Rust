// settings, types, rules and state the engine ties together.
use crate::{
    config::Config,
    event::{Decision, Outcome, TransactionEvent},
    hash::id_hash,
    rules::{
        AccountAgeRule, AccountTakeoverRule, AmountAnomalyRule, BlocklistRule, EntityRule,
        ImpossibleTravelRule, LinkedAccountsRule, NewDeviceRule, Rule, VelocityRule,
    },
    state::{Entities, UserState},
};

// runs every rule on a transaction and turns the hits into one decision.
pub struct Engine {
    // thresholds and weights.
    config: Config,
    // the rules, stored behind one common type so they can sit in one list.
    rules: Vec<Box<dyn Rule>>,
    // the rules that also look at the device and the IP.
    entity_rules: Vec<Box<dyn EntityRule>>,
}

impl Engine {
    // builds an engine with all current rules.
    pub fn new(config: Config) -> Self {
        // the list of rules, new rules are added here.
        let rules: Vec<Box<dyn Rule>> = vec![
            Box::new(VelocityRule),
            Box::new(AmountAnomalyRule),
            Box::new(AccountAgeRule),
            Box::new(NewDeviceRule),
            Box::new(ImpossibleTravelRule),
            Box::new(AccountTakeoverRule),
        ];
        // the rules that need the device and the IP, new ones are added here.
        let entity_rules: Vec<Box<dyn EntityRule>> =
            vec![Box::new(BlocklistRule), Box::new(LinkedAccountsRule)];
        // keep the settings and the rules together.
        Engine {
            config,
            rules,
            entity_rules,
        }
    }

    // decides one transaction and returns the decision plus the updated user, device and IP records.
    pub fn evaluate(
        &self,
        event: &TransactionEvent,
        state: &UserState,
        entities: &Entities,
    ) -> (Decision, UserState, Entities) {
        // ask every rule that looks at the user alone and keep only the ones that fired.
        let user_hits = self
            .rules
            .iter()
            .filter_map(|rule| rule.check(event, state, &self.config));
        // ask every rule that also looks at the device and the IP.
        let entity_hits = self
            .entity_rules
            .iter()
            .filter_map(|rule| rule.check(event, state, entities, &self.config));
        // all hits in one list.
        let hits: Vec<_> = user_hits.chain(entity_hits).collect();
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
        // the new user state is a copy of the old one with this transaction added.
        let mut next = state.clone();
        // record the transaction.
        next.record(event);
        // the device and IP records get the same treatment, they learn that this account used them.
        let mut next_entities = entities.clone();
        // the account is remembered as a number, and the record notes whether the account was blocked.
        next_entities.record(
            id_hash(&event.user_id),
            event.timestamp,
            state.is_blocked(event.timestamp),
        );
        // hand back everything.
        (
            Decision {
                event_id: event.event_id.clone(),
                outcome,
                score,
                reasons,
            },
            next,
            next_entities,
        )
    }
}

// tests for the engine.
#[cfg(test)]
mod tests {
    // bring the engine and helpers into the tests.
    use super::*;
    use crate::event::{AccountEvent, AccountEventKind, sample_event};

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
        let (decision, _, _) =
            engine.evaluate(&sample_event(100_000, 22.0), &state, &Entities::default());
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
        let (decision, _, _) = engine.evaluate(&event, &state, &Entities::default());
        // velocity 20 plus amount 40 plus account age 39.
        assert_eq!(decision.score, 99);
        // the outcome is block.
        assert!(matches!(decision.outcome, Outcome::Block));
        // three rules fired, so three reasons.
        assert_eq!(decision.reasons.len(), 3);
    }

    // failed logins, a new device and a large amount together are blocked.
    #[test]
    fn takeover_sequence_is_blocked() {
        // default engine.
        let engine = Engine::new(Config::default());
        // a user with six normal transactions of 20 on device d1.
        let mut state = UserState::default();
        // record each one.
        for i in 0..6 {
            // add one transaction.
            state.record(&sample_event(i * 10_000, 20.0));
        }
        // three failed logins just before the payment.
        for _ in 0..3 {
            // add one failed login.
            state.record_account(&AccountEvent {
                user_id: "u1".to_string(),
                timestamp: 99_990,
                kind: AccountEventKind::FailedLogin,
            });
        }
        // a large payment from a new device.
        let mut event = sample_event(100_000, 500.0);
        // set the new device.
        event.device_id = "d2".to_string();
        // decide it.
        let (decision, _, _) = engine.evaluate(&event, &state, &Entities::default());
        // amount 40 plus new device 40 plus takeover 45.
        assert_eq!(decision.score, 125);
        // the outcome is block.
        assert!(matches!(decision.outcome, Outcome::Block));
    }

    // a blocked device alone is enough to block.
    #[test]
    fn blocked_device_is_blocked() {
        // default engine.
        let engine = Engine::new(Config::default());
        // the device is blocked for a long time.
        let mut entities = Entities::default();
        // set the block.
        entities.device.blocked_until = Some(1_000_000);
        // an otherwise normal transaction.
        let (decision, _, _) =
            engine.evaluate(&sample_event(100, 20.0), &UserState::default(), &entities);
        // the blocklist weight is 100, plus 10 for no history.
        assert_eq!(decision.score, 110);
        // the outcome is block.
        assert!(matches!(decision.outcome, Outcome::Block));
    }

    // many accounts on one device build up, and the returned records remember the account.
    #[test]
    fn shared_device_builds_up_and_is_remembered() {
        // default engine.
        let engine = Engine::new(Config::default());
        // the first account uses the device and the IP.
        let (_, _, mut entities) = engine.evaluate(
            &sample_event(100, 20.0),
            &UserState::default(),
            &Entities::default(),
        );
        // five more accounts share the same device.
        for user in ["b", "c", "d", "e", "f"] {
            // each one is a new account on the same device.
            let mut event = sample_event(100, 20.0);
            // set the account.
            event.user_id = user.to_string();
            // decide and keep the updated records.
            let (_, _, next) = engine.evaluate(&event, &UserState::default(), &entities);
            // the updated records replace the old ones.
            entities = next;
        }
        // a seventh account on the same device.
        let mut event = sample_event(100, 20.0);
        // set the account.
        event.user_id = "g".to_string();
        // decide it.
        let (decision, _, next) = engine.evaluate(&event, &UserState::default(), &entities);
        // device part 60 at the cap, IP part 12, cold start 10.
        assert_eq!(decision.score, 82);
        // the outcome is block.
        assert!(matches!(decision.outcome, Outcome::Block));
        // the returned record knows the new account.
        assert_eq!(next.device.accounts_with(id_hash("g"), 100, 604_800), 7);
    }

    // the returned state includes the new transaction.
    #[test]
    fn state_is_updated_after_a_decision() {
        // default engine.
        let engine = Engine::new(Config::default());
        // decide one transaction on an empty state.
        let (_, next, _) = engine.evaluate(
            &sample_event(5_000, 30.0),
            &UserState::default(),
            &Entities::default(),
        );
        // the amount was recorded.
        assert_eq!(next.amounts.count(), 1);
        // the first seen time was set.
        assert_eq!(next.first_seen, Some(5_000));
    }
}
