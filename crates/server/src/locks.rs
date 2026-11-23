// the engine hash, which also spreads keys over the lock slots.
use engine::id_hash;
// the async lock and its guard, held while a request works on its records.
use tokio::sync::{Mutex, MutexGuard};

// number of lock slots, many keys share a slot, which is safe and only costs a little waiting.
const SLOTS: usize = 1024;

// a fixed set of locks that stand in for one lock per user, device and IP.
pub struct KeyLocks {
    // every slot is one lock.
    slots: Vec<Mutex<()>>,
}

impl KeyLocks {
    // creates all slots.
    pub fn new() -> Self {
        // one unlocked lock per slot.
        KeyLocks {
            slots: (0..SLOTS).map(|_| Mutex::new(())).collect(),
        }
    }

    // locks the slots of all given keys, the locks are released when the returned guards are dropped.
    pub async fn lock(&self, keys: &[&str]) -> Vec<MutexGuard<'_, ()>> {
        // the slot of every key.
        let mut slots: Vec<usize> = keys
            .iter()
            .map(|key| id_hash(key) as usize % SLOTS)
            .collect();
        // a fixed order means two requests can never wait on each other in a circle.
        slots.sort_unstable();
        // two keys in the same slot must not lock it twice.
        slots.dedup();
        // the guards that are collected one by one.
        let mut guards = Vec::with_capacity(slots.len());
        // lock in order.
        for slot in slots {
            // wait for this slot and keep its guard.
            guards.push(self.slots[slot].lock().await);
        }
        // hand back all guards.
        guards
    }
}

// tests for the locks.
#[cfg(test)]
mod tests {
    // bring the locks and helpers into the tests.
    use super::*;
    use std::{sync::Arc, time::Duration};

    // requests that ask for the same keys in opposite order must all finish.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn opposite_key_orders_do_not_deadlock() {
        // one set of locks shared by all tasks.
        let locks = Arc::new(KeyLocks::new());
        // the running tasks.
        let mut tasks = Vec::new();
        // fifty tasks, half ask for a then b, half for b then a.
        for i in 0..50 {
            // each task gets its own handle.
            let locks = Arc::clone(&locks);
            // start the task.
            tasks.push(tokio::spawn(async move {
                // the key order depends on the task number.
                let keys = if i % 2 == 0 { ["a", "b"] } else { ["b", "a"] };
                // take the locks.
                let _guards = locks.lock(&keys).await;
                // give other tasks a chance to run while the locks are held.
                tokio::task::yield_now().await;
            }));
        }
        // wait for every task, a deadlock would hit the time limit.
        let all = async {
            // wait for each task.
            for task in tasks {
                // a panic inside a task fails the test.
                task.await.unwrap();
            }
        };
        // five seconds is far more than needed.
        tokio::time::timeout(Duration::from_secs(5), all)
            .await
            .expect("tasks deadlocked");
    }

    // the same key twice, or two keys in one slot, must not block the request on itself.
    #[tokio::test]
    async fn the_same_key_twice_does_not_block() {
        // one set of locks.
        let locks = KeyLocks::new();
        // the same key twice.
        let guards = tokio::time::timeout(Duration::from_secs(5), locks.lock(&["a", "a"]))
            .await
            .expect("blocked on itself");
        // only one lock was taken.
        assert_eq!(guards.len(), 1);
    }
}
