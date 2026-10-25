# Dev Archaeologist

A local-first macOS app that rebuilds what you worked on from evidence your tools already keep:
Claude Code and Codex session logs, Git history, file changes, the foreground app, and a zsh hook.
It groups that evidence into sessions with fixed rules, then (optionally) asks the Claude API to write
a daily journal from a short redacted digest. Every journal line cites the events behind it.

## Run

```sh
pnpm install
pnpm tauri dev                # app with hot reload
pnpm tauri build --debug      # .app bundle in src-tauri/target/debug/bundle/macos
```

Headless, same core:

```sh
cd src-tauri && cargo build --bin dev-arch
./target/debug/dev-arch discover 90     # what a first scan would read
./target/debug/dev-arch backfill 90     # read past logs into the local store
./target/debug/dev-arch day 2026-10-06  # print a day's sessions and achievements
./target/debug/dev-arch journal 2026-10-06
./target/debug/dev-arch sync            # catch up, sync tickets, run the daily journal and weekly report
./target/debug/dev-arch sync-agent install 30   # run `sync` every 30 min while the app is closed
./target/debug/dev-arch weekly 2026-10-19       # write that week's report to reports/2026-W43.md
./target/debug/dev-arch hook            # install the zsh hook file
cargo test --lib
```

## Data and privacy

- Store: `~/Library/Application Support/dev-archaeologist/devarch.db` (SQLite). Override with `DEV_ARCH_DB`.
- Agent logs: prompts are clipped to 200 chars and scrubbed; tool output is read only to count tests.
- Terminal: the hook sends the command name only, never arguments.
- The journal call sends one digest per day; projects in "never leave this Mac" are left out.
- Weekly reports: `reports/` next to the database, one Markdown file per ISO week.
- The sync agent reads the same Keychain items as the app. macOS asks once per new `dev-arch`
  binary; choose "Always Allow" so unattended runs don't wait on the prompt.
- API key: macOS Keychain (`dev-archaeologist` / `anthropic-api-key`), or `ANTHROPIC_API_KEY`.
