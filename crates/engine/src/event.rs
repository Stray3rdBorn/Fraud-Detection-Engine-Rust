// serde traits that let these types be read from and written as JSON.
use serde::{Deserialize, Serialize};

// one transaction as sent by the payment system, working fields only.
#[derive(Debug, Deserialize)]
pub struct TransactionEvent {
    // unique id of this transaction.
    pub event_id: String,
    // id of the user who made the transaction.
    pub user_id: String,
    // time of the transaction in unix seconds.
    pub timestamp: i64,
    // amount of money moved.
    pub amount: f64,
    // id of the device used.
    pub device_id: String,
    // IP address the request came from.
    pub ip: String,
    // time the account was created in unix seconds, empty when the sender does not know it.
    pub signup_time: Option<i64>,
    // city of the transaction, empty when the sender does not know it.
    pub city: Option<String>,
}

// the kinds of account activity that are not payments.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AccountEventKind {
    // a wrong password was entered.
    FailedLogin,
    // the password of the account was changed.
    PasswordChanged,
}

// one piece of account activity as sent by the payment system.
#[derive(Debug, Deserialize)]
pub struct AccountEvent {
    // id of the user the activity belongs to.
    pub user_id: String,
    // time of the activity in unix seconds.
    pub timestamp: i64,
    // what happened.
    pub kind: AccountEventKind,
}

// the three possible results of a decision, written in lowercase in JSON.
#[derive(Debug, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Outcome {
    // the transaction goes through.
    Allow,
    // a human should look at the transaction.
    Review,
    // the transaction is stopped.
    Block,
}

// the full answer for one transaction.
#[derive(Debug, Serialize)]
pub struct Decision {
    // id of the transaction this decision belongs to.
    pub event_id: String,
    // allow, review or block.
    pub outcome: Outcome,
    // risk score, a higher number means riskier.
    pub score: u32,
    // one short reason for every rule that fired.
    pub reasons: Vec<String>,
}

// builds a plain transaction for tests, only the time and amount vary.
#[cfg(test)]
pub(crate) fn sample_event(timestamp: i64, amount: f64) -> TransactionEvent {
    // fill every field, the other values are fixed placeholders.
    TransactionEvent {
        // placeholder transaction id.
        event_id: "e1".to_string(),
        // placeholder user id.
        user_id: "u1".to_string(),
        // time given by the test.
        timestamp,
        // amount given by the test.
        amount,
        // placeholder device id.
        device_id: "d1".to_string(),
        // placeholder IP address.
        ip: "10.0.0.1".to_string(),
        // no signup time unless a test sets one.
        signup_time: None,
        // no city unless a test sets one.
        city: None,
    }
}
