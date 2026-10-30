//! `dev-arch`: headless access to the same core the app uses.
//!
//!   dev-arch discover [days]     what a first scan would read
//!   dev-arch backfill [days]     read past logs into the local store
//!   dev-arch day [YYYY-MM-DD]    print one day's sessions and achievements
//!   dev-arch journal YYYY-MM-DD  write the day's journal with the Claude API
//!   dev-arch report FROM TO [session|ticket_day|project_day] [work|personal|all]
//!                [--tickets prefix|tag|off|epic_ticket] [--fit HOURS [--drift MIN]]
//!                [--csv [PATH]] [--html [PATH]] [--include-unapproved]
//!                fit work days to HOURS inside their presence windows, each day moving up to
//!                about MIN minutes toward its presence; write the Toggl CSV and a readable HTML page
//!   dev-arch find-tickets FROM TO [work|personal|all]  suggest tickets from Jira and GitHub
//!   dev-arch schedule [HH:MM HH:MM | off]  set Mon–Fri hours with lunch 12–13
//!   dev-arch gaps [YYYY-MM-DD]   print the day's gaps inside work hours
//!   dev-arch fill DAY PROJECT     assign a day's unaccounted blocks to one project
//!   dev-arch approve-bridges DAY approve a day's bridged gaps
//!   dev-arch away DAY [HH:MM HH:MM | clear]  record time away; fitted days leave it empty
//!   dev-arch toggl recent [DAYS] print the time entries already in Toggl (read only)
//!   dev-arch calendar [connect|reimport]  Calendar access; ask for it; read 90 days of meetings again
//!   dev-arch sync [--tickets]    read every source since the last poll, sync tickets when due
//!                                (or now with --tickets), run the daily automation
//!   dev-arch weekly [YYYY-MM-DD] write the work report for the week holding the day
//!   dev-arch sync-agent install [MIN] | remove  run `sync` every MIN minutes while the app is closed
//!   dev-arch daemon              run the live collectors in the foreground
//!   dev-arch set-key             read an API key from stdin into the Keychain
//!   dev-arch hook                install the zsh hook file and print the source line

use std::io::Read;

use anyhow::{bail, Result};
use chrono::Local;
use devarch::{api, daemon, infer::journal, pipeline::Pipeline, store::{default_db_path, Store}};

fn arg_days(args: &[String]) -> i64 {
    args.get(2).and_then(|d| d.parse().ok()).unwrap_or(90)
}

fn arg_day(args: &[String]) -> String {
    args.get(2).cloned().unwrap_or_else(|| Local::now().format("%Y-%m-%d").to_string())
}

fn print_day(p: &Pipeline, day: &str) -> Result<()> {
    let v = api::day_view(p, day, devarch::reports::ContextFilter::All)?;
    println!("{} · {} min active · {} sessions", v.day, v.active_min, v.sessions.len());
    for s in &v.sessions {
        let fmt = |ms: i64| chrono::DateTime::from_timestamp_millis(ms).unwrap().with_timezone(&Local).format("%H:%M").to_string();
        let chips: Vec<String> = s.chips.iter().map(|c| format!("{} {}", c.n, c.label)).collect();
        println!(
            "  {}–{}  {:>4} min  [{}|{}]  {} · {}\n                 {}",
            fmt(s.session.start_ts),
            fmt(s.session.end_ts),
            s.session.active_min,
            s.session.state,
            s.session.confidence.as_deref().unwrap_or("-"),
            s.session.project.as_deref().unwrap_or("(no project)"),
            s.session.label.as_deref().unwrap_or("-"),
            chips.join(" · ")
        );
    }
    for a in &v.achievements {
        println!("  🏅 {} — {}", a.title, a.detail);
    }
    if let Some(j) = &v.journal {
        println!("  journal ({}):", j.state);
        for l in &j.lines {
            println!("    {}", l.text);
        }
    }
    Ok(())
}

/// The value after `flag`, as in `--fit 8`.
fn flag_value<'a>(args: &'a [String], flag: &str) -> Option<&'a str> {
    args.iter().position(|a| a == flag).and_then(|i| args.get(i + 1)).map(String::as_str)
}

/// `args` without the flags `report` reads and their values.
fn positional(args: &[String]) -> Vec<String> {
    let mut out = Vec::new();
    let mut it = args.iter().peekable();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--fit" | "--drift" | "--tickets" => {
                it.next();
            }
            "--csv" | "--html" => {
                it.next_if(|v| !v.starts_with("--"));
            }
            "--include-unapproved" => {}
            _ => out.push(a.clone()),
        }
    }
    out
}

/// `report FROM TO [grouping] [context] [--tickets prefix|tag|off|epic_ticket] [--fit HOURS [--drift MIN]] [--csv [PATH]] [--html [PATH]] [--include-unapproved]`.
/// Prints each entry with its laid-out times; with `--csv`, writes the Toggl CSV of approved
/// work entries (all work entries with `--include-unapproved`) to PATH or ~/Downloads.
fn print_report(p: &Pipeline, args: &[String]) -> Result<()> {
    let pos = positional(args);
    let today = Local::now().format("%Y-%m-%d").to_string();
    let fit_hours: Option<f64> = flag_value(args, "--fit").map(str::parse).transpose()?;
    let q = devarch::reports::ReportQuery {
        from: pos.get(2).cloned().unwrap_or_else(|| today.clone()),
        to: pos.get(3).cloned().unwrap_or(today),
        grouping: serde_json::from_value(serde_json::json!(pos.get(4).map(String::as_str).unwrap_or("ticket_day")))?,
        tickets: serde_json::from_value(serde_json::json!(flag_value(args, "--tickets").unwrap_or("prefix")))?,
        context: serde_json::from_value(serde_json::json!(pos.get(5).map(String::as_str).unwrap_or("work")))?,
        fit_day_min: fit_hours.map(|h| (h * 60.0).round() as i64),
        fit_drift_min: flag_value(args, "--drift").map(str::parse).transpose()?,
    };
    let r = api::report(p, &q)?;
    println!("{} min total · {} approved · {} to review · prefixes {:?}", r.total_min, r.approved_min, r.review_count, r.known_prefixes);
    let at = |ms: i64| chrono::DateTime::from_timestamp_millis(ms).unwrap().with_timezone(&Local).format("%H:%M").to_string();
    let mut day_total: std::collections::BTreeMap<&str, (i64, i64)> = Default::default();
    for e in r.entries.iter().filter(|e| !e.skip) {
        let t = day_total.entry(&e.day).or_default();
        t.0 += e.duration_min;
        t.1 += e.measured_min;
        let tickets = [&e.jira, &e.github].into_iter().flatten().map(|t| format!("{} ({})", t.key, t.source)).collect::<Vec<_>>().join(" ");
        let times = e.blocks().iter().map(|b| format!("{}–{}", at(b.start_ts), at(b.start_ts + b.duration_min * 60_000))).collect::<Vec<_>>().join(",");
        println!(
            "  {} {:<11} {:>4}m (measured {:>3}m)  {:<22} {:<28} {}",
            e.day,
            times,
            e.duration_min,
            e.measured_min,
            e.toggl_project,
            if tickets.is_empty() { "-".to_string() } else { tickets },
            devarch::event::clip(&e.label, 70)
        );
    }
    for (day, (min, measured)) in &day_total {
        println!("  {day}: {}h{:02} (measured {}h{:02})", min / 60, min % 60, measured / 60, measured % 60);
    }
    if args.iter().any(|a| a == "--csv") {
        write_report_csv(p, &r, args)?;
    }
    if args.iter().any(|a| a == "--html") {
        write_report_html(p, &r, args)?;
    }
    Ok(())
}

/// The path after `flag`, else `name` in ~/Downloads.
fn output_path(args: &[String], flag: &str, name: String) -> Result<std::path::PathBuf> {
    Ok(match flag_value(args, flag).filter(|v| !v.starts_with("--")) {
        Some(path) => std::path::PathBuf::from(path),
        None => dirs::download_dir().or_else(dirs::home_dir).ok_or_else(|| anyhow::anyhow!("no Downloads folder"))?.join(name),
    })
}

/// Writes the report as a readable HTML page, with the same entries `--csv` would write.
fn write_report_html(p: &Pipeline, r: &devarch::reports::Report, args: &[String]) -> Result<()> {
    let entries: Vec<&devarch::reports::ReportEntry> = if args.iter().any(|a| a == "--include-unapproved") {
        devarch::reports::exportable_unapproved(r).collect()
    } else {
        devarch::reports::exportable(r).collect()
    };
    let away = devarch::away::spans_ms(&p.store, &r.query.from, &r.query.to)?;
    let week_min = r.query.fit_day_min.unwrap_or(8 * 60) * 5;
    let path = output_path(args, "--html", format!("timesheet-{}-to-{}.html", r.query.from, r.query.to))?;
    std::fs::write(&path, devarch::timesheet_html::render(r, &entries, &away, week_min))?;
    println!("wrote {}", path.display());
    Ok(())
}

fn write_report_csv(p: &Pipeline, r: &devarch::reports::Report, args: &[String]) -> Result<()> {
    let email = devarch::settings::Settings::load(&p.store)?.toggl.email;
    let unapproved = args.iter().any(|a| a == "--include-unapproved");
    let csv = if unapproved {
        devarch::reports::to_csv(devarch::reports::exportable_unapproved(r), &email)
    } else {
        devarch::reports::to_csv(devarch::reports::exportable(r), &email)
    };
    let path = output_path(args, "--csv", format!("toggl-{}-to-{}.csv", r.query.from, r.query.to))?;
    std::fs::write(&path, csv)?;
    println!("wrote {}", path.display());
    if !unapproved && r.review_count > 0 {
        println!("  {} unapproved entries left out; approve them in Reports or pass --include-unapproved", r.review_count);
    }
    Ok(())
}

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let db = default_db_path();
    let p = Pipeline::new(Store::open(&db)?);
    match args.get(1).map(String::as_str) {
        Some("discover") => println!("{}", serde_json::to_string_pretty(&api::discover(&p, arg_days(&args))?)?),
        Some("backfill") => {
            let started = std::time::Instant::now();
            let r = api::run_backfill(&p, arg_days(&args))?;
            println!("{} · {:.1}s", serde_json::to_string(&r)?, started.elapsed().as_secs_f32());
        }
        Some("day") => print_day(&p, &arg_day(&args))?,
        Some("rebuild") => println!("{} sessions", api::rebuild_day(&p, &arg_day(&args))?),
        Some("journal") => {
            let day = arg_day(&args);
            api::generate_journal(&p, &day)?;
            print_day(&p, &day)?;
        }
        Some("report") => print_report(&p, &args)?,
        Some("find-tickets") => {
            let today = Local::now().format("%Y-%m-%d").to_string();
            let q = devarch::reports::ReportQuery {
                from: args.get(2).cloned().unwrap_or_else(|| today.clone()),
                to: args.get(3).cloned().unwrap_or(today),
                grouping: devarch::reports::Grouping::TicketDay,
                tickets: devarch::reports::TicketMode::Prefix,
                context: serde_json::from_value(serde_json::json!(args.get(4).map(String::as_str).unwrap_or("work")))?,
                fit_day_min: None,
                fit_drift_min: None,
            };
            let r = api::find_tickets(&p, &q)?;
            println!("checked {} · suggested {} ({} by AI) · {:?}", r.checked, r.suggested, r.by_ai, r.looked_in);
            for e in &r.errors {
                println!("  error: {e}");
            }
            for e in api::report(&p, &q)?.entries.iter().filter(|e| e.suggestion.is_some()) {
                let s = e.suggestion.as_ref().unwrap();
                println!("  {} {:<20} {:<14} {} · {}", e.day, e.toggl_project, s.info.key, devarch::event::clip(&s.info.title, 50), s.reason);
            }
        }
        Some("schedule") => {
            let schedule = match args.get(2).map(String::as_str) {
                Some("off") => None,
                _ => Some(devarch::settings::WorkSchedule {
                    start: args.get(2).cloned().unwrap_or_else(|| "08:00".into()),
                    end: args.get(3).cloned().unwrap_or_else(|| "17:00".into()),
                    ..Default::default()
                }),
            };
            api::save_schedule(&p, schedule)?;
            println!("schedule saved; gaps rebuilt for 90 days");
        }
        Some("gaps") => {
            let day = arg_day(&args);
            let v = api::day_view(&p, &day, devarch::reports::ContextFilter::All)?;
            if let Some(s) = &v.schedule {
                println!("{day}: schedule {} · observed {} · filled {} · bridge pending {} · unaccounted {}", s.schedule_min, s.observed_min, s.filled_min, s.pending_bridge_min, s.unaccounted_min);
            }
            for g in &v.gaps {
                let at = |ms: i64| chrono::DateTime::from_timestamp_millis(ms).unwrap().with_timezone(&Local).format("%H:%M").to_string();
                println!("  {}–{}  {:>3} min  {:<12} {:<9} → {:?}", at(g.start_ts), at(g.end_ts), g.minutes(), g.kind, g.state, g.targets.iter().map(|t| t.1).collect::<Vec<_>>());
            }
        }
        Some("fill") => {
            let day = arg_day(&args);
            let project = args.get(3).cloned().unwrap_or_else(|| "Meetings".into());
            println!("filled {} min", api::fill_to_schedule(&p, &day, &project)?);
        }
        Some("approve-bridges") => {
            let day = arg_day(&args);
            println!("approved {} bridges", api::approve_bridges(&p, &day, &day)?);
        }
        Some("away") => {
            let day = arg_day(&args);
            match (args.get(3).map(String::as_str), args.get(4)) {
                (Some("clear"), _) => devarch::away::clear(&p.store, &day)?,
                (Some(start), Some(end)) => devarch::away::add(&p.store, &day, start, end)?,
                (None, _) => {}
                _ => bail!("usage: dev-arch away DAY [HH:MM HH:MM | clear]"),
            }
            let spans = devarch::away::load(&p.store)?.remove(&day).unwrap_or_default();
            println!("{day} away: {}", if spans.is_empty() { "-".into() } else { spans.iter().map(|(s, e)| format!("{s}–{e}")).collect::<Vec<_>>().join(", ") });
        }
        Some("toggl") => match args.get(2).map(String::as_str) {
            Some("recent") => {
                let days = args.get(3).and_then(|d| d.parse().ok()).unwrap_or(14);
                let token = devarch::toggl::token().ok_or_else(|| anyhow::anyhow!("no Toggl token"))?;
                let account = devarch::toggl::account(&token)?;
                let names: std::collections::HashMap<i64, String> =
                    devarch::toggl::projects(&token, account.workspace_id)?.into_iter().map(|p| (p.id, p.name)).collect();
                for e in devarch::toggl::recent_entries(days)? {
                    let project = e.project_id.and_then(|id| names.get(&id).cloned()).unwrap_or_else(|| "-".into());
                    println!("  {} {:>6}m  {:<24} {}", &e.start[..16.min(e.start.len())], e.duration.max(0) / 60, project, e.description.unwrap_or_default());
                }
            }
            Some("learn") => {
                let path = args.get(3).ok_or_else(|| anyhow::anyhow!("usage: dev-arch toggl learn DETAILED_REPORT.csv"))?;
                let epics = devarch::toggl_history::import(&p.store, &std::fs::read_to_string(path)?)?;
                println!("learned the epic of {} tickets from {path}", epics.len());
                for (ticket, epic) in epics.iter().rev().take(8) {
                    println!("  {ticket} → {epic}");
                }
            }
            _ => bail!("usage: dev-arch toggl recent [DAYS] | toggl learn DETAILED_REPORT.csv"),
        },
        Some("calendar") => {
            let n = match args.get(2).map(String::as_str) {
                Some("connect") => api::connect_calendar(&p).map(|_| None)?,
                Some("reimport") => Some(api::reimport_meetings(&p)?),
                None => None,
                _ => bail!("usage: dev-arch calendar [connect | reimport]"),
            };
            let s = api::calendar_status(&p)?;
            println!("calendar access: {:?} · {} calendars · excluded {:?}", s.access, s.calendars.len(), s.excluded);
            if let Some(n) = n {
                println!("{n} new meetings");
            }
        }
        Some("status") => {
            let s = api::status(&p)?;
            println!("events: {:?}\ndays: {}\napi key: {}", s.event_counts, s.days.len(), if s.has_api_key { "set" } else { "not set" });
        }
        Some("sync") => {
            let started = std::time::Instant::now();
            let n = daemon::catch_up(&p, &mut devarch::backfill::AgentTail::default())?;
            println!("{} · {n} new events", Local::now().format("%Y-%m-%d %H:%M"));
            let calendar_access = devarch::adapters::calendar::access();
            if devarch::settings::Settings::load(&p.store)?.sources.calendar && calendar_access != devarch::adapters::calendar::Access::Granted {
                println!("  warning: calendar is on but this binary has no Calendar access ({calendar_access:?}); meetings were not read");
            }
            if devarch::sync::ticket_sync_due(&p.store)? || args.get(2).map(String::as_str) == Some("--tickets") {
                let s = devarch::sync::sync_tickets(&p.store)?;
                println!(
                    "tickets {}..{}: checked {} · suggested {} · applied {} · {:.1}s",
                    s.from, s.to, s.checked, s.suggested, s.applied, started.elapsed().as_secs_f32()
                );
                for e in &s.errors {
                    println!("  error: {e}");
                }
            }
            if let Some(run) = devarch::sync::run_daily(&p)? {
                println!("daily: journals {:?} · weekly report {}", run.journals, run.weekly_report.as_deref().unwrap_or("-"));
                for e in &run.errors {
                    println!("  error: {e}");
                }
            }
        }
        Some("weekly") => {
            let day = chrono::NaiveDate::parse_from_str(&arg_day(&args), "%Y-%m-%d")?;
            println!("{}", devarch::weekly::write_week(&p, devarch::weekly::week_start(day))?.display());
        }
        Some("sync-agent") => match args.get(2).map(String::as_str) {
            Some("remove") => println!("{}", if devarch::sync::remove_launch_agent()? { "sync agent removed" } else { "no sync agent installed" }),
            Some("install") => {
                let every = args.get(3).and_then(|m| m.parse::<u32>().ok()).unwrap_or(30).max(devarch::sync::MIN_AGENT_MIN);
                let path = devarch::sync::install_launch_agent(every)?;
                println!("sync agent runs `{} sync` every {every} min\n  {}", std::env::current_exe()?.display(), path.display());
            }
            _ => bail!("usage: dev-arch sync-agent install [MIN] | remove"),
        },
        Some("daemon") => {
            daemon::start(p)?;
            println!("dev-arch daemon running; ctrl-c to stop");
            loop {
                std::thread::park();
            }
        }
        Some("set-key") => {
            let mut key = String::new();
            std::io::stdin().read_to_string(&mut key)?;
            journal::save_api_key(&key)?;
            println!("API key saved to the Keychain");
        }
        Some("hook") => println!("Add this line to ~/.zshrc:\n  {}", api::install_shell_hook()?),
        _ => bail!("usage: dev-arch discover|backfill|day|rebuild|journal|report|find-tickets|away|toggl|calendar|sync|sync-agent|weekly|status|daemon|set-key|hook"),
    }
    Ok(())
}
