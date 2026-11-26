// the storage contract, the error type and the key helpers of the crate.
use crate::{Keys, Store, StoreError, device_key, event_key, ip_key, user_key};
// the state types the store keeps.
use engine::{Decision, Entities, UserState};
// connection that is shared by all requests and reconnects by itself.
use redis::aio::ConnectionManager;
// serde traits that turn records into bytes and back.
use serde::{Serialize, de::DeserializeOwned};

// seconds a device or IP record lives after its last write, 7 days, the same as the link window.
const ENTITY_TTL_SECONDS: u64 = 604_800;
// seconds a decision is remembered for repeated event ids, 10 minutes, long enough for a payment system to retry.
const EVENT_TTL_SECONDS: u64 = 600;
// first byte of every stored value, raised when the layout of a stored type changes.
const FORMAT_VERSION: u8 = 3;

// the Garnet version of the store, it speaks the Redis protocol.
#[derive(Clone)]
pub struct GarnetStore {
    // one connection that many requests share.
    connection: ConnectionManager,
}

impl GarnetStore {
    // connects to Garnet, the url looks like redis://:password@127.0.0.1:6380.
    pub async fn connect(url: &str) -> Result<Self, StoreError> {
        // parse the url.
        let client = redis::Client::open(url)?;
        // open the connection.
        let connection = ConnectionManager::new(client).await?;
        // keep it.
        Ok(GarnetStore { connection })
    }
}

// turns a record into bytes, the version byte comes first and the compact record follows.
fn encode<T: Serialize>(value: &T) -> Result<Vec<u8>, StoreError> {
    // start the byte list with the version and let the record append itself.
    Ok(postcard::to_extend(value, vec![FORMAT_VERSION])?)
}


// turns stored bytes back into a record, a wrong version or damaged bytes are errors.
fn decode_bytes<T: DeserializeOwned>(bytes: &[u8]) -> Result<T, StoreError> {
    // split the version byte from the rest.
    let (&version, body) = bytes.split_first().ok_or(StoreError::Empty)?;
    // a value from another layout must not be read.
    if version != FORMAT_VERSION {
        // report both versions.
        return Err(StoreError::Version(version, FORMAT_VERSION));
    }
    // read the record from the remaining bytes.
    Ok(postcard::from_bytes(body)?)
}

// turns a stored value into a record, a missing value is a new user, device or IP.
fn decode<T: DeserializeOwned + Default>(bytes: Option<Vec<u8>>) -> Result<T, StoreError> {
    // a key that does not exist yet gives an empty record.
    bytes.map_or_else(|| Ok(T::default()), |bytes| decode_bytes(&bytes))
}

// turns a stored value into a record, a missing value stays missing.
fn decode_optional<T: DeserializeOwned>(bytes: Option<Vec<u8>>) -> Result<Option<T>, StoreError> {
    // only a value that exists is decoded.
    bytes.map(|bytes| decode_bytes(&bytes)).transpose()
}

// the Garnet store follows the storage contract.
impl Store for GarnetStore {
    async fn get_user(&self, user_id: &str) -> Result<UserState, StoreError> {
        // a clone of the connection is cheap, all clones share one link.
        let mut connection = self.connection.clone();
        // ask for the one key.
        let bytes: Option<Vec<u8>> = redis::cmd("GET")
            .arg(user_key(user_id))
            .query_async(&mut connection)
            .await?;
        // turn the bytes into a record.
        decode(bytes)
    }

    async fn put_user(&self, user_id: &str, state: UserState) -> Result<(), StoreError> {
        // a clone of the connection is cheap, all clones share one link.
        let mut connection = self.connection.clone();
        // write the key, a user record has no expiry.
        redis::cmd("SET")
            .arg(user_key(user_id))
            .arg(encode(&state)?)
            .query_async::<()>(&mut connection)
            .await?;
        // done.
        Ok(())
    }
    async fn load(
        &self,
        keys: &Keys<'_>,
    ) -> Result<(UserState, Entities, Option<Decision>), StoreError> {
        // a clone of the connection is cheap, all clones share one link.
        let mut connection = self.connection.clone();
        // the four keys of one transaction.
        let names = [
            user_key(keys.user_id),
            device_key(keys.device_id),
            ip_key(keys.ip),
            event_key(keys.event_id),
        ];
        // one MGET returns all four values in one round trip, a missing key comes back empty.
        let values: Vec<Option<Vec<u8>>> = redis::cmd("MGET")
            .arg(&names[..])
            .query_async(&mut connection)
            .await?;
        // exactly four values are expected, in the order of the keys.
        let [user, device, ip, event]: [Option<Vec<u8>>; 4] =
            values.try_into().map_err(|_| StoreError::BadReply)?;
        // turn each value into its record.
        Ok((
            decode(user)?,
            Entities {
                device: decode(device)?,
                ip: decode(ip)?,
            },
            decode_optional(event)?,
        ))
    }

    async fn save(
        &self,
        keys: &Keys<'_>,
        state: UserState,
        entities: Entities,
        decision: &Decision,
    ) -> Result<(), StoreError> {
        // a clone of the connection is cheap, all clones share one link.
        let mut connection = self.connection.clone();
        // atomic wraps the four writes in MULTI and EXEC, so they all happen or none does, in one round trip.
        redis::pipe()
            .atomic()
            // the user record has no expiry.
            .set(user_key(keys.user_id), encode(&state)?)
            .ignore()
            // device and IP records expire after 7 days without a write.
            .set_ex(
                device_key(keys.device_id),
                encode(&entities.device)?,
                ENTITY_TTL_SECONDS,
            )
            .ignore()
            .set_ex(ip_key(keys.ip), encode(&entities.ip)?, ENTITY_TTL_SECONDS)
            .ignore()
            // the decision is remembered for a short time, so a retry gets the same answer.
            .set_ex(
                event_key(keys.event_id),
                encode(decision)?,
                EVENT_TTL_SECONDS,
            )
            .ignore()
            .query_async::<()>(&mut connection)
            .await?;
        // done.
        Ok(())
    }
}

// tests for the Garnet store.
#[cfg(test)]
mod tests {
    // bring the store and the shared test records into the tests.
    use super::*;
    use crate::testing::{busy_decision, busy_records};

    // a record survives encoding and decoding, and a missing value is an empty record.
    #[test]
    fn records_survive_a_round_trip() {
        // records that carry data in every field.
        let (state, entities) = busy_records();
        // the first byte is the version.
        assert_eq!(encode(&state).unwrap()[0], FORMAT_VERSION);
        // the user state comes back equal.
        let back: UserState = decode(Some(encode(&state).unwrap())).unwrap();
        // compare.
        assert_eq!(back, state);
        // the device record comes back equal.
        let back: engine::EntityState = decode(Some(encode(&entities.device).unwrap())).unwrap();
        // compare.
        assert_eq!(back, entities.device);
        // a missing key gives an empty record.
        assert_eq!(decode::<UserState>(None).unwrap(), UserState::default());
    }

    // values with a wrong version, no bytes or garbage are rejected, never read as an empty record.
    #[test]
    fn bad_values_are_rejected() {
        // a value from another version.
        assert!(matches!(
            decode::<UserState>(Some(vec![FORMAT_VERSION + 1, 0, 0])),
            Err(StoreError::Version(_, _))
        ));
        // a value with no bytes.
        assert!(matches!(
            decode::<UserState>(Some(vec![])),
            Err(StoreError::Empty)
        ));
        // a value with the right version and garbage after it.
        assert!(matches!(
            decode::<UserState>(Some(vec![FORMAT_VERSION, 255, 255, 255])),
            Err(StoreError::Decode(_))
        ));
    }

    // against a real server, run with: cargo test -p store -- --ignored, with GARNET_URL set.
    #[tokio::test]
    #[ignore = "needs a running Garnet or Redis server and the GARNET_URL variable"]
    async fn garnet_round_trip_and_expiry() {
        // the address of the server under test.
        let url = std::env::var("GARNET_URL").expect("set GARNET_URL");
        // connect.
        let store = GarnetStore::connect(&url).await.unwrap();
        let id = format!("garnet-test-{}", std::process::id());
               // all four ids are the same text, the key prefixes keep them apart.
        let keys = Keys {
            user_id: &id,
            device_id: &id,
            ip: &id,
            event_id: &id,
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
        assert_eq!(store.get_user(&id).await.unwrap(), state);
        // a clone of the connection to ask for expiry times.
        let mut connection = store.connection.clone();
        // the user key never expires, TTL answers -1.
        let user_ttl: i64 = redis::cmd("TTL")
            .arg(user_key(&id))
            .query_async(&mut connection)
            .await
            .unwrap();
        // check.
        assert_eq!(user_ttl, -1);
        // the device key expires within 7 days.
        let device_ttl: i64 = redis::cmd("TTL")
            .arg(device_key(&id))
            .query_async(&mut connection)
            .await
            .unwrap();
        // check, it is set and not above the limit.
        assert!((1..=ENTITY_TTL_SECONDS as i64).contains(&device_ttl));
        // the IP key expires within 7 days.
        let ip_ttl: i64 = redis::cmd("TTL")
            .arg(ip_key(&id))
            .query_async(&mut connection)
            .await
            .unwrap();
        // check.
        assert!((1..=ENTITY_TTL_SECONDS as i64).contains(&ip_ttl));
                // the decision key expires within 10 minutes.
        let event_ttl: i64 = redis::cmd("TTL")
            .arg(event_key(&id))
                        .arg(event_key(&id))
            .query_async(&mut connection)
            .await
            .unwrap();
        // check.
        assert!((1..=EVENT_TTL_SECONDS as i64).contains(&event_ttl));
        // clean up the test keys.
        redis::cmd("DEL")
            .arg(user_key(&id))
            .arg(device_key(&id))
            .arg(ip_key(&id))
            .query_async::<()>(&mut connection)
            .await
            .unwrap();
    }
}
