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
    // short reasons that explain the outcome.
    pub reasons: Vec<String>,
}

// what the engine remembers about one user between transactions.
#[derive(Debug, Clone, Default)]
pub struct UserState {
    // number of transactions seen so far for this user.
    pub transaction_count: u64,
}

// decides one transaction from the user's remembered state, returns the decision and the updated state.
pub fn evaluate(event: &TransactionEvent, state: &UserState) -> (Decision, UserState) {
    // copy the old state so the caller's value stays unchanged.
    let mut next = state.clone();
    // count this transaction.
    next.transaction_count += 1;
    // build the decision, no rules exist yet so every transaction is allowed.
    let decision = Decision {
        // copy the id so the caller can match the decision to its transaction.
        event_id: event.event_id.clone(),
        // every transaction is allowed for now.
        outcome: Outcome::Allow,
        // no rule has added any risk yet.
        score: 0,
        // explain the decision, this also shows that the stored state is used.
        reasons: vec![format!(
            "no rules yet, {} transactions seen for this user",
            next.transaction_count
        )],
    };
    // hand back both the decision and the new state.
    (decision, next)
}