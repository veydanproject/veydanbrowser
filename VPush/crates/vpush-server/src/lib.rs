//! VPush server library.
//!
//! The binary crate only parses the command line; everything it runs is here.

pub mod admin;
pub mod api;
pub mod config;
pub mod delivery;
pub mod logging;
pub mod serve;
pub mod version;

pub use config::Config;
pub use serve::serve;
