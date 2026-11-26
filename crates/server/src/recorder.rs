// the record that is queued and the Postgres store that receives it.
use store::{DecisionRecord, Postgres};
// the queue between the request handlers and the writing task.
use tokio::sync::mpsc;
// counter that many tasks can add to, and the time helpers for the retry pause.
use std::{
    sync::atomic::{AtomicU64, Ordering},
    time::Duration,
};

// decisions that can wait in the queue, a full queue means Postgres is far behind.
const QUEUE_SIZE: usize = 10_000;
// most decisions written in one statement.
const BATCH_SIZE: usize = 500;
// tries for one batch before it is given up.
const TRIES: u32 = 3;
// pause between tries.
const RETRY_PAUSE: Duration = Duration::from_secs(1);

// hands decisions to a background task, so a decision never waits for the database.
pub struct Recorder {
    // the sending end of the queue.
    queue: mpsc::Sender<DecisionRecord>,
    // how many decisions were dropped because the queue was full.
    dropped: AtomicU64,
}

impl Recorder {
    // starts the writing task and returns the handle that feeds it.
    pub fn start(db: Postgres) -> Self {
        // a bounded queue, so memory cannot grow without limit.
        let (queue, mut inbox) = mpsc::channel(QUEUE_SIZE);
        // the writing task runs next to the request handlers.
        tokio::spawn(async move {
            // the batch is reused, so no new memory is needed for each round.
            let mut batch = Vec::with_capacity(BATCH_SIZE);
            // wait for at least one decision and take everything already waiting, up to the batch size.
            // a slow database lets the queue fill, so batches grow by themselves under load.
            while inbox.recv_many(&mut batch, BATCH_SIZE).await > 0 {
                // write the batch.
                write(&db, &batch).await;
                // empty it for the next round.
                batch.clear();
            }
        });
        // the handle.
        Recorder {
            queue,
            dropped: AtomicU64::new(0),
        }
    }

    // queues one decision without waiting, a full queue drops it and says so.
    pub fn record(&self, record: DecisionRecord) {
        // try_send never waits.
        if self.queue.try_send(record).is_err() {
            // count the loss.
            let total = self.dropped.fetch_add(1, Ordering::Relaxed) + 1;
            // print only at 1, 2, 4, 8 and so on, so a long outage does not flood the log.
            if total.is_power_of_two() {
                // tell the operator.
                eprintln!("decision log is full, {total} decisions were not stored so far");
            }
        }
    }
}

// writes one batch, retrying a few times, and gives it up when Postgres stays unavailable.
async fn write(db: &Postgres, batch: &[DecisionRecord]) {
    // try a few times.
    for attempt in 1..=TRIES {
        // success ends the work.
        match db.insert_decisions(batch).await {
            // stored.
            Ok(()) => return,
            // print the reason for the operator and wait before the next try.
            Err(error) => {
                // the error and the attempt.
                eprintln!("decision log write failed, try {attempt} of {TRIES}: {error}");
                // wait.
                tokio::time::sleep(RETRY_PAUSE).await;
            }
        }
    }
    // the batch is dropped so the queue keeps moving.
    eprintln!(
        "decision log gave up, {} decisions were not stored",
        batch.len()
    );
}
