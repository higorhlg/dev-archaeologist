# Changelog

All notable changes to Dev Archaeologist. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the project uses
[Semantic Versioning](https://semver.org/). The version lives in `package.json`,
`src-tauri/Cargo.toml` and `src-tauri/tauri.conf.json`, kept equal.

## [0.2.0] - 2026-10-12

### Added
- Reports view: this/last week or month, entries per ticket, per session or per project,
  review flags, one-click approval of confident sessions, and inline edits of
  descriptions and tickets.
- Ticket finder: Jira keys trusted only once seen in a branch or commit scope, GitHub
  issues and PRs from branches, squash refs and links. Tickets go in the description,
  as a Toggl tag, or nowhere. The last applied report options are saved.
- "Label with AI" writes journals for days in the range that still lack AI labels.
- Toggl: CSV export of approved entries to ~/Downloads, project mapping (Toggl project,
  client, billable, never send), and a confirmed send through the API v9 that records
  each entry so it is never sent twice. Token in the macOS Keychain.

### Fixed
- AI session labels survive the per-minute rebuild of today's sessions.
- An agent turn ends at a pause longer than 15 minutes and counts at most 3 hours.
- Agent tool calls and test runs count as agent activity in their own project.
- Report minutes share overlapping parallel sessions, so a day adds up to its
  wall-clock active time.

## [0.1.0] - 2026-10-12

### Added
- Local event store (SQLite) with deduplicated `WorkEvent`s and redaction of secrets,
  secret files and noise folders before anything is written.
- Backfill from Claude Code (every `~/.claude*` profile), Codex rollouts and local Git.
- Live collectors on macOS: foreground app and idle time, FSEvents file changes,
  Git and agent-log polling, and a zsh hook that sends command names only.
- Rule-based sessions per project, with overlapping sessions for parallel agents,
  wall-clock day totals, and 5-minute rounding.
- Achievements from fixed rules, including "Fixed N bugs caused by one typo".
- Daily journal through the Claude API from a redacted digest; lines without valid
  citations are dropped. API key in the macOS Keychain.
- Corrections: confirm, edit, hide, and edits that become rules for later sessions.
- Tauri app: onboarding, day strata view with evidence drawer, journal, project
  heatmap, and sources/privacy settings.
- Report core: ticket detection (Jira keys from branches and commit scopes, GitHub
  issues and PRs), Toggl CSV export, and a Toggl API v9 client. `dev-arch report`.
- `dev-arch` CLI over the same core.
