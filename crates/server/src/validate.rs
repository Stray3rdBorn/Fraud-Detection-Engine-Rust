// the error type of the server.
use crate::error::ApiError;
// the two kinds of input that are checked.
use engine::{AccountEvent, TransactionEvent};
// the type that checks and normalizes IP addresses.
use std::net::IpAddr;

// longest id accepted, in bytes.
const MAX_ID_LEN: usize = 128;
// longest analyst note accepted, in bytes.
const MAX_NOTE_LEN: usize = 500;
// longest city name accepted, in bytes.
const MAX_CITY_LEN: usize = 64;
// largest amount accepted, far above any real payment, keeps the statistics sane.
const MAX_AMOUNT: f64 = 1_000_000_000.0;

// a text field must be filled, short and free of control characters such as line breaks.
fn text(value: &str, max_len: usize, message: &'static str) -> Result<(), ApiError> {
    // any of the three problems makes the field invalid.
    if value.is_empty() || value.len() > max_len || value.chars().any(char::is_control) {
        // the message names the field.
        return Err(ApiError::Invalid(message));
    }
    // fine.
    Ok(())
}

// a time must not be negative and, when the clock check is on, must be near the server clock.
fn time(value: i64, now: i64, max_skew: Option<i64>) -> Result<(), ApiError> {
    // the unix time starts at zero.
    if value < 0 {
        // negative.
        return Err(ApiError::Invalid("timestamp must not be negative"));
    }
    // too far from the server clock, in either direction.
    if max_skew.is_some_and(|skew| value.abs_diff(now) > skew as u64) {
        // too far.
        return Err(ApiError::Invalid(
            "timestamp is too far from the server time",
        ));
    }
    // fine.
    Ok(())
}

// checks one transaction, and rewrites its IP address into one standard form.
pub fn transaction(
    event: &mut TransactionEvent,
    now: i64,
    max_skew: Option<i64>,
) -> Result<(), ApiError> {
    // the three ids.
    text(
        &event.event_id,
        MAX_ID_LEN,
        "event_id must be 1 to 128 characters without control characters",
    )?;
    // the user.
    text(
        &event.user_id,
        MAX_ID_LEN,
        "user_id must be 1 to 128 characters without control characters",
    )?;
    // the device.
    text(
        &event.device_id,
        MAX_ID_LEN,
        "device_id must be 1 to 128 characters without control characters",
    )?;
    // the IP must parse, and is stored in its standard written form so one address has one key.
    event.ip = event
        .ip
        .parse::<IpAddr>()
        .map_err(|_| ApiError::Invalid("ip must be a valid IPv4 or IPv6 address"))?
        .to_string();
    // the amount must be a normal number in range, NaN and infinity are not in any range.
    if !(0.0..=MAX_AMOUNT).contains(&event.amount) {
        // out of range.
        return Err(ApiError::Invalid("amount must be between 0 and 1000000000"));
    }
    // the time.
    time(event.timestamp, now, max_skew)?;
    // a signup time must lie between the start of time and the transaction.
    if event
        .signup_time
        .is_some_and(|signup| !(0..=event.timestamp).contains(&signup))
    {
        // wrong.
        return Err(ApiError::Invalid(
            "signup_time must not be negative or after the transaction",
        ));
    }
    // a city is optional, but when it is present it must be a normal short text.
    if let Some(city) = &event.city {
        // check it.
        text(
            city,
            MAX_CITY_LEN,
            "city must be 1 to 64 characters without control characters",
        )?;
    }
    // everything is fine.
    Ok(())
}

// checks one piece of account activity.
pub fn account(event: &AccountEvent, now: i64, max_skew: Option<i64>) -> Result<(), ApiError> {
    // the user.
    text(
        &event.user_id,
        MAX_ID_LEN,
        "user_id must be 1 to 128 characters without control characters",
    )?;
    // the time.
    time(event.timestamp, now, max_skew)
}

// checks the optional note of a verdict, a missing note is fine.
pub fn note(note: Option<&str>) -> Result<(), ApiError> {
    // only a note that is present is checked.
    if let Some(note) = note {
        // same rules as the other text fields.
        text(
            note,
            MAX_NOTE_LEN,
            "note must be 1 to 500 characters without control characters",
        )?;
    }
    // fine.
    Ok(())
}

// tests for the checks.
#[cfg(test)]
mod tests {
    // bring the checks and the types into the tests.
    use super::*;
    use engine::AccountEventKind;

    // the server clock used by all tests.
    const NOW: i64 = 1_700_000_000;

    // a transaction that passes every check.
    fn valid() -> TransactionEvent {
        // every field filled with a normal value.
        TransactionEvent {
            event_id: "e1".to_string(),
            user_id: "u1".to_string(),
            timestamp: NOW,
            amount: 25.0,
            device_id: "d1".to_string(),
            ip: "1.2.3.4".to_string(),
            signup_time: Some(NOW - 1_000),
            city: Some("Dhaka".to_string()),
        }
    }

    // runs the transaction check with the usual clock limit.
    fn check(event: &mut TransactionEvent) -> Result<(), ApiError> {
        // five minutes of allowed difference.
        transaction(event, NOW, Some(300))
    }

    // a normal transaction passes and its IP is written in standard form.
    #[test]
    fn a_normal_transaction_passes_and_the_ip_is_normalized() {
        // a good transaction.
        assert!(check(&mut valid()).is_ok());
        // an IPv6 address in capital letters.
        let mut event = valid();
        // set it.
        event.ip = "2001:DB8::1".to_string();
        // it passes.
        assert!(check(&mut event).is_ok());
        // and now has the standard lower case form.
        assert_eq!(event.ip, "2001:db8::1");
    }

    // each bad field is refused.
    #[test]
    fn bad_fields_are_refused() {
        // applies an edit to a good transaction and says whether the check refuses it.
        let refused = |edit: fn(&mut TransactionEvent)| {
            // start from a good transaction.
            let mut event = valid();
            // break it.
            edit(&mut event);
            // the result.
            check(&mut event).is_err()
        };
        // an empty id.
        assert!(refused(|e| e.user_id = String::new()));
        // a very long id.
        assert!(refused(|e| e.device_id = "x".repeat(129)));
        // a line break inside an id.
        assert!(refused(|e| e.event_id = "a\nb".to_string()));
        // an IP that is not an address.
        assert!(refused(|e| e.ip = "not an ip".to_string()));
        // a negative amount.
        assert!(refused(|e| e.amount = -1.0));
        // a number that is not a number.
        assert!(refused(|e| e.amount = f64::NAN));
        // an infinite amount.
        assert!(refused(|e| e.amount = f64::INFINITY));
        // an amount that is far too large.
        assert!(refused(|e| e.amount = 1e12));
        // a time ten minutes away from the server clock.
        assert!(refused(|e| e.timestamp = NOW + 600));
        // a negative time.
        assert!(refused(|e| e.timestamp = -5));
        // a signup after the transaction.
        assert!(refused(|e| e.signup_time = Some(NOW + 1)));
        // a very long city.
        assert!(refused(|e| e.city = Some("x".repeat(65))));
    }

    // the clock check can be turned off, so old data can be replayed.
    #[test]
    fn the_clock_check_can_be_turned_off() {
        // an old transaction.
        let mut event = valid();
        // set the time to three years earlier, and keep the signup before it.
        event.timestamp = NOW - 100_000_000;
        // the signup stays before it.
        event.signup_time = Some(event.timestamp - 10);
        // refused with the check on.
        assert!(transaction(&mut event, NOW, Some(300)).is_err());
        // accepted with the check off.
        assert!(transaction(&mut event, NOW, None).is_ok());
    }

    // account activity gets the same id and time checks.
    #[test]
    fn account_events_are_checked() {
        // a good event.
        let good = AccountEvent {
            user_id: "u1".to_string(),
            timestamp: NOW,
            kind: AccountEventKind::FailedLogin,
        };
        // it passes.
        assert!(account(&good, NOW, Some(300)).is_ok());
        // the same event too far from the clock.
        let late = AccountEvent {
            timestamp: NOW + 600,
            ..good
        };
        // it is refused.
        assert!(account(&late, NOW, Some(300)).is_err());
    }

        // a note is optional, but when present it must be short text.
    #[test]
    fn a_note_is_checked() {
        // no note.
        assert!(note(None).is_ok());
        // a normal note.
        assert!(note(Some("same card used on three devices")).is_ok());
        // an empty note.
        assert!(note(Some("")).is_err());
        // a very long note.
        assert!(note(Some(&"x".repeat(501))).is_err());
        // a note with a line break.
        assert!(note(Some("a\nb")).is_err());
    }
}
