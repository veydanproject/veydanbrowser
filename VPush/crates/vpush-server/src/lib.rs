//! VPush server library.
//!
//! The binary crate only parses the command line; everything it runs is here.

pub mod admin;
pub mod auth;
pub mod api;
pub mod config;
pub mod delivery;
pub mod limit;
pub mod logging;
pub mod pipeline;
pub mod relay;
pub mod relays;
pub mod serve;
pub mod store;
pub mod version;

pub use config::Config;
pub use serve::serve;
