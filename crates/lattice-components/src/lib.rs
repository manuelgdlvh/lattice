//! Interactive components registry (C4 and beyond) — v0.2+ feature set.
//! v0.1 only reserves the API surface.

#![deny(unsafe_code)]

pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_is_non_empty() {
        assert!(!version().is_empty());
    }
}
