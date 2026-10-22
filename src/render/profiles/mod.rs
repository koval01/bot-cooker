//! One module per family of artifacts. Each turns the scope into a `Doc`
//! by choosing templates and building their context; no text lives here.

pub mod api;
pub mod assets;
pub mod config;
pub mod errors;
pub mod feeds;
pub mod logs;
pub mod pages;
pub mod secrets;
pub mod site;
pub mod sql;
pub mod vcs;
