//! Library interface for litho-generator.
//!
//! This file exposes the crate's modules as a library target so that
//! integration tests in `tests/` can import types without duplicating code.
//! Production entry-point remains `main.rs`.

pub mod benchmark;
pub mod cache;
pub mod cli;
pub mod config;
pub mod generator;
pub mod i18n;
pub mod integrations;
pub mod llm;
pub mod memory;
pub mod types;
pub mod utils;
