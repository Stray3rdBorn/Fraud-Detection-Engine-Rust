// settings, event and state types the rules read from.
use crate::{
    config::Config,
    event::TransactionEvent,
    geo,
    hash::id_hash,
    state::{Entities, UserState},
};

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

// the contract for rules that also look at the device and the IP, which are shared between accounts.
pub trait EntityRule: Send + Sync {
    // returns a hit when the rule fires and nothing when the transaction looks normal.
    fn check(
        &self,
        event: &TransactionEvent,
        user: &UserState,
        entities: &Entities,
        config: &Config,
    ) -> Option<Hit>;
}

// names of the signals that answered yes.
fn fired_names<'a>(signals: &[(&'a str, bool)]) -> Vec<&'a str> {
    // keep the names whose answer is yes.
    signals
        .iter()
        .filter(|(_, on)| *on)
        .map(|(name, _)| *name)
        .collect()
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

        // how many spreads this amount sits above the average.
        let z = state.amounts.z(event.amount, config.amount_min_spread);
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

// true when the user has used other devices before but not this one.
fn is_new_device(event: &TransactionEvent, state: &UserState) -> bool {
    // a user with no known device has nothing to compare against.
    !state.devices.is_empty() && !state.devices.contains(&event.device_id)
}

// true when the user has a baseline and this amount is far above it.
fn is_large_amount(event: &TransactionEvent, state: &UserState, config: &Config) -> bool {
    // the baseline needs enough history, and the amount must reach the starting distance.
    state.amounts.count() >= config.amount_min_history
        && state.amounts.z(event.amount, config.amount_min_spread) >= config.amount_z_start
}

// flags a device the user has not used before.
pub struct NewDeviceRule;

impl Rule for NewDeviceRule {
    fn check(&self, event: &TransactionEvent, state: &UserState, config: &Config) -> Option<Hit> {
        // a known device or a first device is fine.
        if !is_new_device(event, state) {
            // normal activity.
            return None;
        }
        // a large amount on a new device is worse.
        let large = is_large_amount(event, state, config);
        // a large amount adds a bonus on top of the base weight.
        let weight = config.new_device_weight
            + if large {
                config.new_device_large_bonus
            } else {
                0
            };
        // the reason says whether the amount was large.
        let reason = if large {
            "new device used with a large amount"
        } else {
            "new device"
        };
        // report the hit.
        Some(Hit {
            weight,
            reason: reason.to_string(),
        })
    }
}

// flags two cities that are too far apart for the time between the transactions.
pub struct ImpossibleTravelRule;

impl Rule for ImpossibleTravelRule {
    fn check(&self, event: &TransactionEvent, state: &UserState, config: &Config) -> Option<Hit> {
        // both the last position and the current city must be known, otherwise nothing can be compared.
        let last = state.last_place?;
        // position of the current city.
        let pos = event.city.as_deref().and_then(geo::lookup)?;
        // distance between the two positions.
        let km = geo::distance_km(last.pos, pos);
        // short moves are ignored.
        if km < config.travel_min_km {
            // normal activity.
            return None;
        }
        // seconds since the last position, at least one so the division below is safe.
        let elapsed = (event.timestamp - last.time).max(1);
        // speed in kilometers per hour.
        let speed = km * 3600.0 / elapsed as f64;
        // a believable speed does not fire.
        if speed <= config.travel_max_kmh {
            // normal activity.
            return None;
        }
        // report the hit with the distance and the time as the reason.
        Some(Hit {
            weight: config.travel_weight,
            reason: format!("impossible travel: {km:.0} km in {elapsed} seconds"),
        })
    }
}

// flags several weak account signals that fire together.
pub struct AccountTakeoverRule;

impl Rule for AccountTakeoverRule {
    fn check(&self, event: &TransactionEvent, state: &UserState, config: &Config) -> Option<Hit> {
        // each signal is a name with a yes or no answer.
        let signals = [
            (
                "failed logins",
                state.failed_logins.count(event.timestamp) >= config.takeover_failed_logins,
            ),
            (
                "password change",
                // the change must be in the past and inside the window.
                state.password_changed_at.is_some_and(|changed| {
                    (0..config.takeover_password_window)
                        .contains(&event.timestamp.saturating_sub(changed))
                }),
            ),
            ("new device", is_new_device(event, state)),
            ("large amount", is_large_amount(event, state, config)),
        ];
        // keep the names of the signals that answered yes.
        let fired = fired_names(&signals);
        // number of signals that fired.
        let count = fired.len() as u32;
        // too few signals together is not a takeover.
        if count < config.takeover_min_signals {
            // normal activity.
            return None;
        }
        // the base weight covers the minimum, every further signal adds a little more.
        let weight = config.takeover_weight
            + (count - config.takeover_min_signals) * config.takeover_per_extra;
        // report the hit with the fired signals as the reason.
        Some(Hit {
            weight,
            reason: format!("account takeover signals: {}", fired.join(", ")),
        })
    }
}

// flags an account, a device or an IP that is on the blocklist.
pub struct BlocklistRule;

impl EntityRule for BlocklistRule {
    fn check(
        &self,
        event: &TransactionEvent,
        user: &UserState,
        entities: &Entities,
        config: &Config,
    ) -> Option<Hit> {
        // each place a block can sit, with its yes or no answer.
        let checks = [
            ("account", user.is_blocked(event.timestamp)),
            ("device", entities.device.is_blocked(event.timestamp)),
            ("IP", entities.ip.is_blocked(event.timestamp)),
        ];
        // the names of the places that are blocked.
        let blocked = fired_names(&checks);
        // nothing blocked means nothing fires.
        if blocked.is_empty() {
            // normal activity.
            return None;
        }
        // one hit with a heavy weight, the reason names every blocked place.
        Some(Hit {
            weight: config.blocklist_weight,
            reason: format!("blocklist: {} blocked", blocked.join(", ")),
        })
    }
}

// flags devices and IPs that are shared by many accounts, a sign of fraud rings.
pub struct LinkedAccountsRule;

impl EntityRule for LinkedAccountsRule {
    fn check(
        &self,
        event: &TransactionEvent,
        _user: &UserState,
        entities: &Entities,
        config: &Config,
    ) -> Option<Hit> {
        // the account as a number, which is how the lists remember it.
        let user = id_hash(&event.user_id);
        // accounts on the device including this one.
        let on_device = entities
            .device
            .accounts_with(user, event.timestamp, config.link_window);
        // accounts on the IP including this one.
        let on_ip = entities
            .ip
            .accounts_with(user, event.timestamp, config.link_window);
        // a shared device is a strong signal, it grows with every extra account and stops at the cap.
        let device_weight = on_device
            .saturating_sub(config.device_free_accounts)
            .saturating_mul(config.device_per_account)
            .min(config.device_max_weight);
        // a shared IP is a weak signal, because homes and offices share IPs.
        let ip_weight = on_ip
            .saturating_sub(config.ip_free_accounts)
            .saturating_mul(config.ip_per_account)
            .min(config.ip_max_weight);
        // a blocked account that used the same device or IP recently adds more.
        let near_blocked = entities
            .device
            .has_blocked_account(event.timestamp, config.link_window)
            || entities
                .ip
                .has_blocked_account(event.timestamp, config.link_window);
        // the extra weight applies only when such a link exists.
        let blocked_weight = if near_blocked {
            config.linked_blocked_weight
        } else {
            0
        };
        // all parts together.
        let weight = device_weight + ip_weight + blocked_weight;
        // nothing shared and nothing blocked means nothing fires.
        if weight == 0 {
            // normal activity.
            return None;
        }
        // report the counts, and the blocked link when there is one.
        Some(Hit {
            weight,
            reason: format!(
                "linked accounts: {on_device} on this device, {on_ip} on this IP{}",
                if near_blocked {
                    ", linked to a blocked account"
                } else {
                    ""
                }
            ),
        })
    }
}

// tests for the rules.
#[cfg(test)]
mod tests {
    // bring the rules and helpers into the tests.
    use super::*;
    use crate::event::{AccountEvent, AccountEventKind, sample_event};
    use crate::state::EntityState;

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
        assert!(
            VelocityRule
                .check(&sample_event(1_000, 10.0), &UserState::default(), &config)
                .is_none()
        );
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
        assert!(
            AmountAnomalyRule
                .check(&sample_event(0, 25.0), &state, &config)
                .is_none()
        );
    }

    // builds a device or IP record that has seen the given accounts at time zero.
    fn entity_with(users: &[&str]) -> EntityState {
        // start empty.
        let mut entity = EntityState::default();
        // record every account.
        for user in users {
            // add one account.
            entity.record(id_hash(user), 0, false);
        }
        // hand back the filled record.
        entity
    }

    // any blocked place fires with the heavy weight, an expired block does not.
    #[test]
    fn blocklist_rule_names_the_blocked_places() {
        // default settings.
        let config = Config::default();
        // the device is blocked until time 1000.
        let mut entities = Entities::default();
        // set the block.
        entities.device.blocked_until = Some(1_000);
        // a transaction at time 100 is inside the block.
        let hit = BlocklistRule
            .check(
                &sample_event(100, 10.0),
                &UserState::default(),
                &entities,
                &config,
            )
            .unwrap();
        // the weight is the blocklist weight.
        assert_eq!(hit.weight, 100);
        // the reason names the device.
        assert!(hit.reason.contains("device"));
        // at time 1000 the block has ended.
        assert!(
            BlocklistRule
                .check(
                    &sample_event(1_000, 10.0),
                    &UserState::default(),
                    &entities,
                    &config
                )
                .is_none()
        );
        // a blocked account fires too and is named.
        let user = UserState {
            blocked_until: Some(1_000),
            ..UserState::default()
        };
        // check with clean entities.
        let hit = BlocklistRule
            .check(
                &sample_event(100, 10.0),
                &user,
                &Entities::default(),
                &config,
            )
            .unwrap();
        // the reason names the account.
        assert!(hit.reason.contains("account"));
    }

    // a shared device is strong, a shared IP is weak, old links and the account itself do not count.
    #[test]
    fn linked_accounts_rule_weights_device_strong_and_ip_weak() {
        // default settings.
        let config = Config::default();
        // a user without history, the entities carry all the information.
        let user = UserState::default();
        // the transaction is from account u1.
        let event = sample_event(100, 10.0);
        // nothing shared gives nothing.
        assert!(
            LinkedAccountsRule
                .check(&event, &user, &Entities::default(), &config)
                .is_none()
        );
        // three other accounts on the device make four with u1, so three extra at 15.
        let device = Entities {
            device: entity_with(&["a", "b", "c"]),
            ip: EntityState::default(),
        };
        // the device weight is 45.
        assert_eq!(
            LinkedAccountsRule
                .check(&event, &user, &device, &config)
                .unwrap()
                .weight,
            45
        );
        // five other accounts on the IP make six with u1, so three above the free three, at 3.
        let ip = Entities {
            device: EntityState::default(),
            ip: entity_with(&["a", "b", "c", "d", "e"]),
        };
        // the IP weight is 9.
        assert_eq!(
            LinkedAccountsRule
                .check(&event, &user, &ip, &config)
                .unwrap()
                .weight,
            9
        );
        // u1 already on the device is not counted twice, so one other account gives one extra at 15.
        let known = Entities {
            device: entity_with(&["u1", "a"]),
            ip: EntityState::default(),
        };
        // the weight is 15.
        assert_eq!(
            LinkedAccountsRule
                .check(&event, &user, &known, &config)
                .unwrap()
                .weight,
            15
        );
        // links older than 7 days are forgotten, so a transaction 8 days later gives nothing.
        let late = sample_event(8 * 86_400, 10.0);
        // no hit.
        assert!(
            LinkedAccountsRule
                .check(&late, &user, &device, &config)
                .is_none()
        );
    }

    // a blocked account on the same device adds the extra weight even without a crowd.
    #[test]
    fn linked_accounts_rule_adds_weight_near_a_blocked_account() {
        // default settings.
        let config = Config::default();
        // the device saw a blocked account at time 50.
        let mut entities = Entities::default();
        // set the mark.
        entities.device.blocked_account_at = Some(50);
        // a transaction at time 100.
        let hit = LinkedAccountsRule
            .check(
                &sample_event(100, 10.0),
                &UserState::default(),
                &entities,
                &config,
            )
            .unwrap();
        // the weight is the blocked link weight.
        assert_eq!(hit.weight, 30);
        // the reason mentions the blocked account.
        assert!(hit.reason.contains("blocked account"));
    }

    // builds one piece of account activity for tests.
    fn account_event(kind: AccountEventKind, timestamp: i64) -> AccountEvent {
        // the user id is a fixed placeholder.
        AccountEvent {
            user_id: "u1".to_string(),
            timestamp,
            kind,
        }
    }

    // signals count only when at least two fire together, and each extra one adds weight.
    #[test]
    fn takeover_rule_needs_two_signals_together() {
        // default settings.
        let config = Config::default();
        // a user who always spends 20 from device d1.
        let mut state = state_with(&[20.0; 6]);
        // three failed logins shortly before the payment.
        for _ in 0..3 {
            // add one failed login.
            state.record_account(&account_event(AccountEventKind::FailedLogin, 5));
        }
        // a normal payment on the known device has only the failed logins signal.
        let mut event = sample_event(10, 25.0);
        // one signal alone gives nothing.
        assert!(AccountTakeoverRule.check(&event, &state, &config).is_none());
        // a new device adds a second signal.
        event.device_id = "d2".to_string();
        // two signals give the base weight.
        assert_eq!(
            AccountTakeoverRule
                .check(&event, &state, &config)
                .unwrap()
                .weight,
            30
        );
        // a large amount adds a third signal.
        event.amount = 500.0;
        // the hit now has one extra step and names the large amount.
        let hit = AccountTakeoverRule.check(&event, &state, &config).unwrap();
        // 30 plus one extra step of 15.
        assert_eq!(hit.weight, 45);
        // the reason lists the fired signals.
        assert!(hit.reason.contains("large amount"));
    }

    // a password change counts for one day and then stops counting.
    #[test]
    fn takeover_rule_forgets_old_password_changes() {
        // default settings.
        let config = Config::default();
        // a user with history and a password change at time 1000.
        let mut state = state_with(&[20.0; 6]);
        // record the change.
        state.record_account(&account_event(AccountEventKind::PasswordChanged, 1_000));
        // a payment from a new device shortly after.
        let mut event = sample_event(2_000, 25.0);
        // set the new device.
        event.device_id = "d2".to_string();
        // password change plus new device are two signals.
        assert_eq!(
            AccountTakeoverRule
                .check(&event, &state, &config)
                .unwrap()
                .weight,
            30
        );
        // a full day after the change the signal is gone, so only the new device remains.
        event.timestamp = 1_000 + 86_400;
        // one signal gives nothing.
        assert!(AccountTakeoverRule.check(&event, &state, &config).is_none());
    }

    // an unknown device fires, a larger amount raises the weight, known and first devices do not fire.
    #[test]
    fn new_device_rule_scores_unknown_devices() {
        // default settings.
        let config = Config::default();
        // a user who always spends 20 from device d1.
        let state = state_with(&[20.0; 6]);
        // the same user on a device not seen before with a normal amount.
        let mut event = sample_event(0, 25.0);
        // set the new device.
        event.device_id = "d2".to_string();
        // the base weight applies.
        assert_eq!(
            NewDeviceRule.check(&event, &state, &config).unwrap().weight,
            20
        );
        // a large amount on the new device adds the bonus.
        event.amount = 500.0;
        // base plus bonus.
        assert_eq!(
            NewDeviceRule.check(&event, &state, &config).unwrap().weight,
            40
        );
        // the known device does not fire.
        assert!(
            NewDeviceRule
                .check(&sample_event(0, 25.0), &state, &config)
                .is_none()
        );
        // a user with no devices yet does not fire.
        assert!(
            NewDeviceRule
                .check(&event, &UserState::default(), &config)
                .is_none()
        );
    }

    // a far city in one hour fires, the same trip over a day does not, unknown cities are ignored.
    #[test]
    fn impossible_travel_rule_checks_speed_between_cities() {
        // default settings.
        let config = Config::default();
        // a transaction in New York at time zero.
        let mut first = sample_event(0, 10.0);
        // set the city.
        first.city = Some("New York".to_string());
        // remember it.
        let mut state = UserState::default();
        // record the first transaction.
        state.record(&first);
        // Los Angeles one hour later.
        let mut next = sample_event(3_600, 10.0);
        // set the city.
        next.city = Some("Los Angeles".to_string());
        // about 3936 km in one hour is not possible.
        assert_eq!(
            ImpossibleTravelRule
                .check(&next, &state, &config)
                .unwrap()
                .weight,
            60
        );
        // the same trip a day later is possible.
        next.timestamp = 86_400;
        // no hit.
        assert!(ImpossibleTravelRule.check(&next, &state, &config).is_none());
        // a city outside the table is ignored.
        next.city = Some("Nowhere".to_string());
        // no hit.
        assert!(ImpossibleTravelRule.check(&next, &state, &config).is_none());
        // the same city is a zero distance.
        next.city = Some("New York".to_string());
        // no hit.
        assert!(ImpossibleTravelRule.check(&next, &state, &config).is_none());
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
        assert!(
            AccountAgeRule
                .check(&event, &UserState::default(), &config)
                .is_none()
        );
        // no signup time and no history gives nothing.
        assert!(
            AccountAgeRule
                .check(&sample_event(100_000, 10.0), &UserState::default(), &config)
                .is_none()
        );
    }
}
