// lets the settings be read from a file.
use serde::Deserialize;

// largest weight any single setting may carry, keeps the sum of all weights far from overflowing.
const MAX_WEIGHT: u32 = 10_000;

// all weights and thresholds in one place, so tuning never touches the rules.
// a file may set only some values, the rest keep their defaults, and a misspelled name is an error.
#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    // a score at or above this sends the transaction to review.
    pub review_threshold: u32,
    // a score at or above this blocks the transaction.
    pub block_threshold: u32,
    // transactions in the last hour that cost no weight.
    pub velocity_free: u32,
    // weight added for every transaction above the free amount.
    pub velocity_per_extra: u32,
    // highest weight the velocity rule can add.
    pub velocity_max_weight: u32,
    // earlier transactions a user needs before the amount rule trusts the average.
    pub amount_min_history: u64,
    // small weight for users who do not have enough history yet.
    pub amount_cold_weight: u32,
    // how many spreads above the average an amount must be before it counts.
    pub amount_z_start: f64,
    // weight added for every spread above the starting point.
    pub amount_per_z: f64,
    // highest weight the amount rule can add.
    pub amount_max_weight: u32,
    // smallest spread allowed, so identical past amounts do not give huge scores.
    pub amount_min_spread: f64,
    // accounts younger than this many seconds count as new.
    pub account_age_window: i64,
    // highest weight the account age rule can add, reached at an age of zero.
    pub account_age_max_weight: u32,
    // weight added when a known user pays from a device not seen before.
    pub new_device_weight: u32,
    // extra weight added when that payment is also unusually large.
    pub new_device_large_bonus: u32,
    // fastest believable travel speed in kilometers per hour, about a passenger plane.
    pub travel_max_kmh: f64,
    // moves shorter than this many kilometers are ignored.
    pub travel_min_km: f64,
    // weight added when the speed between two cities is not possible.
    pub travel_weight: u32,
    // failed logins in the last hour that count as a takeover signal.
    pub takeover_failed_logins: u32,
    // seconds after a password change during which the change counts as a signal.
    pub takeover_password_window: i64,
    // signals that must fire together before the takeover rule adds weight.
    pub takeover_min_signals: u32,
    // weight added when the minimum number of signals fire together.
    pub takeover_weight: u32,
    // extra weight for every signal above the minimum.
    pub takeover_per_extra: u32,
    // weight added when the account, the device or the IP is on the blocklist.
    pub blocklist_weight: u32,
    // seconds an account stays linked to a device or IP after it was last seen there, 7 days.
    pub link_window: i64,
    // accounts on one device that cost no weight, the user alone.
    pub device_free_accounts: u32,
    // weight added for every further account on the same device.
    pub device_per_account: u32,
    // highest weight the device link can add.
    pub device_max_weight: u32,
    // accounts on one IP that cost no weight, homes and offices share IPs.
    pub ip_free_accounts: u32,
    // weight added for every further account on the same IP.
    pub ip_per_account: u32,
    // highest weight the IP link can add.
    pub ip_max_weight: u32,
    // extra weight when a blocked account used the same device or IP inside the window.
    pub linked_blocked_weight: u32,
    // transactions per hour on one device that cost no weight, a shared family device is normal.
    pub device_velocity_free: u32,
    // weight added for every further transaction on the same device in the hour.
    pub device_velocity_per_extra: u32,
    // highest weight the device velocity can add.
    pub device_velocity_max_weight: u32,
    // transactions per hour on one IP that cost no weight, offices and mobile networks share IPs.
    pub ip_velocity_free: u32,
    // weight added for every further transaction on the same IP in the hour.
    pub ip_velocity_per_extra: u32,
    // highest weight the IP velocity can add.
    pub ip_velocity_max_weight: u32,
}

impl Config {
    // checks that the values make sense, a bad file must stop the server before it decides anything.
    pub fn validate(&self) -> Result<(), String> {
        // every weight with its name, so one check covers them all.
        let weights = [
            ("velocity_per_extra", self.velocity_per_extra),
            ("velocity_max_weight", self.velocity_max_weight),
            ("amount_cold_weight", self.amount_cold_weight),
            ("amount_max_weight", self.amount_max_weight),
            ("account_age_max_weight", self.account_age_max_weight),
            ("new_device_weight", self.new_device_weight),
            ("new_device_large_bonus", self.new_device_large_bonus),
            ("travel_weight", self.travel_weight),
            ("takeover_weight", self.takeover_weight),
            ("takeover_per_extra", self.takeover_per_extra),
            ("blocklist_weight", self.blocklist_weight),
            ("device_per_account", self.device_per_account),
            ("device_max_weight", self.device_max_weight),
            ("ip_per_account", self.ip_per_account),
            ("ip_max_weight", self.ip_max_weight),
            ("linked_blocked_weight", self.linked_blocked_weight),
        ];
        // no weight may be above the limit.
        for (name, weight) in weights {
            // report the first one that is too big.
            if weight > MAX_WEIGHT {
                return Err(format!("{name} must be at most {MAX_WEIGHT}"));
            }
        }
        // numbers that must be above zero, a zero would divide by zero.
        let positive = [
            ("amount_min_spread", self.amount_min_spread),
            ("travel_max_kmh", self.travel_max_kmh),
        ];
        // NaN and infinity fail this check too.
        for (name, value) in positive {
            // report the first bad one.
            if !(value.is_finite() && value > 0.0) {
                return Err(format!("{name} must be a number above 0"));
            }
        }
        // numbers that may be zero but never negative.
        let not_negative = [
            ("amount_z_start", self.amount_z_start),
            ("amount_per_z", self.amount_per_z),
            ("travel_min_km", self.travel_min_km),
        ];
        // NaN and infinity fail this check too.
        for (name, value) in not_negative {
            // report the first bad one.
            if !(value.is_finite() && value >= 0.0) {
                return Err(format!("{name} must be a number of 0 or more"));
            }
        }
        // time windows must have a length.
        let windows = [
            ("account_age_window", self.account_age_window),
            ("link_window", self.link_window),
            ("takeover_password_window", self.takeover_password_window),
        ];
        // a window of zero or less would never match anything.
        for (name, seconds) in windows {
            // report the first bad one.
            if seconds <= 0 {
                return Err(format!("{name} must be above 0"));
            }
        }
        // review must come first, and a threshold of zero would review everything.
        if self.review_threshold == 0 || self.review_threshold > self.block_threshold {
            return Err("review_threshold must be above 0 and not above block_threshold".to_string());
        }
        // the standard deviation needs at least two amounts.
        if self.amount_min_history < 2 {
            return Err("amount_min_history must be at least 2".to_string());
        }
        // takeover needs several signals together, and only four exist.
        if !(2..=4).contains(&self.takeover_min_signals) {
            return Err("takeover_min_signals must be between 2 and 4".to_string());
        }
        // everything is fine.
        Ok(())
    }
}

// starting values, they are tuned later with the threshold sweep on the test data.
impl Default for Config {
    fn default() -> Self {
        Config {
            // a score of 40 sends the transaction to review.
            review_threshold: 40,
            // a score of 80 blocks the transaction.
            block_threshold: 80,
            // up to 5 transactions in an hour are normal.
            velocity_free: 5,
            // every transaction above 5 adds 4.
            velocity_per_extra: 4,
            // velocity alone never adds more than 40.
            velocity_max_weight: 40,
            // 5 earlier transactions are needed for a baseline.
            amount_min_history: 5,
            // users without a baseline get a small caution of 10.
            amount_cold_weight: 10,
            // an amount 3 spreads above the average starts to count.
            amount_z_start: 3.0,
            // each extra spread adds 10.
            amount_per_z: 10.0,
            // the amount rule never adds more than 40.
            amount_max_weight: 40,
            // the spread never counts as smaller than 5 currency units.
            amount_min_spread: 5.0,
            // accounts younger than one hour count as new.
            account_age_window: 3600,
            // a brand new account adds 40.
            account_age_max_weight: 40,
            // a new device adds 20.
            new_device_weight: 20,
            // a large amount on a new device adds another 20.
            new_device_large_bonus: 20,
            // nobody travels faster than 900 km per hour.
            travel_max_kmh: 900.0,
            // moves under 100 km are ignored, city centers are not exact.
            travel_min_km: 100.0,
            // impossible travel alone sends the transaction to review.
            travel_weight: 60,
            // 3 failed logins in an hour is a signal.
            takeover_failed_logins: 3,
            // a password change counts for one day.
            takeover_password_window: 86_400,
            // two signals together are needed.
            takeover_min_signals: 2,
            // two signals together add 30.
            takeover_weight: 30,
            // every further signal adds 15, so all four reach 60.
            takeover_per_extra: 15,
            // a blocklist hit alone is enough to block.
            blocklist_weight: 100,
            // links last 7 days.
            link_window: 604_800,
            // one account on a device is normal.
            device_free_accounts: 1,
            // every further account on a device adds 15, a strong signal.
            device_per_account: 15,
            // the device link never adds more than 60.
            device_max_weight: 60,
            // up to 3 accounts on an IP are normal.
            ip_free_accounts: 3,
            // every further account on an IP adds 3, a weak signal.
            ip_per_account: 3,
            // the IP link never adds more than 20.
            ip_max_weight: 20,
            // a link to a blocked account adds 30.
            linked_blocked_weight: 30,
            // up to 10 transactions an hour on a device are normal.
            device_velocity_free: 10,
            // every further transaction on a device adds 3.
            device_velocity_per_extra: 3,
            // the device velocity never adds more than 30.
            device_velocity_max_weight: 30,
            // up to 20 transactions an hour on an IP are normal.
            ip_velocity_free: 20,
            // every further transaction on an IP adds 1, a weak signal.
            ip_velocity_per_extra: 1,
            // the IP velocity never adds more than 15.
            ip_velocity_max_weight: 15,
        }
    }
}

// tests for the settings.
#[cfg(test)]
mod tests {
    // bring the settings into the tests.
    use super::*;

    // the defaults are valid, and each kind of bad value is caught.
    #[test]
    fn validate_accepts_defaults_and_rejects_bad_values() {
        // the defaults pass.
        assert!(Config::default().validate().is_ok());
        // changes the default settings with the given edit and says whether validation fails.
        let fails = |edit: fn(&mut Config)| {
            // start from the defaults.
            let mut config = Config::default();
            // apply the edit.
            edit(&mut config);
            // the result of the check.
            config.validate().is_err()
        };
        // block below review.
        assert!(fails(|c| c.block_threshold = 10));
        // review at zero.
        assert!(fails(|c| c.review_threshold = 0));
        // a spread floor of zero.
        assert!(fails(|c| c.amount_min_spread = 0.0));
        // a number that is not a number.
        assert!(fails(|c| c.amount_min_spread = f64::NAN));
        // a window of zero.
        assert!(fails(|c| c.link_window = 0));
        // a weight above the limit.
        assert!(fails(|c| c.travel_weight = 10_001));
        // too few signals.
        assert!(fails(|c| c.takeover_min_signals = 1));
        // too little history.
        assert!(fails(|c| c.amount_min_history = 1));
        // a negative minimum distance.
        assert!(fails(|c| c.travel_min_km = -1.0));
    }
}
