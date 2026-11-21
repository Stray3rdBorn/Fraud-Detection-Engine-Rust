# Dataset Plan

This file documents the data behind my fraud detection engine: what data I have, which fraud types it can support, where it falls short, how I fix that, and how much I need to generate. The engine itself is built in Rust. Every number marked as an estimate is a starting point, and I will adjust it after the first runs.

## Where I stand

- Scale target: 5000 transactions per second, about 1 million users, one program, history saved to disk.
- Fraud types covered: velocity, amount anomaly, impossible travel, new device, blocklist, account takeover, linked accounts. Odd behavior is excluded because it creates too many false alarms.
- Detection method: rules plus per user statistics. No machine learning in version one.
- Test data: two public datasets plus my own generator, merged into one schema.
- Two separate benchmarks. The accuracy benchmark uses the merged data and measures detection. The load benchmark uses generated traffic and measures speed.

## The data I have

| Source       | What it is                                                            | Size                                       | What I use it for                                                                        |
| ------------ | --------------------------------------------------------------------- | ------------------------------------------ | ---------------------------------------------------------------------------------------- |
| Dataset 1    | Ecommerce fraud data, Apache 2.0 license, likely synthetic            | 151,112 transactions, 14,151 fraud (9.36%) | Fraud labels, shared devices and IPs, gap between signup and purchase                    |
| Dataset 2    | Bank transaction data, open source, no fraud label in its description | 2,512 rows                                 | Realistic honest behavior: amounts, cities, devices, login attempts, history per account |
| My generator | Code I write                                                          | Sized below                                | Everything the datasets do not have, plus the volume for the load test                   |

Dataset 1 fields: user id, signup time, purchase time, purchase value, device id, source, browser, sex, age, IP address, fraud label, plus an IP range to country table.

Dataset 2 fields: transaction id, account id, amount, date, type, city, device id, IP address, merchant id, balance, previous transaction date, channel, age, occupation, duration, login attempts.

## Working event shapes

`TransactionEvent` (what the engine decides on): event id, user id, timestamp, amount, device id, IP address, city or country, channel, and optionally signup time. Dataset 1 fills user, timestamp, amount, device, IP and signup time. Dataset 2 fills the same plus city and channel. The generator fills anything missing.

`AccountEvent` (what builds account state): user id, timestamp, and a kind of either `failed_login` or `password_changed`. Dataset 2 login attempts become failed logins. Password changes come only from my generator.

## Fraud types: possible or not

All seven are possible once the data is merged.

### 1. Velocity abuse

- **What it does:** Counts transactions per user, device and IP in a short window, using bucketed counters (12 buckets of 5 minutes).
- **What it is for:** Card testing and bot bursts.
- **Data:** Timestamps in both datasets. Dataset 1 shows fraud on one device at 1 second intervals.
- **Tradeoff:** Natural bursts are rare in the real data.
- **My fix:** I inject 500 bursts of about 15 events each.

### 2. Amount anomaly

- **What it does:** Compares the amount with that user's own running average and spread (a z score).
- **What it is for:** Stolen cards used for an unusually large purchase.
- **Data:** Dataset 2 amounts per account, and my generated users.
- **Tradeoff:** Users with few transactions have a weak baseline.
- **My fix:** Every generated honest user gets 10 or more events before any attack is injected. New users with no history get a default caution score for their first few transactions.

### 3. Impossible travel

- **What it does:** Divides the distance between the last and current location by the time between them, and flags speeds that are not possible.
- **What it is for:** One card used in two far apart places.
- **Data:** Dataset 2 city and timestamp.
- **Tradeoff:** Cities are US only, and I need coordinates for them. Dataset 1 country from IP is unreliable because 21,966 rows cannot be mapped.
- **My fix:** I build a city to coordinates table from the cities in dataset 2 and use that, not the IP country. I inject 500 pairs of transactions in two distant cities within too short a time.

### 4. New device risk

- **What it does:** Checks whether the device is in the user's known device set, capped in size, and raises the weight when the amount is large.
- **What it is for:** Stolen credentials used from the criminal's own device.
- **Data:** Device ids in dataset 2 per account and in dataset 1.
- **Tradeoff:** Most devices appear once in dataset 1, and real device changes are rare in dataset 2.
- **My fix:** I inject 500 new device transactions on established users, with a larger amount than usual.

### 5. Blocklist hits

- **What it does:** Looks up the device, IP and account in a blocklist.
- **What it is for:** Known bad actors.
- **Data:** A list I build.
- **Tradeoff:** If I build the list from the same data I test on, the list contains the answers.
- **My fix:** I split the timeline. The first 70% builds the blocklist and the baselines. The last 30% is the test. The list holds 2,000 entries, and I inject 500 test transactions from blocked entities.

### 7. Account takeover

- **What it does:** Combines weak signals: many failed logins, a recent password change, a new device, a large amount. When two or more fire together it adds a bonus.
- **What it is for:** Hijacked accounts.
- **Data:** Login attempts from dataset 2 become failed logins. No dataset has password changes.
- **Tradeoff:** The attack sequences are entirely my own, so detection on them tests my rule against my generator.
- **My fix:** I generate 500 sequences of failed logins, a password change, a new device and a large withdrawal, and I report this number as a test of the rule logic, not real world accuracy.

### 8. Linked accounts

- **What it does:** Counts distinct accounts per device (strong signal) and per IP (weak signal) over 7 days, with a cap of 50 and an expiry. It adds weight when a linked account was already blocked.
- **What it is for:** Fraud rings.
- **Data:** Dataset 1 shared devices and IPs. In dataset 1, 760 repeated IPs carry 7,634 fraud out of 8,360 transactions, and 6,175 repeated devices carry 10,141 out of 19,331.
- **Tradeoff:** Links are sparse in dataset 2, and the strong result in dataset 1 makes the rule look easier than it is.
- **My fix:** I inject 200 rings of about 10 accounts sharing a device, and I report dataset 1 and the injected rings separately.

## Tradeoffs that apply to the whole dataset

| Problem                                                                                                                                                                     | My fix                                                                                                                            |
| --------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------- |
| In dataset 1, all 7,600 transactions with 1 second between signup and purchase are fraud, and the fastest honest one took 137 seconds. This shortcut inflates every result. | I report accuracy twice, with those rows and without them. The result without them is the honest one.                             |
| I do not know how many transactions each user has in dataset 1.                                                                                                             | I count the repeats of `user_id` first. If every user appears once, dataset 1 serves only the device, IP and signup rules.        |
| Dataset 1 is likely synthetic, and a merged dataset has artificial links between columns.                                                                                   | The README says the results show the engine detects these patterns, not real world accuracy.                                      |
| Dataset 2 is small and has no fraud label.                                                                                                                                  | I treat it as honest behavior and use it as the model for how generated honest users spend.                                       |
| The fraud share in my merged data is about 6%, far above real life.                                                                                                         | I report the false alarm rate separately, and I run a second pass with the injected fraud thinned so fraud is about 1% of events. |
| The datasets cover different dates.                                                                                                                                         | I shift each onto one timeline and replay in timestamp order.                                                                     |

## How many I need to generate

### Accuracy benchmark (estimates)

| Item                    | Count                                         | Why this number                                                                           |
| ----------------------- | --------------------------------------------- | ----------------------------------------------------------------------------------------- |
| Dataset 1 rows          | 151,112, used as they are                     | Real labels                                                                               |
| Dataset 2 rows          | 2,512, used as they are                       | Honest background and the model for generated users                                       |
| Generated honest events | 400,000 across 40,000 users, about 10 each    | Gives each user a history for baselines and enough honest traffic to measure false alarms |
| Type 1 bursts           | 500 bursts, about 7,500 events                | 500 cases measure a detection rate to about plus or minus 3 points                        |
| Type 2 large amounts    | 500 events                                    | Same                                                                                      |
| Type 3 travel pairs     | 500 pairs, 1,000 events                       | Same                                                                                      |
| Type 4 new device       | 500 events                                    | Same                                                                                      |
| Type 5 blocklist        | 2,000 list entries, 500 test events           | Same                                                                                      |
| Type 7 takeover         | 500 sequences, about 3,500 events             | Same                                                                                      |
| Type 8 rings            | 200 rings, 2,000 accounts, about 8,000 events | 200 rings measure to about plus or minus 4 points                                         |

The injected attacks add about 21,500 events. The total is about 575,000 events, with about 540,000 honest ones. A false alarm rate of 0.5% then shows up as about 2,700 flagged honest events, which is enough to measure.

### Load benchmark (estimates)

| Item                  | Count             | Why this number                                                         |
| --------------------- | ----------------- | ----------------------------------------------------------------------- |
| Smoke run             | 100,000 events    | About 20 seconds at 5000 TPS, for quick checks while building           |
| Main run              | 3,000,000 events  | 10 minutes at 5000 TPS                                                  |
| Distinct users        | 1,000,000         | Matches my scale target, about 3 events each, so history actually grows |
| Distinct devices      | about 1,300,000   | Estimate, most users have one device and some have two or three         |
| Distinct IPs          | about 1,500,000   | Estimate                                                                |
| Attack pattern events | about 60,000 (2%) | Makes the rules that write state run under load                         |
| Optional peak run     | 600,000 events    | 10,000 TPS for 60 seconds, to see how much headroom exists              |

The events are generated before the test starts and held in memory, which is roughly 300 MB for the main run (estimate), so the generator does not take CPU from the engine. The generator uses a fixed seed, so the same data can be recreated.

I measure the engine twice, as a plain function call and through the HTTP API. I report the median delay and the p99 delay, which is the delay that 99 out of 100 requests beat. The README states the machine I ran the test on.

## Open items

1. The `user_id` repeat count in dataset 1.
2. The final `TransactionEvent` field list.

## Security notes

- Dataset 1 is Apache 2.0. If I redistribute any of its data I keep the license notice. I have not read the exact license text of dataset 2.
- The raw datasets stay out of the public repo. The repo holds the converters and the generator, not the data.
- User ids are stored as hashed 64 bit numbers inside the engine.
- Everything the generator produces is fake, so no real personal data enters the project.
