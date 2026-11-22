// the state type that the store keeps for each user.
use engine::{EntityState, UserState};
// map type for the in memory version.
use std::collections::HashMap;
// future type used by the async methods of the storage contract.
use std::future::Future;
// lock type so many requests can share the map safely.
use std::sync::Mutex;

// the storage contract, every store version must provide these two operations.
pub trait Store: Send + Sync + 'static {
    // read the state of one user, a new user gets an empty state.
    fn get(&self, user_id: &str) -> impl Future<Output = UserState> + Send;
    // save the state of one user.
    fn put(&self, user_id: &str, state: UserState) -> impl Future<Output = ()> + Send;
        // read what is remembered about one device or IP, the key names which one, a new key gets an empty record.
    fn get_entity(&self, key: &str) -> impl Future<Output = EntityState> + Send;
    // save what is remembered about one device or IP.
    fn put_entity(&self, key: &str, entity: EntityState) -> impl Future<Output = ()> + Send;
}

// in memory version of the store, meant for development and tests.
#[derive(Default)]
pub struct InMemoryStore {
    // user id to state, behind a lock because requests run at the same time.
    users: Mutex<HashMap<String, UserState>>,
        // device or IP key to record, behind its own lock.
    entities: Mutex<HashMap<String, EntityState>>,
}

// the in memory store follows the storage contract.
impl Store for InMemoryStore {
    async fn get(&self, user_id: &str) -> UserState {
        // lock the map, copy this user's state out, or use an empty state for a new user.
        self.users
            .lock()
            .unwrap()
            .get(user_id)
            .cloned()
            .unwrap_or_default()
    }

    async fn put(&self, user_id: &str, state: UserState) {
        // lock the map and insert or replace this user's state.
        self.users
            .lock()
            .unwrap()
            .insert(user_id.to_string(), state);
    }
        async fn get_entity(&self, key: &str) -> EntityState {
        // lock the map, copy this record out, or use an empty record for a new key.
        self.entities
            .lock()
            .unwrap()
            .get(key)
            .cloned()
            .unwrap_or_default()
    }

    async fn put_entity(&self, key: &str, entity: EntityState) {
        // lock the map and insert or replace this record.
        self.entities
            .lock()
            .unwrap()
            .insert(key.to_string(), entity);
    }
}
