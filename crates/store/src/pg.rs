// the error type of the crate.
use crate::StoreError;
// the transaction and the decision that are stored together.
use engine::{Decision, TransactionEvent};
// the connection pool and its settings.
use sqlx::{PgPool, postgres::PgPoolOptions};
// type for the time limit on connecting.
use std::time::Duration;

// the tables, created when they do not exist yet.
const SCHEMA: &str = include_str!("schema.sql");
// most connections kept open, the background writer uses one at a time.
const MAX_CONNECTIONS: u32 = 5;
// longest wait for a free connection or for the first connection.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);

// one transaction with the decision made for it, the unit that is written to Postgres.
pub struct DecisionRecord {
    // what the payment system sent.
    pub event: TransactionEvent,
    // what the engine answered.
    pub decision: Decision,
}

// the Postgres version, it keeps decisions and cases for analysts and audits.
#[derive(Clone)]
pub struct Postgres {
    // connections that many tasks share.
    pool: PgPool,
}

impl Postgres {
    // connects to Postgres and makes sure the tables exist, the url looks like postgres://user:password@127.0.0.1:5432/fraud.
    pub async fn connect(url: &str) -> Result<Self, StoreError> {
        // open the pool, a wrong address or password fails here.
        let pool = PgPoolOptions::new()
            .max_connections(MAX_CONNECTIONS)
            .acquire_timeout(CONNECT_TIMEOUT)
            .connect(url)
            .await?;
        // create the tables, running it twice changes nothing.
        sqlx::raw_sql(SCHEMA).execute(&pool).await?;
        // keep the pool.
        Ok(Postgres { pool })
    }

    // writes many decisions in one statement, and opens a case for every one that is not allow.
    pub async fn insert_decisions(&self, batch: &[DecisionRecord]) -> Result<(), StoreError> {
        // each column of the batch as its own list, Postgres turns the lists back into rows.
        let event_ids: Vec<&str> = batch.iter().map(|r| r.event.event_id.as_str()).collect();
        // the users.
        let user_ids: Vec<&str> = batch.iter().map(|r| r.event.user_id.as_str()).collect();
        // the devices.
        let device_ids: Vec<&str> = batch.iter().map(|r| r.event.device_id.as_str()).collect();
        // the IP addresses.
        let ips: Vec<&str> = batch.iter().map(|r| r.event.ip.as_str()).collect();
        // the amounts.
        let amounts: Vec<f64> = batch.iter().map(|r| r.event.amount).collect();
        // the cities, some are empty.
        let cities: Vec<Option<&str>> = batch.iter().map(|r| r.event.city.as_deref()).collect();
        // the transaction times.
        let times: Vec<i64> = batch.iter().map(|r| r.event.timestamp).collect();
        // the outcomes as words.
        let outcomes: Vec<&str> = batch.iter().map(|r| r.decision.outcome.as_str()).collect();
        // the scores.
        let scores: Vec<i32> = batch.iter().map(|r| r.decision.score as i32).collect();
        // the reasons of each decision as one JSON text, the database turns it into a JSON value.
        let reasons = batch
            .iter()
            .map(|r| serde_json::to_string(&r.decision.reasons))
            .collect::<Result<Vec<String>, _>>()
            .map_err(|error| sqlx::Error::Encode(Box::new(error)))?;
        // one statement does both inserts, so a decision and its case are saved together or not at all.
        sqlx::query(
            "WITH new AS (
                 INSERT INTO decisions
                     (event_id, user_id, device_id, ip, amount, city, event_time, outcome, score, reasons)
                 SELECT event_id, user_id, device_id, ip, amount, city, event_time, outcome, score, reasons::jsonb
                 FROM UNNEST($1::text[], $2::text[], $3::text[], $4::text[], $5::float8[],
                             $6::text[], $7::int8[], $8::text[], $9::int4[], $10::text[])
                     AS t(event_id, user_id, device_id, ip, amount, city, event_time, outcome, score, reasons)
                 ON CONFLICT (event_id) DO NOTHING
                 RETURNING event_id, outcome
             )
             INSERT INTO cases (event_id)
             SELECT event_id FROM new WHERE outcome <> 'allow'
             ON CONFLICT (event_id) DO NOTHING",
        )
        .bind(event_ids)
        .bind(user_ids)
        .bind(device_ids)
        .bind(ips)
        .bind(amounts)
        .bind(cities)
        .bind(times)
        .bind(outcomes)
        .bind(scores)
        .bind(reasons)
        .execute(&self.pool)
        .await?;
        // done.
        Ok(())
    }
}