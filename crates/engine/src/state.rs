// the transaction type that gets recorded into the state, and the city table.
use crate::{
    event::{AccountEvent, AccountEventKind, TransactionEvent},
    geo,
};

// number of time slots in the velocity window.
const SLOTS: usize = 12;
// length of one slot in seconds, 12 slots of 5 minutes cover one hour.
const SLOT_SECONDS: i64 = 300;
// most devices remembered per user, the oldest is forgotten when the list is full.
const MAX_DEVICES: usize = 5;
// most accounts remembered per device or IP, the oldest is forgotten when the list is full.
const LINK_CAP: usize = 50;

// counts events per 5 minute slot, a slot starts again when time reaches it a second time.
#[derive(Debug, Clone, Default)]
pub struct Velocity {
    // each slot stores the id of the time slot it belongs to and the count seen in it.
    slots: [(i64, u32); SLOTS],
}

impl Velocity {
    // number of transactions in the last hour, ending at the given time.
    pub fn count(&self, now: i64) -> u32 {
        // id of the time slot that the given time falls into.
        let current = now.div_euclid(SLOT_SECONDS);
        // go through all slots.
        self.slots
            .iter()
            // keep only the slots that are inside the last hour.
            .filter(|(id, _)| *id > current - SLOTS as i64 && *id <= current)
            // take the count of each remaining slot.
            .map(|(_, count)| count)
            // add the counts together.
            .sum()
    }

    // adds one transaction at the given time.
    pub fn record(&mut self, now: i64) {
        // id of the time slot that the given time falls into.
        let id = now.div_euclid(SLOT_SECONDS);
        // the slot position is the id modulo the number of slots.
        let slot = &mut self.slots[id.rem_euclid(SLOTS as i64) as usize];
        // a slot that belongs to an older time slot is stale, so it starts again.
        if slot.0 != id {
            // reset the slot to the new id with a count of zero.
            *slot = (id, 0);
        }
        // count this transaction.
        slot.1 += 1;
    }
}

// running average and spread of a user's amounts, three numbers instead of every amount.
#[derive(Debug, Clone, Default)]
pub struct AmountStats {
    // how many amounts have been recorded.
    count: u64,
    // average of the recorded amounts.
    mean: f64,
    // sum of squared distances from the average, used to get the spread.
    m2: f64,
}

impl AmountStats {
    // how many amounts have been recorded.
    pub fn count(&self) -> u64 {
        // the stored count.
        self.count
    }

    // average of the recorded amounts.
    pub fn mean(&self) -> f64 {
        // the stored average.
        self.mean
    }

    // standard deviation, the typical distance of an amount from the average.
    pub fn std_dev(&self) -> f64 {
        // fewer than two amounts have no spread.
        if self.count < 2 {
            // no spread to report.
            return 0.0;
        }
        // divide by one less than the count and take the square root.
        (self.m2 / (self.count - 1) as f64).sqrt()
    }

    // how many spreads an amount sits above the average, the spread never counts below the floor.
    pub fn z(&self, amount: f64, min_spread: f64) -> f64 {
        // distance from the average divided by the spread.
        (amount - self.mean) / self.std_dev().max(min_spread)
    }

    // adds one amount, this is Welford's running update.
    pub fn record(&mut self, amount: f64) {
        // count the new amount.
        self.count += 1;
        // distance of the new amount from the old average.
        let delta = amount - self.mean;
        // move the average toward the new amount.
        self.mean += delta / self.count as f64;
        // grow the squared distance sum using the old and the new average.
        self.m2 += delta * (amount - self.mean);
    }
}

// devices a user has used before, capped in size.
#[derive(Debug, Clone, Default)]
pub struct KnownDevices(Vec<String>);

impl KnownDevices {
    // true when no device has been seen yet.
    pub fn is_empty(&self) -> bool {
        // the list has no entries.
        self.0.is_empty()
    }

    // true when this device has been used before.
    pub fn contains(&self, device: &str) -> bool {
        // look for an equal id in the list.
        self.0.iter().any(|known| known == device)
    }

    // adds a device, forgetting the oldest one when the list is full.
    pub fn remember(&mut self, device: &str) {
        // a known device changes nothing.
        if self.contains(device) {
            // nothing to add.
            return;
        }
        // a full list drops its oldest entry, which sits at the front.
        if self.0.len() == MAX_DEVICES {
            // remove the oldest device.
            self.0.remove(0);
        }
        // add the new device at the back.
        self.0.push(device.to_string());
    }
}

// the last known position of a user and when it was seen.
#[derive(Debug, Clone, Copy)]
pub struct Place {
    // latitude and longitude in degrees.
    pub pos: (f64, f64),
    // time of that transaction in unix seconds.
    pub time: i64,
}

// everything the engine remembers about one user between transactions.
#[derive(Debug, Clone, Default)]
pub struct UserState {
    // time of the first transaction seen for this user, empty until then.
    pub first_seen: Option<i64>,
    // transactions per 5 minute slot for the last hour.
    pub velocity: Velocity,
    // running average and spread of this user's amounts.
    pub amounts: AmountStats,
    // devices this user has used before.
    pub devices: KnownDevices,
    // last position that could be placed on the map, empty until then.
    pub last_place: Option<Place>,
    // failed logins per 5 minute slot for the last hour.
    pub failed_logins: Velocity,
    // time of the last password change, empty until one happens.
    pub password_changed_at: Option<i64>,
    // time until which this account is blocked, empty when it is not.
    pub blocked_until: Option<i64>,
}

impl UserState {
    // adds one transaction to the remembered state.
    pub fn record(&mut self, event: &TransactionEvent) {
        // remember the first time this user was seen, later transactions keep it.
        self.first_seen.get_or_insert(event.timestamp);
        // count the transaction in its time slot.
        self.velocity.record(event.timestamp);
        // add the amount to the running statistics.
        self.amounts.record(event.amount);
        // remember the device.
        self.devices.remember(&event.device_id);
        // a city that is not in the table never replaces the last known position.
        if let Some(pos) = event.city.as_deref().and_then(geo::lookup) {
            // store the position with the time.
            self.last_place = Some(Place {
                pos,
                time: event.timestamp,
            });
        }
    }

    // true when the account is on the blocklist at the given time.
    pub fn is_blocked(&self, now: i64) -> bool {
        // a block counts until its end time.
        self.blocked_until.is_some_and(|until| until > now)
    }

    // adds one piece of account activity to the remembered state.
    pub fn record_account(&mut self, event: &AccountEvent) {
        // each kind of activity updates its own field.
        match event.kind {
            // count the failed login in its time slot.
            AccountEventKind::FailedLogin => self.failed_logins.record(event.timestamp),
            // remember when the password changed.
            AccountEventKind::PasswordChanged => self.password_changed_at = Some(event.timestamp),
        }
    }
}

// what is remembered about one device or one IP, shared by all accounts that use it.
#[derive(Debug, Clone, Default)]
pub struct EntityState {
    // hashed ids of the accounts seen here with the time each was last seen, capped in size.
    accounts: Vec<(u64, i64)>,
    // time until which this device or IP is blocked, empty when it is not.
    pub blocked_until: Option<i64>,
    // last time an account that was blocked used this device or IP.
    pub blocked_account_at: Option<i64>,
}

impl EntityState {
    // true when this device or IP is on the blocklist at the given time.
    pub fn is_blocked(&self, now: i64) -> bool {
        // a block counts until its end time.
        self.blocked_until.is_some_and(|until| until > now)
    }

    // true when a blocked account used this device or IP inside the window.
    pub fn has_blocked_account(&self, now: i64, window: i64) -> bool {
        // the sighting must be newer than the start of the window.
        self.blocked_account_at
            .is_some_and(|seen| seen > now.saturating_sub(window))
    }

    // number of different accounts seen inside the window, the given account is counted once.
    pub fn accounts_with(&self, user: u64, now: i64, window: i64) -> u32 {
        // accounts seen after this time are inside the window.
        let start = now.saturating_sub(window);
        // count of accounts inside the window.
        let mut count = 0;
        // whether the given account is one of them.
        let mut has_user = false;
        // go through everything remembered here.
        for &(account, seen) in &self.accounts {
            // old sightings are ignored.
            if seen > start {
                // one more account.
                count += 1;
                // remember if it is the given account.
                has_user |= account == user;
            }
        }
        // the given account counts even when it has not been seen here yet.
        count + u32::from(!has_user)
    }

    // records that an account used this device or IP.
    pub fn record(&mut self, user: u64, now: i64, user_blocked: bool) {
        // an account that is already known only gets a newer time, an older time never replaces a newer one.
        if let Some(entry) = self
            .accounts
            .iter_mut()
            .find(|(account, _)| *account == user)
        {
            // keep the later time.
            entry.1 = entry.1.max(now);
        } else {
            // a full list makes room by dropping the account seen longest ago.
            if self.accounts.len() == LINK_CAP {
                // position of the oldest sighting.
                if let Some(oldest) = self
                    .accounts
                    .iter()
                    .enumerate()
                    .min_by_key(|(_, (_, seen))| *seen)
                    .map(|(index, _)| index)
                {
                    // remove it, the order of the list does not matter.
                    self.accounts.swap_remove(oldest);
                }
            }
            // add the new account.
            self.accounts.push((user, now));
        }
        // remember when a blocked account was last here.
        if user_blocked {
            // store the time.
            self.blocked_account_at = Some(now);
        }
    }
}

// the device and the IP of one transaction.
#[derive(Debug, Clone, Default)]
pub struct Entities {
    // what is remembered about the device.
    pub device: EntityState,
    // what is remembered about the IP address.
    pub ip: EntityState,
}

impl Entities {
    // records the account on both the device and the IP.
    pub fn record(&mut self, user: u64, now: i64, user_blocked: bool) {
        // update the device.
        self.device.record(user, now, user_blocked);
        // update the IP.
        self.ip.record(user, now, user_blocked);
    }
}

// tests for the state types.
#[cfg(test)]
mod tests {
    // bring the types above into the tests.
    use super::*;
    use crate::event::sample_event;

    // transactions count for an hour, then they are forgotten.
    #[test]
    fn velocity_counts_recent_slots_and_forgets_old_ones() {
        // start with an empty counter.
        let mut velocity = Velocity::default();
        // record two transactions at the same time.
        velocity.record(10_000);
        velocity.record(10_000);
        // both count right away.
        assert_eq!(velocity.count(10_000), 2);
        // half an hour later both are still inside the hour.
        assert_eq!(velocity.count(10_000 + 1_800), 2);
        // two hours later both are outside the hour.
        assert_eq!(velocity.count(10_000 + 7_200), 0);
        // one hour later the same slot is reused and the old count is replaced.
        velocity.record(10_000 + 3_600);
        // only the new transaction counts.
        assert_eq!(velocity.count(10_000 + 3_600), 1);
    }

    // the running statistics match a known example.
    #[test]
    fn amount_stats_match_known_values() {
        // start with empty statistics.
        let mut stats = AmountStats::default();
        // record a set of amounts with an average of 5.
        for amount in [2.0, 4.0, 4.0, 4.0, 5.0, 5.0, 7.0, 9.0] {
            // add one amount.
            stats.record(amount);
        }
        // the count matches the number of amounts.
        assert_eq!(stats.count(), 8);
        // the average is 5.
        assert!((stats.mean() - 5.0).abs() < 1e-9);
        // the sample standard deviation is the square root of 32 divided by 7.
        assert!((stats.std_dev() - 2.138089935).abs() < 1e-6);
    }
    // the device list keeps at most five devices and forgets the oldest first.
    #[test]
    fn known_devices_are_capped_and_forget_the_oldest() {
        // start with an empty list.
        let mut devices = KnownDevices::default();
        // remember six devices, one more than the cap.
        for id in ["a", "b", "c", "d", "e", "f"] {
            // add one device.
            devices.remember(id);
        }
        // the oldest is gone.
        assert!(!devices.contains("a"));
        // the newest is there.
        assert!(devices.contains("f"));
        // remembering a known device changes nothing.
        devices.remember("b");
        // the next oldest is still there.
        assert!(devices.contains("c"));
    }

    // a city outside the table never replaces the last known position.
    #[test]
    fn unknown_city_keeps_the_last_place() {
        // start with an empty state.
        let mut state = UserState::default();
        // a transaction in a known city.
        let mut event = sample_event(100, 10.0);
        // set the city.
        event.city = Some("Houston".to_string());
        // record it.
        state.record(&event);
        // a later transaction in a city that is not in the table.
        event.city = Some("Nowhere".to_string());
        // set the later time.
        event.timestamp = 200;
        // record it.
        state.record(&event);
        // the stored position still carries the first time.
        assert_eq!(state.last_place.unwrap().time, 100);
    }
    // account activity updates the failed login counter and the password time, not the first seen time.
    #[test]
    fn account_events_update_the_state() {
        // start with an empty state.
        let mut state = UserState::default();
        // one failed login at time 100.
        state.record_account(&AccountEvent {
            user_id: "u1".to_string(),
            timestamp: 100,
            kind: AccountEventKind::FailedLogin,
        });
        // one password change at time 200.
        state.record_account(&AccountEvent {
            user_id: "u1".to_string(),
            timestamp: 200,
            kind: AccountEventKind::PasswordChanged,
        });
        // the failed login is counted.
        assert_eq!(state.failed_logins.count(100), 1);
        // the password time is stored.
        assert_eq!(state.password_changed_at, Some(200));
        // account activity is not a transaction, so the first seen time stays empty.
        assert_eq!(state.first_seen, None);
    }
    // distinct accounts are counted inside the window, and the given account is never counted twice.
    #[test]
    fn entity_counts_distinct_accounts_in_the_window() {
        // start with an empty entity.
        let mut entity = EntityState::default();
        // account 1 at time 100, account 2 at time 200, account 1 again at time 300.
        entity.record(1, 100, false);
        entity.record(2, 200, false);
        entity.record(1, 300, false);
        // two accounts are known and account 1 is one of them.
        assert_eq!(entity.accounts_with(1, 300, 1_000), 2);
        // a new account 3 would make three.
        assert_eq!(entity.accounts_with(3, 300, 1_000), 3);
        // with a short window only the sighting at time 300 is inside, so account 3 would make two.
        assert_eq!(entity.accounts_with(3, 300, 50), 2);
    }

    // the list never grows past the cap and the oldest account is dropped first.
    #[test]
    fn entity_list_is_capped_and_forgets_the_oldest() {
        // start with an empty entity.
        let mut entity = EntityState::default();
        // record 60 accounts, each one second after the previous.
        for id in 0..60u64 {
            // add one account.
            entity.record(id, id as i64, false);
        }
        // only 50 are kept.
        assert_eq!(entity.accounts.len(), 50);
        // the first account is gone.
        assert!(!entity.accounts.iter().any(|(account, _)| *account == 0));
    }

    // a blocked account leaves a mark and the blocklist expires.
    #[test]
    fn entity_tracks_blocked_accounts_and_expiry() {
        // start with an empty entity.
        let mut entity = EntityState::default();
        // a blocked account uses the entity at time 100.
        entity.record(1, 100, true);
        // the mark is visible inside the window.
        assert!(entity.has_blocked_account(200, 1_000));
        // the mark is gone once the window has passed.
        assert!(!entity.has_blocked_account(2_000, 1_000));
        // a block that ends at 500 holds before 500 and not at 500.
        entity.blocked_until = Some(500);
        // still blocked.
        assert!(entity.is_blocked(499));
        // expired.
        assert!(!entity.is_blocked(500));
    }
}
