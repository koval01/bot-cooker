//! bot-cooker as a library: everything the binary wires together, exposed so
//! integration tests can drive the real HTTP stack.

pub mod catalog;
pub mod config;
pub mod data;
pub mod engine;
pub mod http;
#[cfg(feature = "pack")]
pub mod pack;
pub mod persona;
pub mod render;
pub mod routing;
pub mod server;
#[cfg(windows)]
pub mod service;
pub mod stats;
pub mod tarpit;
pub mod text;
pub mod util;
pub mod wire;
pub mod world;
