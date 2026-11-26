# Fraud Detection Engine: Design

Author: Wahid
Status: Draft v1
Date: 2026-11-21

## 1. Overview

I am building a fraud detection engine in Rust. A payment system sends it every transaction, and the engine returns a decision within milliseconds: allow, review or block. Every decision comes with the reasons behind it. Flagged cases go to a web page where an analyst marks each one as fraud or a false alarm, and that verdict feeds back into the engine.

## 2. Problem and goals

Businesses lose money to stolen cards, hijacked accounts and fraud rings, and no team can review every transaction by hand.

Goals:

1. Decide every transaction in real time at 5000 transactions per second with about 1,000,000 users.
2. Explain each decision with reason codes.
3. Cover eight rules: seven fraud types plus an account age rule.
4. Measure honestly: detection at a fixed false alarm budget, throughput and latency.
5. Follow industry practice: workspace crates, trait based storage, async Rust, tests, benchmarks and security.

Non goals for now: multiple engine instances, a monitoring dashboard, money based metrics, gRPC.

## 3. How it is used

| User           | What they do                                                                                     |
| -------------- | ------------------------------------------------------------------------------------------------ |
| Payment system | Sends transactions and account events over the API and receives a decision                       |
| Analyst        | Reviews flagged cases on a web page, reads the reasons, and marks each as fraud or a false alarm |
| Me, the owner  | Tunes rules and thresholds, runs the benchmarks, and reads the results                           |

## 4. Decisions

**1. Scale.**
Chosen: medium, 5000 TPS, about 1,000,000 users, one engine process.
Why: it forces real engineering on state size, concurrency and measurement, and it stays one program.
Not chosen: small (about 100 TPS) proves little. Large (10,000+ TPS across several instances) would spend the whole build on infrastructure, and the numbers would mean nothing without real traffic.

**2. Fraud types.**
Chosen: velocity abuse, amount anomaly, impossible travel, new device risk, blocklist hits, account takeover and linked accounts, plus an account age rule.
Why: together they cover the most common real patterns on a small shared data model.
Not chosen: odd behavior, such as unusual hours, because it flags too many honest users.

**3. Detection method.**
Chosen: rules, plus per user statistics, plus a machine learning scorer.
Why: rules catch known patterns and explain themselves. Per user statistics judge each user against their own normal. The model adds a learned signal.
Not chosen: rules only, because fixed thresholds treat every user the same. Machine learning only, because it is hard to explain and needs labeled data.

**4. Storage.**
Chosen: Garnet for state, Postgres for decisions and cases, and Rust memory for data that rarely changes. Docker on my PC for testing.
Why: Garnet is Microsoft's Redis compatible server. It is MIT licensed, runs natively on Windows and works with standard Redis clients. Postgres gives queryable cases and a durable record.
Not chosen: memory only with my own recovery, because it uses no database. An embedded database such as SQLite or RocksDB, because it is slower for hot state and RocksDB is hard to build on Windows. Redis through Memurai, because the license terms after the 90 day trial were unclear. Docker on my laptop, because it needs a BIOS change.

**5. Analyst interface.**
Chosen: a web page on top of the API.
Why: it shows the full workflow, and the API has to exist anyway.
Not chosen: API only, because there is nothing to see. A command line tool, because analysts do not work that way.

**6. Feedback loop.**
Chosen: verdicts change state. A fraud verdict adds the device, IP and account to the blocklist. A false alarm verdict adds a temporary allowlist entry that only lowers the score. Every verdict is stored in full.
Why: the effect is immediate and explainable, and the stored verdicts allow tuning later.
Not chosen: store only, because nothing improves. Automatic tuning from verdicts, because it needs many verdicts and has a bias: analysts only label what the engine flagged, so missed fraud never gets a label.

**7. Test data.**
Chosen: one unified dataset from two public datasets and my own generator.
Why: no single dataset has every field the rules need, and the generator fills the gaps and supplies volume.
Not chosen: a single public dataset, because none covers all the rules.

**8. Success measurement.**
Chosen: a threshold sweep, with detection reported at a fixed false alarm budget, the review rate, a per rule breakdown, throughput and median and p99 latency.
Why: this is how fraud teams compare systems, because the business sets the false alarm budget.
Not chosen: one fixed threshold, because it hides the tradeoff. Money based measures, because they need invented cost numbers.

**9. Stack.**
Chosen: `tokio`, `axum`, REST with JSON, a Cargo workspace, `serde`, the `redis` client, `sqlx`, `ort`, `tracing`, `thiserror` and `criterion`.
Why: these are the common choices, and they keep the rules testable on their own.
Not chosen: `actix-web`, because of its steeper learning curve. gRPC, because it needs more setup and browsers cannot call it directly. A single crate, because it mixes the rules with input and output.

## 5. Rules

| Rule              | Signal                                                                   | Notes                                                                             |
| ----------------- | ------------------------------------------------------------------------ | --------------------------------------------------------------------------------- | --- |
| Velocity          | Transactions per user, device and IP in a short window                   | 12 buckets of 5 minutes, summed when read                                         |
| Amount anomaly    | Amount against the user's running average and spread                     | Z score, default caution for users with no history                                |
| Impossible travel | Distance between the last and current city divided by the time           | Needs a city to coordinates table, US cities from dataset 2 and Bangladesh cities |     |
| New device        | Device not in the user's known set, with a larger weight for big amounts | Known set is capped in size                                                       |
| Blocklist         | Device, IP or account on the blocklist                                   | Entries from verdicts expire                                                      |
| Account takeover  | Failed logins, recent password change, new device, large amount          | A bonus applies when two or more fire together                                    |
| Linked accounts   | Distinct accounts per device (strong) and per IP (weak) over 7 days      | Cap of 50 accounts, extra weight when linked to a blocked account                 |
| Account age       | Time since signup, or since first seen when there is no signup time      | Very new accounts add weight                                                      |

Each rule returns a weight and a reason. The weights add up to a risk score, and two thresholds map the score to allow, review or block.

The machine learning scorer reads the same features the rules compute, so no feature code is duplicated. I train it offline in Python, starting with gradient boosted trees, and export it to ONNX. The engine loads it with `ort`.

## 6. Decision flow

1. The payment system sends a transaction to `POST /v1/evaluate`.
2. The server checks authentication, validates the input and enforces size limits.
3. The engine reads that user's state from Garnet in one round trip.
4. The engine computes the features and runs every rule.
5. The machine learning scorer produces its score from the same features.
6. The engine combines everything into a risk score and a decision with reason codes.
7. The new state is written back to Garnet in one round trip.
8. The server returns the decision.
9. A background task writes the decision to Postgres in batches, so the database never slows a decision.

## 7. Architecture

Starting skeleton:

```
 Payment system (a simulator during development)
        |
        |  POST /v1/evaluate    POST /v1/account-events
        v
 +----------------------- server (axum) -----------------------+
 |  authentication, validation, size limits                    |
 |        |                                                    |
 |        v                                                    |
 |  engine: state -> features -> rules -> ML score -> decision |
 |        |                                    |               |
 +--------|------------------------------------|---------------+
          | read and write state               | batched writes
          v                                    v
   Garnet                                  Postgres
   user state, links,                      decisions, cases,
   blocklist, allowlist                    verdicts
                                               ^
                                               |  through the server
 Analyst web page: GET /v1/cases, POST /v1/cases/{id}/verdict
```

Project layout:

```
fraud-engine/
  Cargo.toml          workspace
  crates/
    engine/           types, features, rules, scoring (no input or output)
    store/            storage trait with Garnet, Postgres and in memory versions
    server/           axum API, authentication, analyst page
    tools/            generator, dataset converters, replay and load runner
  training/           Python: model training and ONNX export
  config/             rule weights and thresholds
  data/               raw datasets, not committed
  docs/               design.md, dataset.md
```

The `engine` crate never touches the network or the disk, so every rule can be tested directly. The `store` crate defines one storage trait. The in memory version is for development and tests, and the Garnet and Postgres versions are what runs for real.

## 8. State and data ownership

Each piece of data has one owner, so two copies never go out of sync.

| Where             | What |
| ----------------- | ---- |
| Garnet            | Per user state (counters, running statistics, known devices, last city, login failures, password change time, first seen time), device and IP links with a 7 day expiry, blocklist and allowlist |
| Garnet, event key | The decision made for each event id, kept for 10 minutes, so a retried request gets the same answer and is not counted twice |
| Postgres          | Every decision with its reasons, cases, analyst verdicts. A case is opened for every review or block decision. |
| Rust memory       | Rules, config, the city table, the model |

Garnet holds a user's whole state together, so a decision costs two network calls: one read and one write.

## 9. API (initial)

Every endpoint except `GET /health` needs the header `x-api-key`. Input is validated, bodies are limited to 4 KB, each request has a 2 second limit, and at most 1024 requests run at once.

| Endpoint                      | Purpose                                    |
| ----------------------------- | ------------------------------------------ |
| `POST /v1/evaluate`           | Decide one transaction                     |
| `POST /v1/account-events`     | Record a failed login or a password change |
| `GET /v1/cases`               | List flagged cases for analysts            |
| `POST /v1/cases/{id}/verdict` | Mark a case as fraud or a false alarm      |
| `GET /health`                 | Liveness check                             |

A fraud verdict blocks the account, device and IP for 30 days. A false alarm verdict lowers the account's score by 30 points for 7 days and never overrides a block. Both durations and the 30 points are settings. Until the analyst login exists, the case routes use the same API key as the other routes.

Working event fields, finalized during the build: a `TransactionEvent` carries an event id, user id, timestamp, amount, device id, IP address, city or country, channel and an optional signup time. An `AccountEvent` carries a user id, timestamp and a kind.

## 10. Test data

The full plan is in `dataset.md`. In short:

| Source                                                                    | Role                                                      |
| ------------------------------------------------------------------------- | --------------------------------------------------------- |
| Dataset 1, ecommerce fraud, 151,112 transactions, 9.36% fraud, Apache 2.0 | Fraud labels, shared devices and IPs, signup gap          |
| Dataset 2, bank transactions, 2,512 rows, open source                     | Realistic honest behavior and history per account         |
| My generator                                                              | Injected attacks, ring accounts, password changes, volume |

Estimates: about 575,000 events for the accuracy benchmark, and 3,000,000 events over 1,000,000 users for the load benchmark.

## 11. Measurement

Two benchmarks:

1. Accuracy: replay the unified dataset in time order, sweep the threshold, and report detection at a fixed false alarm budget, the review rate and a per rule breakdown. Report rules only, machine learning only and both together. Report results with and without the rows in dataset 1 where signup and purchase are one second apart, because those rows make the data too easy.
2. Load: replay generated events at 5000 TPS and report throughput and median and p99 latency. Measure both the plain engine call and the full HTTP path. Record the machine used.

## 12. Security considerations

| Area                     | Plan                                                                                           | OWASP                                    |
| ------------------------ | ---------------------------------------------------------------------------------------------- | ---------------------------------------- |
| Payment system access    | API key on every request                                                                       | API2 Broken Authentication               |
| Analyst access           | Login and roles, authorization on the verdict endpoint                                         | API5 Broken Function Level Authorization |
| Resource limits          | Rate limiting, request size limits, input validation                                           | API4 Unrestricted Resource Consumption   |
| Analyst page             | Escape all transaction fields, protect the verdict action against forged requests              | A03:2021 Injection (XSS)                 |
| Service configuration    | Authentication on Garnet and Postgres, localhost only binding, a least privilege database user | API8 Security Misconfiguration           |
| Auditing                 | Log every decision and every verdict                                                           |                                          |
| Privacy                  | Hashed ids, no real card numbers, expiry on links                                              |                                          |
| Feedback abuse           | Allowlist only lowers the score and never overrides the blocklist, automatic entries expire    |                                          |
| Baseline poisoning       | Weight recent history less than long history, and cap how fast a baseline can move             |                                          |
| Threshold probing        | Rate limiting, plus per user statistics instead of one fixed line                              |                                          |
| Model file               | Loaded from a trusted path and checked at startup                                              |                                          |
| Dependencies and secrets | `cargo audit`, secrets in environment variables, HTTPS on the analyst page                     |                                          |
| Datasets                 | Respect licenses, keep raw data out of the repository                                          |                                          |

## 13. Risks and tradeoffs

1. Garnet is Redis compatible and is not Redis. I will check every command I rely on before building on it.
2. Network calls to Garnet add delay compared with memory. I expect a few milliseconds per decision, and the benchmark will show the real number.
3. The datasets are likely synthetic and merged, so results show the engine detects these patterns and are not real world accuracy.
4. One feature in dataset 1 separates fraud almost perfectly, so I report results with and without it.
5. A model must see the same feature values in training and in the engine. Reusing the engine's own features avoids a mismatch.
6. The project is large, so I build it in layers and keep each layer working.

## 14. Out of scope for now

Odd behavior rule, money based metrics, running several engine instances, a monitoring dashboard, automatic rule tuning from verdicts, gRPC.

## 15. Open questions

1. When Garnet is unreachable, the engine answers with status 503 and the payment system chooses whether to approve or block. (Solved for now)
2. Is the analyst page served by the engine or built as a separate Next.js app?
3. How do the machine learning score and the rules combine: a blend, or separate signals?
4. The final field list for `TransactionEvent`.
5. How many transactions each user has in dataset 1.
6. A latency target, set after the first measurements.

## 16. Build order

A rough order of work, not a schedule. I plan on about 10 days and adjust as I learn.

1. Workspace, types and the in memory store
2. The rules and scoring, with unit tests
3. The Garnet store
4. The API
5. Postgres, cases and verdicts
6. The analyst page
7. The generator and dataset converters
8. The benchmarks
9. The machine learning scorer
10. Security pass and README
