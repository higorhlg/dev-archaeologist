CREATE TABLE IF NOT EXISTS work_events (
  id         TEXT PRIMARY KEY,
  source     TEXT NOT NULL,             -- git | claude_code | codex | fs | app | shell
  source_id  TEXT NOT NULL,             -- id in the source, for dedupe
  ts         INTEGER NOT NULL,          -- unix ms
  kind       TEXT NOT NULL,             -- commit_created, test_failed, prompt, app_active …
  project    TEXT,                      -- main git repo name, resolved at insert
  path       TEXT,                      -- file path or working directory
  branch     TEXT,
  detail     TEXT,                      -- redacted, at most 200 chars
  meta       TEXT,                      -- JSON numbers: exit, pass, fail, plus, minus, dur_ms …
  UNIQUE (source, source_id)
);
CREATE INDEX IF NOT EXISTS work_events_ts ON work_events (ts);

CREATE TABLE IF NOT EXISTS sessions (
  id              TEXT PRIMARY KEY,
  day             TEXT NOT NULL,
  start_ts        INTEGER NOT NULL,
  end_ts          INTEGER NOT NULL,
  project         TEXT,
  branch          TEXT,
  label           TEXT,
  summary         TEXT,
  active_min      INTEGER NOT NULL DEFAULT 0,
  agent_only_min  INTEGER NOT NULL DEFAULT 0,
  state           TEXT NOT NULL CHECK (state IN ('observed','inferred','edited','confirmed','excluded')),
  confidence      TEXT CHECK (confidence IN ('low','medium','high'))
);
CREATE INDEX IF NOT EXISTS sessions_day ON sessions (day);

CREATE TABLE IF NOT EXISTS session_events (
  session_id TEXT NOT NULL,
  event_id   TEXT NOT NULL,
  PRIMARY KEY (session_id, event_id)
);

CREATE TABLE IF NOT EXISTS achievements (
  id          TEXT PRIMARY KEY,
  day         TEXT NOT NULL,
  rule        TEXT NOT NULL,
  title       TEXT NOT NULL,
  detail      TEXT NOT NULL,
  evidence_id TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS journals (
  day        TEXT PRIMARY KEY,
  body       TEXT NOT NULL,             -- JSON: [{ text, cites: [event id] }]
  state      TEXT NOT NULL CHECK (state IN ('inferred','confirmed')),
  model      TEXT,
  created_at INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS rules (
  id   TEXT PRIMARY KEY,
  kind TEXT NOT NULL,                   -- project_for_path | label_for_branch | exclude_app
  body TEXT NOT NULL                    -- JSON
);

CREATE TABLE IF NOT EXISTS settings (
  key   TEXT PRIMARY KEY,
  value TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS toggl_sync (
  entry_key    TEXT PRIMARY KEY,         -- session id, or "day|project" for a merged entry
  toggl_id     INTEGER NOT NULL,
  duration_min INTEGER NOT NULL,
  description  TEXT NOT NULL,
  sent_at      INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS ticket_overrides (
  session_id TEXT PRIMARY KEY,
  ticket     TEXT NOT NULL                -- '' means "this session has no ticket"
);
