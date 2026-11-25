-- every decision the engine made, one row per transaction.
CREATE TABLE IF NOT EXISTS decisions (
    event_id   text PRIMARY KEY,
    user_id    text NOT NULL,
    device_id  text NOT NULL,
    ip         text NOT NULL,
    amount     double precision NOT NULL,
    city       text,
    event_time bigint NOT NULL,
    outcome    text NOT NULL CHECK (outcome IN ('allow', 'review', 'block')),
    score      integer NOT NULL,
    reasons    jsonb NOT NULL,
    decided_at timestamptz NOT NULL DEFAULT now()
);

-- finds the history of one user quickly.
CREATE INDEX IF NOT EXISTS decisions_user_idx ON decisions (user_id, event_time);

-- one case for every decision that was not allow, an analyst works through the open ones.
CREATE TABLE IF NOT EXISTS cases (
    case_id   bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    event_id  text NOT NULL UNIQUE REFERENCES decisions (event_id),
    status    text NOT NULL DEFAULT 'open' CHECK (status IN ('open', 'closed')),
    opened_at timestamptz NOT NULL DEFAULT now()
);

-- lists open cases quickly, closed cases are left out of the index.
CREATE INDEX IF NOT EXISTS cases_open_idx ON cases (case_id) WHERE status = 'open';