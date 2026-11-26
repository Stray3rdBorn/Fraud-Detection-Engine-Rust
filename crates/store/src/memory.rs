// the state types the store keeps, and the error and key helpers of the crate.
use crate::{Keys, Store, StoreError, device_key, event_key, ip_key, user_key};
use engine::{Decision, Entities, EntityState, UserState};
// map type for the records.
use std::collections::HashMap;
// lock type so many requests can share the maps safely.
use std::sync::Mutex;

// in memory version of the store, meant for development and tests.
#[derive(Default)]
pub struct InMemoryStore {
    // user key to state, behind a lock because requests run at the same time.
    users: Mutex<HashMap<String, UserState>>,
    // device or IP key to record, behind its own lock.
    entities: Mutex<HashMap<String, EntityState>>,
    // event key to the decision made for it, never expires here, which is fine for development.
    decisions: Mutex<HashMap<String, Decision>>,
}

impl InMemoryStore {
    // copies one user out, a new user gets an empty state.
    fn read_user(&self, key: &str) -> UserState {
        // lock the map, copy the state out, or use an empty state.
        self.users
            .lock()
            .unwrap()
            .get(key)
            .cloned()
            .unwrap_or_default()
    }

    // copies one device or IP record out, a new key gets an empty record.
    fn read_entity(&self, key: &str) -> EntityState {
        // lock the map, copy the record out, or use an empty record.
        self.entities
            .lock()
            .unwrap()
            .get(key)
            .cloned()
            .unwrap_or_default()
    }
}

// the in memory store follows the storage contract.
impl Store for InMemoryStore {
    async fn get_user(&self, user_id: &str) -> Result<UserState, StoreError> {
        // read the user.
        Ok(self.read_user(&user_key(user_id)))
    }

    async fn put_user(&self, user_id: &str, state: UserState) -> Result<(), StoreError> {
        // lock the map and insert or replace this user's state.
        self.users.lock().unwrap().insert(user_key(user_id), state);
        // nothing can fail in memory.
        Ok(())
    }

    async fn load(
        &self,
        keys: &Keys<'_>,
    ) -> Result<(UserState, Entities, Option<Decision>), StoreError> {
        // read the user, the device, the IP and any earlier decision for this event.
        Ok((
            self.read_user(&user_key(keys.user_id)),
            Entities {
                device: self.read_entity(&device_key(keys.device_id)),
                ip: self.read_entity(&ip_key(keys.ip)),
            },
            self.decisions
                .lock()
                .unwrap()
                .get(&event_key(keys.event_id))
                .cloned(),
        ))
    }

    async fn save(
        &self,
        keys: &Keys<'_>,
        state: UserState,
        entities: Entities,
        decision: &Decision,
    ) -> Result<(), StoreError> {
        // save the user.
        self.users
            .lock()
            .unwrap()
            .insert(user_key(keys.user_id), state);
        // save the decision, so a repeated event id gets the same answer.
        self.decisions
            .lock()
            .unwrap()
            .insert(event_key(keys.event_id), decision.clone());
        // lock the entity map once and save the device and the IP.
        let mut map = self.entities.lock().unwrap();
        // the device record.
        map.insert(device_key(keys.device_id), entities.device);
        // the IP record.
        map.insert(ip_key(keys.ip), entities.ip);
        // nothing can fail in memory.
        Ok(())
    }
}

// tests for the in memory store.
#[cfg(test)]
mod tests {
    // bring the store and the shared test records into the tests.
    // bring the store and the shared test records into the tests.
    use super::*;
    use crate::testing::{busy_decision, busy_records};

    // new keys load empty, and what is saved is loaded back unchanged.
    #[tokio::test]
    async fn save_then_load_returns_the_same_records() {
        // an empty store.
        let store = InMemoryStore::default();
        // the ids of one transaction.
        let keys = Keys {
            user_id: "u1",
            device_id: "d1",
            ip: "1.1.1.1",
            event_id: "e1",
        };
        // a new user, device, IP and event load empty.
        let (empty_state, empty_entities, empty_decision) = store.load(&keys).await.unwrap();
        // the state is empty.
        assert_eq!(empty_state, UserState::default());
        // the entities are empty.
        assert_eq!(empty_entities, Entities::default());
        // no decision exists yet.
        assert!(empty_decision.is_none());
        // save records that carry data in every field.
        let (state, entities) = busy_records();
        // the decision that goes with them.
        let decision = busy_decision();
        // write them.
        store
            .save(&keys, state.clone(), entities.clone(), &decision)
            .await
            .unwrap();
        // read them back.
        let (loaded_state, loaded_entities, loaded_decision) = store.load(&keys).await.unwrap();
        // the state is the same.
        assert_eq!(loaded_state, state);
        // the entities are the same.
        assert_eq!(loaded_entities, entities);
        // the decision is the same.
        assert_eq!(loaded_decision, Some(decision));
        // the user alone can be read too.
        assert_eq!(store.get_user("u1").await.unwrap(), state);
    }
}
