// all weights and thresholds in one place, so tuning never touches the rules.
#[derive(Debug, Clone)]
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
