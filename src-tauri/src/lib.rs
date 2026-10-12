//! Dev Archaeologist core: local collectors, the event store, session rules, the daily
//! journal and achievements. The Tauri app and the `dev-arch` CLI both use this crate.

pub mod achievements;
pub mod adapters;
pub mod api;
pub mod backfill;
pub mod collectors;
pub mod corrections;
pub mod daemon;
pub mod event;
pub mod infer;
pub mod pipeline;
pub mod project;
pub mod redact;
pub mod reports;
pub mod sessions;
pub mod settings;
pub mod store;
pub mod tickets;
pub mod toggl;
