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

CREATE TABLE IF NOT EXISTS work_gaps (
  id        TEXT PRIMARY KEY,             -- stable hash of day and start
  day       TEXT NOT NULL,
  start_ts  INTEGER NOT NULL,
  end_ts    INTEGER NOT NULL,
  kind      TEXT NOT NULL CHECK (kind IN ('at_computer','bridged','unaccounted')),
  state     TEXT NOT NULL CHECK (state IN ('open','approved','assigned','not_work')),
  targets   TEXT NOT NULL DEFAULT '[]',   -- JSON [[session id, minutes]]: who gets the minutes
  project   TEXT,                         -- set when the user assigns the block
  label     TEXT
);
CREATE INDEX IF NOT EXISTS work_gaps_day ON work_gaps (day);

CREATE TABLE IF NOT EXISTS ticket_suggestions (
  session_id TEXT PRIMARY KEY,
  ticket     TEXT NOT NULL,
  reason     TEXT NOT NULL,               -- why the ticket fits, shown to the user
  by_ai      INTEGER NOT NULL DEFAULT 0,  -- 1 when the AI tie-break picked it
  dismissed  INTEGER NOT NULL DEFAULT 0,  -- 1 when the user said no; the same ticket is never suggested again
  via        TEXT                         -- the GitHub ref that led to a Jira ticket; kept as the GitHub ref on use
);

CREATE TABLE IF NOT EXISTS github_ref_overrides (
  session_id TEXT PRIMARY KEY,
  ref        TEXT NOT NULL                -- insight-ui#128; '' means "this session has no GitHub ref"
);

CREATE TABLE IF NOT EXISTS ticket_info (
  ticket     TEXT PRIMARY KEY,            -- SHOP-1261 or insight-ui#128
  title      TEXT NOT NULL,
  url        TEXT NOT NULL,
  tracker    TEXT NOT NULL,               -- jira | github
  fetched_at INTEGER NOT NULL,
  epic_key   TEXT,                        -- the Jira epic the ticket belongs to
  epic_title TEXT
);
