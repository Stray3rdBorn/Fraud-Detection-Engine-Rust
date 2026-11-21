// the state type that the store keeps for each user.
use engine::UserState;
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
}

// in memory version of the store, meant for development and tests.
#[derive(Default)]
pub struct InMemoryStore {
    // user id to state, behind a lock because requests run at the same time.
    users: Mutex<HashMap<String, UserState>>,
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
}