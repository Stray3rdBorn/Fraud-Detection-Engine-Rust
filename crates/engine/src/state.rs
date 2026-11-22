// the transaction type that gets recorded into the state.
use crate::event::TransactionEvent;

// number of time slots in the velocity window.
const SLOTS: usize = 12;
// length of one slot in seconds, 12 slots of 5 minutes cover one hour.
const SLOT_SECONDS: i64 = 300;

// counts transactions per 5 minute slot, a slot starts again when time reaches it a second time.
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

// everything the engine remembers about one user between transactions.
#[derive(Debug, Clone, Default)]
pub struct UserState {
    // time of the first transaction seen for this user, empty until then.
    pub first_seen: Option<i64>,
    // transactions per 5 minute slot for the last hour.
    pub velocity: Velocity,
    // running average and spread of this user's amounts.
    pub amounts: AmountStats,
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
    }
}

// tests for the state types.
#[cfg(test)]
mod tests {
    // bring the types above into the tests.
    use super::*;

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
}