#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use devarch::api::{self, DayView, ProjectRow, SessionEdit, Status};
use devarch::backfill::{BackfillReport, Discovery};
use devarch::event::WorkEvent;
use devarch::infer::journal;
use devarch::pipeline::Pipeline;
use devarch::reports::{Report, ReportQuery};
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
fn day(p: State<Pipeline>, day: String) -> CmdResult<DayView> {
    api::day_view(&p, &day).map_err(err)
}

#[tauri::command]
fn session_events(p: State<Pipeline>, id: String) -> CmdResult<Vec<WorkEvent>> {
    api::session_events(&p, &id).map_err(err)
}

#[tauri::command]
fn projects(p: State<Pipeline>, from: String, to: String) -> CmdResult<Vec<ProjectRow>> {
    api::projects(&p, &from, &to).map_err(err)
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

fn main() {
    let store = Store::open(&default_db_path()).expect("open the local database");
    let pipeline = Pipeline::new(store);
    devarch::daemon::start(pipeline.clone()).expect("start the collectors");
    tauri::Builder::default()
        .manage(pipeline)
        .invoke_handler(tauri::generate_handler![
            status, discover, backfill, day, session_events, projects, confirm_session, exclude_session,
            edit_session, generate_journal, confirm_journal, rebuild_day, save_settings, save_api_key,
            install_shell_hook, report, approve_sessions, approve_confident, set_entry_label, set_ticket,
            export_toggl_csv, connect_toggl, toggl_projects, send_to_toggl, days_needing_labels
        ])
        .run(tauri::generate_context!())
        .expect("run the Dev Archaeologist window");
}
