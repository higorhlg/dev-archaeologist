#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use devarch::api::{self, DayView, ProjectRow, SessionEdit, Status};
use devarch::backfill::{BackfillReport, Discovery};
use devarch::event::WorkEvent;
use devarch::infer::journal;
use devarch::pipeline::Pipeline;
use devarch::reports::{ContextFilter, Report, ReportQuery};
use devarch::toggl::{SendResult, TogglAccount, TogglProjectInfo};
use devarch::settings::Settings;
use devarch::store::{default_db_path, Journal, Store};
use tauri::State;

type CmdResult<T> = Result<T, String>;

fn err(e: anyhow::Error) -> String {
    format!("{e:#}")
}

#[tauri::command]
fn status(p: State<Pipeline>) -> CmdResult<Status> {
    api::status(&p).map_err(err)
}

#[tauri::command]
fn discover(p: State<Pipeline>, days: i64) -> CmdResult<Discovery> {
    api::discover(&p, days).map_err(err)
}

/// Runs on a worker thread so the window stays responsive during a long scan.
#[tauri::command]
async fn backfill(p: State<'_, Pipeline>, days: i64) -> CmdResult<BackfillReport> {
    let p = p.inner().clone();
    tauri::async_runtime::spawn_blocking(move || api::run_backfill(&p, days)).await.map_err(|e| e.to_string())?.map_err(err)
}

#[tauri::command]
fn day(p: State<Pipeline>, day: String, side: Option<ContextFilter>) -> CmdResult<DayView> {
    api::day_view(&p, &day, side.unwrap_or(ContextFilter::All)).map_err(err)
}

#[tauri::command]
fn session_events(p: State<Pipeline>, id: String) -> CmdResult<Vec<WorkEvent>> {
    api::session_events(&p, &id).map_err(err)
}

#[tauri::command]
fn projects(p: State<Pipeline>, from: String, to: String, side: Option<ContextFilter>) -> CmdResult<Vec<ProjectRow>> {
    api::projects(&p, &from, &to, side.unwrap_or(ContextFilter::All)).map_err(err)
}

#[tauri::command]
fn confirm_session(p: State<Pipeline>, id: String) -> CmdResult<()> {
    api::confirm_session(&p, &id).map_err(err)
}

#[tauri::command]
fn exclude_session(p: State<Pipeline>, id: String) -> CmdResult<()> {
    api::exclude_session(&p, &id).map_err(err)
}

#[tauri::command]
fn edit_session(p: State<Pipeline>, id: String, edit: SessionEdit) -> CmdResult<()> {
    api::edit_session(&p, &id, edit).map_err(err)
}

#[tauri::command]
async fn generate_journal(p: State<'_, Pipeline>, day: String) -> CmdResult<Journal> {
    let p = p.inner().clone();
    tauri::async_runtime::spawn_blocking(move || api::generate_journal(&p, &day)).await.map_err(|e| e.to_string())?.map_err(err)
}

#[tauri::command]
fn confirm_journal(p: State<Pipeline>, day: String) -> CmdResult<()> {
    api::confirm_journal(&p, &day).map_err(err)
}

#[tauri::command]
fn rebuild_day(p: State<Pipeline>, day: String) -> CmdResult<usize> {
    api::rebuild_day(&p, &day).map_err(err)
}

#[tauri::command]
fn save_settings(p: State<Pipeline>, settings: Settings) -> CmdResult<()> {
    api::save_settings(&p, &settings).map_err(err)
}

#[tauri::command]
fn save_api_key(key: String) -> CmdResult<()> {
    journal::save_api_key(&key).map_err(err)
}

#[tauri::command]
fn install_shell_hook() -> CmdResult<String> {
    api::install_shell_hook().map_err(err)
}

#[tauri::command]
fn report(p: State<Pipeline>, query: ReportQuery) -> CmdResult<Report> {
    api::report(&p, &query).map_err(err)
}

#[tauri::command]
fn approve_sessions(p: State<Pipeline>, ids: Vec<String>) -> CmdResult<()> {
    api::approve_sessions(&p, &ids).map_err(err)
}

#[tauri::command]
fn approve_confident(p: State<Pipeline>, query: ReportQuery) -> CmdResult<usize> {
    api::approve_confident(&p, &query).map_err(err)
}

#[tauri::command]
fn set_entry_label(p: State<Pipeline>, session_id: String, label: String) -> CmdResult<()> {
    api::set_entry_label(&p, &session_id, &label).map_err(err)
}

#[tauri::command]
fn set_ticket(p: State<Pipeline>, session_ids: Vec<String>, ticket: Option<String>) -> CmdResult<()> {
    api::set_ticket(&p, &session_ids, ticket).map_err(err)
}

#[tauri::command]
fn set_github_ref(p: State<Pipeline>, session_ids: Vec<String>, github_ref: Option<String>) -> CmdResult<()> {
    api::set_github_ref(&p, &session_ids, github_ref).map_err(err)
}

#[tauri::command]
fn export_toggl_csv(p: State<Pipeline>, query: ReportQuery) -> CmdResult<String> {
    api::export_toggl_csv(&p, &query).map_err(err)
}

#[tauri::command]
async fn connect_toggl(p: State<'_, Pipeline>, token: String) -> CmdResult<TogglAccount> {
    let p = p.inner().clone();
    tauri::async_runtime::spawn_blocking(move || api::connect_toggl(&p, &token)).await.map_err(|e| e.to_string())?.map_err(err)
}

#[tauri::command]
async fn toggl_projects(p: State<'_, Pipeline>) -> CmdResult<Vec<TogglProjectInfo>> {
    let p = p.inner().clone();
    tauri::async_runtime::spawn_blocking(move || api::toggl_projects(&p)).await.map_err(|e| e.to_string())?.map_err(err)
}

#[tauri::command]
async fn send_to_toggl(p: State<'_, Pipeline>, query: ReportQuery) -> CmdResult<SendResult> {
    let p = p.inner().clone();
    tauri::async_runtime::spawn_blocking(move || api::send_to_toggl(&p, &query)).await.map_err(|e| e.to_string())?.map_err(err)
}

#[tauri::command]
fn days_needing_labels(p: State<Pipeline>, query: ReportQuery) -> CmdResult<Vec<String>> {
    api::days_needing_labels(&p, &query).map_err(err)
}

#[tauri::command]
async fn ai_models() -> CmdResult<Vec<devarch::infer::models::ModelInfo>> {
    tauri::async_runtime::spawn_blocking(api::ai_models).await.map_err(|e| e.to_string())
}

#[tauri::command]
fn report_days(p: State<Pipeline>, query: ReportQuery) -> CmdResult<Vec<api::DaySchedule>> {
    api::report_days(&p, &query).map_err(err)
}

#[tauri::command]
fn resolve_gap(p: State<Pipeline>, id: String, resolution: api::GapResolution) -> CmdResult<()> {
    api::resolve_gap(&p, &id, resolution).map_err(err)
}

#[tauri::command]
fn approve_bridges(p: State<Pipeline>, from: String, to: String) -> CmdResult<usize> {
    api::approve_bridges(&p, &from, &to).map_err(err)
}

#[tauri::command]
fn fill_to_schedule(p: State<Pipeline>, day: String, project: String) -> CmdResult<i64> {
    api::fill_to_schedule(&p, &day, &project).map_err(err)
}

#[tauri::command]
async fn save_schedule(p: State<'_, Pipeline>, schedule: Option<devarch::settings::WorkSchedule>) -> CmdResult<()> {
    let p = p.inner().clone();
    tauri::async_runtime::spawn_blocking(move || api::save_schedule(&p, schedule)).await.map_err(|e| e.to_string())?.map_err(err)
}

#[tauri::command]
fn project_contexts(p: State<Pipeline>) -> CmdResult<Vec<api::ProjectContext>> {
    api::project_contexts(&p).map_err(err)
}

#[tauri::command]
fn set_project_context(p: State<Pipeline>, project: String, context: Option<devarch::contexts::Context>) -> CmdResult<()> {
    api::set_project_context(&p, &project, context).map_err(err)
}

#[tauri::command]
fn patch_settings(p: State<Pipeline>, patch: serde_json::Value) -> CmdResult<Settings> {
    api::patch_settings(&p, patch).map_err(err)
}

#[tauri::command]
fn detect_contexts() -> devarch::contexts::ContextRules {
    api::detect_contexts()
}

#[tauri::command]
async fn calendar_status(p: State<'_, Pipeline>) -> CmdResult<api::CalendarStatus> {
    let p = p.inner().clone();
    tauri::async_runtime::spawn_blocking(move || api::calendar_status(&p)).await.map_err(|e| e.to_string())?.map_err(err)
}

#[tauri::command]
async fn connect_calendar(p: State<'_, Pipeline>) -> CmdResult<api::CalendarStatus> {
    let p = p.inner().clone();
    tauri::async_runtime::spawn_blocking(move || api::connect_calendar(&p)).await.map_err(|e| e.to_string())?.map_err(err)
}

#[tauri::command]
async fn reimport_meetings(p: State<'_, Pipeline>) -> CmdResult<usize> {
    let p = p.inner().clone();
    tauri::async_runtime::spawn_blocking(move || api::reimport_meetings(&p)).await.map_err(|e| e.to_string())?.map_err(err)
}

#[tauri::command]
fn dismiss_suggestion(p: State<Pipeline>, session_ids: Vec<String>) -> CmdResult<()> {
    api::dismiss_suggestion(&p, &session_ids).map_err(err)
}

#[tauri::command]
async fn tracker_status(p: State<'_, Pipeline>) -> CmdResult<devarch::trackers::TrackerStatus> {
    let p = p.inner().clone();
    tauri::async_runtime::spawn_blocking(move || api::tracker_status(&p)).await.map_err(|e| e.to_string())?.map_err(err)
}

#[tauri::command]
async fn connect_jira(p: State<'_, Pipeline>, site: String, email: String, token: String) -> CmdResult<devarch::trackers::jira::JiraAccount> {
    let p = p.inner().clone();
    tauri::async_runtime::spawn_blocking(move || api::connect_jira(&p, &site, &email, &token)).await.map_err(|e| e.to_string())?.map_err(err)
}

#[tauri::command]
fn open_jira_token_page() -> CmdResult<()> {
    api::open_jira_token_page().map_err(err)
}

#[tauri::command]
fn open_ticket(p: State<Pipeline>, key: String) -> CmdResult<()> {
    api::open_ticket(&p, &key).map_err(err)
}

#[tauri::command]
async fn find_tickets(p: State<'_, Pipeline>, query: ReportQuery) -> CmdResult<devarch::trackers::find::FindResult> {
    let p = p.inner().clone();
    tauri::async_runtime::spawn_blocking(move || api::find_tickets(&p, &query)).await.map_err(|e| e.to_string())?.map_err(err)
}

fn main() {
    let store = Store::open(&default_db_path()).expect("open the local database");
    let pipeline = Pipeline::new(store);
    if let Err(e) = pipeline.forget_stash_commits() {
        eprintln!("could not remove stored stash commits: {e:#}");
    }
    devarch::daemon::start(pipeline.clone()).expect("start the collectors");
    tauri::Builder::default()
        .manage(pipeline)
        .invoke_handler(tauri::generate_handler![
            status, discover, backfill, day, session_events, projects, confirm_session, exclude_session,
            edit_session, generate_journal, confirm_journal, rebuild_day, save_settings, save_api_key,
            install_shell_hook, report, approve_sessions, approve_confident, set_entry_label, set_ticket, set_github_ref,
            export_toggl_csv, connect_toggl, toggl_projects, send_to_toggl, days_needing_labels, ai_models, report_days, resolve_gap,
            approve_bridges, fill_to_schedule, save_schedule, project_contexts, set_project_context, patch_settings, detect_contexts,
            calendar_status, connect_calendar, reimport_meetings, dismiss_suggestion, tracker_status, connect_jira,
            open_jira_token_page, open_ticket, find_tickets
        ])
        .run(tauri::generate_context!())
        .expect("run the Dev Archaeologist window");
}
