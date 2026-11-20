# System Design: Real-Time Fraud Detection Engine

## 1. Overview & Goals

A low-latency, high-throughput fraud detection service written in Rust. It evaluates incoming financial transactions in real time using sliding-window velocity checks, entity-link graphs, hot-reloadable deterministic rules, and statistical/ML anomaly scoring—without blocking legitimate users.

### Non-Functional Targets (Success Criteria)

- **Throughput:** 2,000 – 5,000 Transactions Per Second (TPS) on a single node.
- **Latency:** `< 15ms` at p99 for `POST /v1/evaluate` (end-to-end decision path).
- **Scale:** 100,000 simulated active users.
- **Detection Accuracy:** `> 85%` Precision and `> 80%` Recall against simulated adversarial traffic.
- **Resilience:** Idempotent request handling and configurable fail-open/fail-closed behavior if state stores degrade.

### Explicit Non-Goals (Out of Scope)

- Multi-region active-active clustering.
- Integration with live payment processors (Stripe/Adyen) or external commercial IP/ASN APIs (simulated locally instead).
- Online/streaming ML model retraining (handled via offline batch export).

---

## 2. Threat Model (Fraud Scenarios Covered)

1. **Card Testing:** Rapid bursts of small-amount authorizations from a single card, device, or IP within short sliding windows (1m, 10m).
2. **Account Takeover (ATO):** High-value withdrawal or purchase occurring shortly after failed login spikes, a password/email change, or from an unrecognized device/impossible travel location.
3. **Fraud Rings (Linked Accounts):** Multiple user accounts sharing the same `device_id` or datacenter/proxy `ip` within synchronized timeframes (distinguished from benign shared carrier/public Wi-Fi NATs via device-to-IP ratios and ASN metadata).

---

## 3. High-Level Architecture

```text
                       ┌──────────────────────┐
                       │   Traffic Simulator  │
                       │ (Normal + Attackers) │
                       └──────────┬───────────┘
                                  │ HTTP/JSON (POST /v1/evaluate)
                                  ▼
┌─────────────────────────────────────────────────────────────────────────┐
│                        Rust Decision Service (Axum)                     │
│                                                                         │
│  ┌─────────────┐    ┌───────────────────┐    ┌───────────────────────┐  │
│  │ Idempotency │───▶│ Feature Hydrator  │───▶│   Decision Pipeline   │  │
│  │   Guard     │    │  (tokio::join!)   │    │ Rules (ArcSwap) + ML  │  │
│  └──────┬──────┘    └─────────┬─────────┘    └───────────┬───────────┘  │
│         │                     │                          │              │
│         ▼                     ▼                          ▼              │
│  ┌────────────────────────────────────┐        ┌─────────────────────┐  │
│  │       Redis (Hot State Store)      │        │  Async Audit Queue  │  │
│  │ • Sliding Windows (Lua ZSETs)      │        │  (tokio::mpsc)      │  │
│  │ • Bounded Entity Graph (Sets+TTL)  │        └──────────┬──────────┘  │
│  │ • User State (Last Reset, Mean/SD) │                   │              │
│  └────────────────────────────────────┘                   ▼              │
│                                                ┌─────────────────────┐  │
│  ┌────────────────────────────────────┐        │ Postgres (Cold DB)  │  │
│  │  Analyst Web UI & Feedback API     │◀──────▶│ • Audit Traces      │  │
│  │  (Review Queue + Verdict Override) │        │ • Analyst Labels    │  │
│  └────────────────────────────────────┘        └─────────────────────┘  │
└─────────────────────────────────────────────────────────────────────────┘
```

---

## 4. Request Lifecycle (Hot Path)

When `POST /v1/evaluate` receives a `TransactionEvent`:

1. **Idempotency Check:** Checks Redis `SETNX idemp:{tx_id}` (TTL 24h). Duplicate retries immediately return the cached `Decision` without inflating velocity counters.
2. **Parallel Feature Hydration (`tokio::join!`):** Concurrently fetches context from Redis:
   - **Velocity Counters:** Atomic Lua script updates and queries Redis Sorted Sets (`ZSET`) for 1m, 10m, and 1h transaction counts and spend sums.
   - **Entity Graph Links:** Checks bounded `SET`s (`device:{id} -> users`, `ip:{addr} -> users`) capped at 50 entries with a 7-day TTL to prevent memory bloat on carrier NATs.
   - **User Baseline & ATO State:** Reads `last_password_change_ts`, `failed_logins_1h`, and running spend mean/variance (Welford's online algorithm).
3. **Rule Evaluation:** Evaluates active rules (and `SHADOW` rules) loaded via `ArcSwap` for lock-free, zero-downtime YAML hot-reloading. Supports weighted risk points and hard overrides.
4. **Statistical & ML Scoring:** Computes Z-score spend deviation and runs the enriched `FeatureVector` through a pre-trained LightGBM model exported to ONNX (`ort`).
5. **Decision & Non-Blocking Audit:** Resolves final action (`BLOCK > CHALLENGE > APPROVE`), returns HTTP 200 immediately, and pushes the full decision trace over a bounded `tokio::sync::mpsc` channel to a background worker that batch-inserts into PostgreSQL.

---

## 5. Key Architectural Trade-Offs

| Decision               | Choice                                        | Alternative Considered                            | Rationale                                                                                                                                                                                         |
| :--------------------- | :-------------------------------------------- | :------------------------------------------------ | :------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| **State Storage**      | **Two-Tier (Redis + Postgres)**               | SQLite or Postgres-only                           | Separates sub-millisecond in-memory sliding windows and graph lookups from durable relational audit logs.                                                                                         |
| **Data Model**         | **Enriched Transaction + User Profile State** | Polymorphic Event Stream (`Login`, `Reset`, `Tx`) | Keeps the ingestion schema clean while capturing ATO signals (`last_password_change_ts`, `failed_logins_1h`) via lightweight user state keys.                                                     |
| **NAT vs. Fraud Ring** | **Composite Signals + Bounded Sets**          | Blocking on raw IP count                          | Shared carrier IPs have many users across _unique_ devices; fraud rings share _few_ devices across many accounts or originate from datacenter ASNs. Capping Redis Sets at 50 prevents RAM spikes. |
| **Rule Reloading**     | **`ArcSwap` + YAML DSL**                      | Hardcoded Rust `match` or external microservice   | Allows live rule updates and Shadow Mode testing with zero lock contention and zero network overhead.                                                                                             |
| **Audit Logging**      | **Async `mpsc` Batch Writer**                 | Synchronous `INSERT` on request                   | Keeps disk I/O off the critical latency path while preserving full explainability for analyst reviews.                                                                                            |

---

## 6. Repository Structure

```text
fraud-engine/
├── Cargo.toml
├── DESIGN.md
├── docker-compose.yml          # Redis, Postgres, Prometheus
├── rules/
│   └── default_rules.yaml      # Active & Shadow rules
├── models/
│   └── fraud_scorer.onnx       # Pre-trained ONNX model
└── src/
    ├── main.rs                 # App bootstrap, graceful shutdown, worker wiring
    ├── config.rs               # Environment config & fail-open/closed policy
    ├── api/
    │   ├── mod.rs
    │   ├── evaluate.rs         # POST /v1/evaluate
    │   ├── analyst.rs          # GET /v1/cases, POST /v1/cases/{id}/verdict
    │   └── dashboard.rs        # Minimal HTML/HTMX analyst review UI
    ├── domain/
    │   ├── event.rs            # TransactionEvent & enriched FeatureVector
    │   ├── decision.rs         # Action, RiskScore, TriggeredRule traces
    │   └── error.rs            # Typed errors (thiserror)
    ├── features/
    │   ├── mod.rs
    │   ├── redis_store.rs      # Lua sliding windows, bounded graph sets, idempotency
    │   └── stats.rs            # Online mean/variance (Z-score) & geo-distance
    ├── engine/
    │   ├── mod.rs
    │   ├── pipeline.rs         # Core 5-step evaluation orchestrator
    │   ├── rules.rs            # Expression evaluator + ArcSwap file watcher
    │   └── ml_scorer.rs        # ONNX Runtime (`ort`) inference wrapper
    ├── storage/
    │   ├── mod.rs
    │   └── postgres.rs         # Async batch audit writer & analyst verdict queries
    └── bin/
        └── simulator.rs        # Load generator (legitimate baselines + attack scenarios)
```

---

## 7. Implementation Milestones (Vertical Slices)

- **Milestone 1 (Walking Skeleton):** Axum HTTP endpoint, domain structs, stubbed pipeline, and Docker Compose setup.
- **Milestone 2 (Hot State & Simulator):** Redis Lua sliding-window counters, bounded entity-link sets, idempotency keys, and `bin/simulator.rs` generating normal + card-testing/ATO traffic.
- **Milestone 3 (Dynamic Rules & Audit):** YAML rule parser, `ArcSwap` hot-reloader, Shadow Mode logging, and Postgres async batch persistence.
- **Milestone 4 (Analyst Loop & ML):** Minimal web dashboard for reviewing flagged transactions and submitting verdicts, plus `ort` ONNX model integration.
- **Milestone 5 (Benchmarks & Observability):** Prometheus latency histograms (`p50`, `p95`, `p99`), throughput test results, and README documentation.
