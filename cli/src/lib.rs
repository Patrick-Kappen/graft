#![deny(clippy::all)]
#![deny(clippy::pedantic)]
#![allow(clippy::module_name_repetitions)]

//! Graft CLI — TOML → Quadlet config file generator.

pub mod cli;
pub mod commands;
pub mod config;
