//! open-upstream-watch core: watch the upstream projects RPoem re-implements, prepare the Rust-side work in an
//! isolated clone, and merge it automatically only through a strict gate.

pub mod analysis;
pub mod config;
pub mod gate;
pub mod github;
pub mod gitops;
pub mod notify;
pub mod pipeline;
pub mod porter;
pub mod services;
pub mod state;
pub mod version;

pub use config::Config;
pub use state::State;
