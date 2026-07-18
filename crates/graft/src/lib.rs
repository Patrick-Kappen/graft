#![deny(clippy::all)]
#![deny(clippy::pedantic)]
#![allow(clippy::module_name_repetitions)]

//! Graft CLI — TOML → resolved JSON generator.

pub mod cli;
pub mod config;
pub mod manifest;
pub mod protocol;
pub mod resolve;
pub mod worker;
