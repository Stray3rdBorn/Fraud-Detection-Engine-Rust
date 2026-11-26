// the state types the store keeps.
use engine::{Decision, Entities, UserState};
// future type used by the async methods of the storage contract.
use std::future::Future;

// the Garnet version of the store.
mod garnet;
// the in memory version of the store.
mod memory;
// the Postgres version, which keeps decisions and cases.
mod pg;

// make both versions available as store::Name.
pub use garnet::GarnetStore;
pub use memory::InMemoryStore;
pub use pg::{CaseRow, CaseTarget, DecisionRecord, Postgres};

// everything that can go wrong when a store is used.
#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    // the connection to Garnet failed or Garnet answered with an error.
    #[error("garnet error: {0}")]
    Garnet(#[from] redis::RedisError),
    // a stored value could not be turned back into a record.
    #[error("stored value could not be decoded: {0}")]
    Decode(#[from] postcard::Error),
    // a stored value was written with another layout version.
    #[error("stored value has format version {0}, expected {1}")]
    Version(u8, u8),
    // a stored value has no bytes at all.
    #[error("stored value is empty")]
    Empty,
    // Garnet answered with a different number of values than were asked for.
    #[error("garnet returned an unexpected number of values")]
    BadReply,
    // the connection to Postgres failed or Postgres answered with an error.
    #[error("postgres error: {0}")]
    Postgres(#[from] sqlx::Error),
}

// the key of one user's record, the prefix keeps users, devices and IPs apart.
fn user_key(user_id: &str) -> String {
    // prefix and id.
    format!("user:{user_id}")
}

// the key of one device's record.
fn device_key(device_id: &str) -> String {
    // prefix and id.
    format!("device:{device_id}")
}

// the key of one IP address's record.
fn ip_key(ip: &str) -> String {
    // prefix and address.
    format!("ip:{ip}")
}

// the key of one transaction's stored decision, used to answer a repeated event id with the same decision.
fn event_key(event_id: &str) -> String {
    // prefix and id.
    format!("event:{event_id}")
}

// the four ids of one transaction, the keys of everything its decision reads and writes.
pub struct Keys<'a> {
    // the user who made the transaction.
    pub user_id: &'a str,
    // the device it came from.
    pub device_id: &'a str,
    // the IP address it came from.
    pub ip: &'a str,
    // the transaction itself.
    pub event_id: &'a str,
}

// the storage contract, every store version must provide these operations.
pub trait Store: Send + Sync + 'static {
    // read the state of one user, a new user gets an empty state.
    fn get_user(&self, user_id: &str)
    -> impl Future<Output = Result<UserState, StoreError>> + Send;
    // save the state of one user.
    fn put_user(
        &self,
        user_id: &str,
        state: UserState,
    ) -> impl Future<Output = Result<(), StoreError>> + Send;
    // read everything one transaction needs in one round trip, the user, the device, the IP
    // and the decision already made for this event id, which is empty for a new event.
    fn load(
        &self,
        keys: &Keys<'_>,
    ) -> impl Future<Output = Result<(UserState, Entities, Option<Decision>), StoreError>> + Send;
    // save the user, device and IP records alone, used when a verdict changes a block or an allowlist entry.
    fn update(
        &self,
        keys: &Keys<'_>,
        state: UserState,
        entities: Entities,
    ) -> impl Future<Output = Result<(), StoreError>> + Send;
    // save everything one transaction changed, including its decision, all or nothing.
    fn save(
        &self,
        keys: &Keys<'_>,
        state: UserState,
        entities: Entities,
        decision: &Decision,
    ) -> impl Future<Output = Result<(), StoreError>> + Send;
}

// records that carry data in every field, shared by the tests of both versions.
#[cfg(test)]
mod testing {
    // the types used to build the records.
    use engine::{Entities, TransactionEvent, UserState, id_hash};

    // a decision where no field is empty.
    pub fn busy_decision() -> engine::Decision {
        // every field filled.
        engine::Decision {
            event_id: "e1".to_string(),
            outcome: engine::Outcome::Review,
            score: 55,
            reasons: vec!["velocity: 9 transactions in the last hour (+16)".to_string()],
        }
    }

    // builds a user state and entity records where no field is empty, so a round trip checks everything.
    pub fn busy_records() -> (UserState, Entities) {
        // one transaction with every optional field set.
        let event = TransactionEvent {
            event_id: "e1".to_string(),
            user_id: "u1".to_string(),
            timestamp: 1_700_000_000,
            amount: 42.5,
            device_id: "d1".to_string(),
            ip: "1.1.1.1".to_string(),
            signup_time: Some(1_699_990_000),
            city: Some("Dhaka".to_string()),
        };
        // record it twice so the counters and statistics have a history.
        let mut state = UserState::default();
        // first record.
        state.record(&event);
        // second record.
        state.record(&event);
        // a password change and a block fill the last two fields.
        state.password_changed_at = Some(1_699_999_000);
        // end time of the block.
        state.blocked_until = Some(1_800_000_000);
        // two accounts on the device and the IP, one of them blocked.
        let mut entities = Entities::default();
        // first account.
        entities.record(id_hash("u1"), event.timestamp, false);
        // second account.
        entities.record(id_hash("u2"), event.timestamp + 5, true);
        // the entity block is the last empty field.
        entities.device.blocked_until = Some(1_800_000_000);
        // hand back both.
        (state, entities)
    }
}
