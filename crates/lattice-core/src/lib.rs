//! Core domain model and pure logic for lattice.
//!
//! This crate has **zero I/O**. Every symbol is a pure data type, a
//! validation rule, or a rendering primitive. All persistence, process
//! supervision, and UI live in sibling crates.

#![deny(unsafe_code)]

pub mod derived;
pub mod entities;
pub mod error;
pub mod fields;
pub mod ids;
pub mod prompt;
pub mod time;
pub mod validation;

pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

#[cfg(test)]
mod smoke {
    use super::*;

    #[test]
    fn version_is_non_empty() {
        assert!(!version().is_empty());
    }
}
