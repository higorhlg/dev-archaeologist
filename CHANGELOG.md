# Changelog

All notable changes to Dev Archaeologist. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the project uses
[Semantic Versioning](https://semver.org/). The version lives in `package.json`,
`src-tauri/Cargo.toml` and `src-tauri/tauri.conf.json`, kept equal.

## [0.7.0] - 2026-10-12

### Added
- Calendar meetings from the macOS Calendar app through EventKit, so any account already
  in Calendar works without a sign-in. A meeting is a timed, busy, not-declined event
  with two or more attendees or a video link; it is read once it has ended.
- Each meeting is its own session for its full length, even with no keyboard input, so
  it covers the gap it explains. Back-to-back meetings stay apart.
- Accounts with a work email (or its domain) go to the "Meetings" project, which is
  work; other accounts go to "Personal events".
- Jira keys in meeting titles feed ticket detection.
- Sources › Calendar: grant access, include or exclude each calendar, and read the last
  90 days again. History import includes 90 days of meetings.

## [0.6.0] - 2026-10-12

### Added
- A Work / Personal / All switch at the bottom of the left rail. It filters the day
  view, the project heatmap and reports, and it is saved. The personal side hides work
  hours and gaps, which belong to work.
- Sources › Work and personal: personal folders, a "Find in my git config" button that
  reads `includeIf` work identities again, and a "Save sides" button for these rules.

### Fixed
- Settings saves merge only the changed sections, so the rail switch, Reports and
  Sources never overwrite each other's changes.
- The folder and email boxes in Sources accept new lines while typing.

### Changed
- Reports follow the rail switch and no longer have their own side control.

## [0.5.0] - 2026-10-12

### Added
- Work and personal sides. A project is work when it sits in a work folder or its repo
  commits with a work email; both come from `includeIf "gitdir:…"` in the global git
  config, so no setup is needed. Any other path is personal. Blocks assigned by hand
  come from work hours and count as work. Sources lists every project with its side
  and lets the user pin one.
- Reports filter by side (work by default, saved with the other report options).
  Personal entries never go to Toggl, in CSV or through the API.
- Day view tags personal sessions.

### Fixed
- Parallel minutes are shared across both sides before the report filters, so work
  plus personal equals all.

## [0.4.0] - 2026-10-12

### Added
- Work hours: one range for the chosen work days with a lunch break, asked once after
  the history scan (and once on the next launch for existing installs), editable in
  Sources. Saving rebuilds gaps for the last 90 days.
- Gaps inside work hours, 15 minutes or longer, sorted by evidence: at the computer
  (keyboard or mouse input for half the gap), bridged (idle, at most 60 minutes, with a
  session on both edges), or unaccounted. Time outside work hours is never filled.
- Bridged minutes go to the next session when both sides are one project, else split.
  They count only after approval; approving a report entry approves its bridges.
- Day view: tinted work hours, hatched filled time in the lane that gets it, and an
  "Unaccounted" lane whose blocks take a project, a label, or "not work" in one tap.
- Reports: each day against the schedule, "Approve bridges", and "Fill to schedule"
  with a default project the user sets, never applied without a click.
- `dev-arch schedule`, `gaps`, `fill` and `approve-bridges`.

## [0.3.0] - 2026-10-12

### Added
- Model picker in Sources for every AI feature, listing the models the saved key can
  use with structured outputs (from the Models API, with a built-in list as fallback).
- Server-side refusal fallback (`fallbacks: "default"`) on models that support it.

### Changed
- Claude Sonnet 5.5 is the default model, at medium effort.

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
