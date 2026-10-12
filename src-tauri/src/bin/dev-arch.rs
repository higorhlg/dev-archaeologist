//! `dev-arch`: headless access to the same core the app uses.
//!
//!   dev-arch discover [days]     what a first scan would read
//!   dev-arch backfill [days]     read past logs into the local store
//!   dev-arch day [YYYY-MM-DD]    print one day's sessions and achievements
//!   dev-arch journal YYYY-MM-DD  write the day's journal with the Claude API
//!   dev-arch report FROM TO [session|ticket_day|project_day] [work|personal|all]
//!   dev-arch schedule [HH:MM HH:MM | off]  set Mon–Fri hours with lunch 12–13
//!   dev-arch gaps [YYYY-MM-DD]   print the day's gaps inside work hours
//!   dev-arch fill DAY PROJECT     assign a day's unaccounted blocks to one project
//!   dev-arch approve-bridges DAY approve a day's bridged gaps
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
    let v = api::day_view(p, day)?;
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

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let db = std::env::var("DEV_ARCH_DB").map(Into::into).unwrap_or_else(|_| default_db_path());
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
        Some("report") => {
            let today = Local::now().format("%Y-%m-%d").to_string();
            let q = devarch::reports::ReportQuery {
                from: args.get(2).cloned().unwrap_or_else(|| today.clone()),
                to: args.get(3).cloned().unwrap_or(today),
                grouping: serde_json::from_value(serde_json::json!(args.get(4).map(String::as_str).unwrap_or("ticket_day")))?,
                tickets: devarch::reports::TicketMode::Prefix,
                context: serde_json::from_value(serde_json::json!(args.get(5).map(String::as_str).unwrap_or("work")))?,
            };
            let r = api::report(&p, &q)?;
            println!("{} min total · {} approved · {} to review · prefixes {:?}", r.total_min, r.approved_min, r.review_count, r.known_prefixes);
            for e in &r.entries {
                let t = e.ticket.as_ref().map(|t| format!("{} ({})", t.key, t.source)).unwrap_or_else(|| "-".into());
                println!("  {} {:>4}m  {:<22} {:<28} {}", e.day, e.duration_min, e.toggl_project, t, devarch::event::clip(&e.label, 70));
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
            let v = api::day_view(&p, &day)?;
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
        Some("status") => {
            let s = api::status(&p)?;
            println!("events: {:?}\ndays: {}\napi key: {}", s.event_counts, s.days.len(), if s.has_api_key { "set" } else { "not set" });
        }
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
        _ => bail!("usage: dev-arch discover|backfill|day|rebuild|journal|status|daemon|set-key|hook"),
    }
    Ok(())
}
