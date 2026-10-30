# Changelog

All notable changes to Dev Archaeologist. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the project uses
[Semantic Versioning](https://semver.org/). The version lives in `package.json`,
`src-tauri/Cargo.toml` and `src-tauri/tauri.conf.json`, kept equal.

## [Unreleased]

### Added
- `dev-arch report` takes `--fit HOURS` and `--csv [PATH]` (with `--include-unapproved` to
  write entries still waiting for approval), and prints each entry's laid-out times and the
  measured time beside the fitted one.
- `dev-arch report --fit HOURS --drift MIN` lets each fitted day move up to about MIN minutes
  from HOURS, toward how long that day's presence ran against the other days, in quarter-hour
  steps. The days together still add up to HOURS each. `ReportQuery.fit_drift_min` carries it.
- `dev-arch report --html [PATH]` writes the report as a readable page laid out like a time
  tracker's list view: a week total with a bar per day, time per project, and per day a
  timeline (time away hatched) above one row per entry piece with its project, tickets, time
  range and duration.
- Ticket mode "Epic and ticket" (`epic_ticket`) writes descriptions as `EPIC TICKET`, such as
  `SHOP-1199 SHOP-1331`, from the epic Jira gives the ticket. Rules in the Toggl settings name a
  fallback epic for tickets without one (in its own Jira project only), the description of an
  entry with no ticket, meeting descriptions by title, and the Jira projects that count. The
  timesheet page shows the description above the work label.
- A session with a GitHub pull request and no Jira ticket takes the Jira ticket the pull request
  is for: the one key its title names, else the one key its body names. A release PR naming
  several keys names none. Find tickets reads and keeps the body's keys for each pull request.
- With `use_suggestions` on, a session with no Jira ticket takes its open suggestion that came
  through a pull request (a PR names it, or Jira links the PR). Word-match suggestions never do.
- `dev-arch toggl learn DETAILED_REPORT.csv` reads a Toggl detailed-report export and keeps,
  for each ticket, the epic it was logged under for the most time. `epic_ticket` descriptions
  use that epic before Jira's.
- With `fold_no_ticket` on, `epic_ticket` work without a ticket goes to the ticketed work
  nearest it in time that day instead of taking the no-ticket text.
- Jira tickets carry whether the user is their assignee or reporter. Projects listed in the
  description rules' `mine_only_projects`, such as `PLAT`, file an entry under their ticket only
  when it is the user's; any other of their tickets counts as no ticket.
- Recurring entries in the Toggl settings, such as a weekly company meeting: a weekday, start
  time, length, description, Toggl project, client and billable flag. Reports add one to each
  worked day on that weekday, already approved; fitted days hold it at its time like a meeting
  and count it toward the day.
- `dev-arch report --tickets MODE` picks the ticket mode.
- `dev-arch toggl recent [DAYS]` prints the time entries already in Toggl, read only.
- `dev-arch away DAY [HH:MM HH:MM | clear]` records time away from work. Fitted days leave it
  empty and do not count it as presence; work that runs into it continues after it.
- `dev-arch sync` warns when the calendar source is on but the binary has no Calendar access,
  so meetings are not read.
- `dev-arch calendar [connect | reimport]` shows Calendar access, asks macOS for it, and reads
  90 days of meetings again. The `dev-arch` binary carries its own Info.plist with the Calendar
  usage text, so macOS can ask for access outside the app bundle.

### Changed
- Fitted days are written in quarter hours: entries scale in 15-minute steps, an entry too short
  to hold one folds into its nearest neighbour, split pieces land on whole steps, meetings snap
  to the nearest quarter hours, and CSV start times drop their seconds.
- "Fit days to N h" lays entries inside the day's presence windows (session spans joined over
  breaks up to 30 minutes). An entry that would start in a break waits for the next window
  while the windows left hold the rest of the day, so lunch and time away stay empty unless
  the day needs them.
- Fitted days keep meetings at their calendar time and length, counted toward the day. Work
  that runs into a meeting splits into pieces before and after it; the CSV writes one row per
  piece and Toggl gets one entry per piece. A piece shorter than a quarter hour moves the whole
  entry past the meeting or away time instead.
- The daily automation rewrites a journal written before its day ended, such as one made
  mid-day for a time sheet. Confirmed journals stay.

- A journal labels confirmed sessions whose label is still the one their evidence gives, and
  they stay confirmed. Edited sessions keep their labels, and meetings keep their calendar
  title.

### Fixed
- A journal whose answer labels fewer than half the day's sessions is asked for again once,
  and fails rather than standing as the day's journal with raw prompts as session labels.

## [0.11.0] - 2026-10-25

### Added
- Pull requests from agent shell commands. A `gh pr` command a Claude Code or Codex session
  ran sets the session's GitHub ref when it names the repo and number: one the session opened
  counts most, then one it edited, commented on, reviewed, merged or checked out, then one it
  only viewed (less again for a repo other than the session's project). Each pull request
  counts once per session, so repeated status checks do not outweigh the PR the session
  opened. Pull requests a session only viewed stay out of the text Find tickets matches.
- Codex sessions read the pull request `gh pr create` opened from its output, and the branch
  a `git switch -c`, `git checkout -b`, `git worktree add -b` or `gh pr create --head` created.
- The next launch reads 90 days of agent logs again to find them.
- "Fit days to N h" in Reports (8 by default) scales each work day's entries in proportion so
  the day adds up to N hours, in 5-minute steps, laid back to back so none overlap. The CSV
  and Toggl export use the fitted time. Weekends and days off the schedule keep their
  measured time, entries already sent to Toggl keep theirs, and each entry and day shows the
  measured time beside the fitted one.
- The app opens the database `DEV_ARCH_DB` names, like `dev-arch`.

### Changed
- The weekly report reads more easily: durations such as `35m`, each project's share, an epic
  column only when a ticket has an epic, a Days table even without a work schedule, and
  "Needs attention" listing only the entries without a ticket, longest first, plus one line
  on how much is still unapproved.
- The automatic sync line in Reports says "nothing new" instead of "0 applied, 0 suggested".

### Fixed
- A lowercase Jira key in a branch name, such as `feat/shop-1068-pricing`, names the ticket
  when its prefix is a known Jira prefix.

## [0.10.0] - 2026-10-25

### Added
- Tickets from agent sessions. A pull request a Claude Code session opened or pushed to sets
  the session's GitHub ref, and a Jira ticket an agent moved, edited, commented on or logged
  work on sets its Jira ticket (a ticket it only opened counts less). Their prefixes count as
  known Jira prefixes. The next launch reads 90 days of agent logs again to find them.
- Tickets applied without a click. When a session's work names a Jira key exactly, or its own
  pull request leads to a Jira ticket (the PR title names the key, or the Jira ticket links
  the PR), Find tickets applies that Jira ticket to the session and Reports marks it "auto";
  "Not this" takes it back. Word matches, AI picks and entries of several sessions named by
  only one of them still wait for "Use".
- Automatic ticket sync: at launch and every 30 minutes while the app runs (Sources sets the
  interval or turns it off), session by session over the last two weeks of work, without the
  AI tie-break. Reports
  shows when it last ran and reloads when it lands.
- `dev-arch sync` reads every source since the last poll, syncs tickets when the interval is
  due (`--tickets` forces it) and runs the daily work, which runs once a day across the app
  and the agent. `dev-arch sync-agent install [MIN]` installs a LaunchAgent that runs it while the app
  is closed; `dev-arch sync-agent remove` takes it out.
- Daily journals: each morning the app writes the AI journal for finished days of the past
  week that have none, when an API key is saved. A switch in Sources turns it off.
- Weekly report: on Monday the app writes last week's work report as Markdown next to the
  database (`reports/2026-W43.md`): time per project and per ticket with titles and epics,
  each day against the work hours, the journal, and entries that still need a ticket or
  approval. "Open weekly reports" in Sources writes this week's so far and shows the folder.
  `dev-arch weekly [DAY]` writes one by hand.
- The journal digest carries tickets applied without a click.
- The database waits up to 30 seconds for a lock, so the app and `dev-arch` can write at once.

### Fixed
- The app reads everything since it last ran instead of the last day only, and rebuilds every
  day in between, so days with the app closed and work after midnight are no longer lost.

## [0.9.0] - 2026-10-13

### Added
- Jira ticket and GitHub ref are separate on each report entry. Each has its own chip, is
  found from evidence on its own, and can be set by hand. Toggl files the entry under the
  Jira ticket, or under the GitHub ref when there is no Jira ticket.
- Epics: Jira tickets carry their parent epic. Reports shows it next to the ticket and in
  the ticket's hover text.
- Hover text on both chips shows the ticket title. Find tickets fetches the title and epic
  of every Jira ticket in the report, and the title of each GitHub ref it has not seen.

### Changed
- Using a Jira suggestion keeps the GitHub ref it came through (a PR naming the key, or a
  PR the Jira ticket links), and a GitHub key stored as the ticket moves to the GitHub ref.
- Typing a `repo#N` key in the ticket box sets the GitHub ref.

## [0.8.2] - 2026-10-13

### Added
- Find tickets asks Jira which ticket links each GitHub pull request in play (the PR URL in
  its summary, description or a comment) and suggests that ticket, whoever owns it.

### Changed
- The Jira suggestion next to a GitHub ticket appears for tickets you set as well as
  guessed ones. "Use" replaces the GitHub ticket.

## [0.8.1] - 2026-10-13

### Changed
- Find tickets tries Jira first: Jira tickets, and pull requests whose title names a Jira
  key. A plain GitHub issue or pull request is suggested only when no Jira ticket fits, or
  when the work names it and the Jira tickets are only a close call.
- Find tickets also checks entries whose ticket is a GitHub issue or pull request the app
  guessed, and offers a Jira key next to it. "Use" replaces the guessed ticket.

### Fixed
- Commits GitHub made when it merged a pull request (committer `noreply@github.com`) no
  longer count as work on the day they were fetched, and no longer give a session their
  `(#N)` ticket. Stash commits ("WIP on", "index on") are not counted either. Ones stored
  earlier are removed and their days rebuilt.

## [0.8.0] - 2026-10-12

### Added
- Ticket sources in Sources. Jira connects with an API token: "Get a token" opens the
  Atlassian token page in the browser, and the token goes to the Keychain. GitHub uses
  the accounts the `gh` CLI is signed in with, so it needs no new sign-in. Both only read.
- "Find tickets" in Reports. For each entry without a ticket it asks the trackers for
  your own tickets in the range (Jira: assigned to you during the range, or reported or
  logged by you; GitHub: issues and pull requests that involve you in the report's
  repos). It ranks them by repo, Jira prefix, day, shared words and ticket numbers
  named in the work, then suggests the best one. Nothing is set until you accept.
- Close calls go to the AI in one call per run, with only the entry outline and the
  candidate titles. Projects that never leave the Mac, or a missing API key, keep the
  best rule-based guess.
- A pull request whose title names a known Jira key suggests that Jira key.
- Suggestions show the ticket title and why it fits, with Use, Not this and Other.
  "Not this" stops that ticket from coming back for those sessions. "Use N suggested
  tickets" accepts them all.
- Tickets with a known link open in the browser from Reports.
- The journal digest carries the tickets you set or accepted, and the journal may name
  them. It names no other ticket.
- `dev-arch find-tickets FROM TO [work|personal|all]`.

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
