#![deny(clippy::all)]
#![deny(clippy::pedantic)]
#![allow(clippy::module_name_repetitions)]

//! Graft CLI — manage Podman Quadlet containers from TOML.

pub mod cli;
pub mod commands;
pub mod config;
pub mod container;
pub mod workspace;
