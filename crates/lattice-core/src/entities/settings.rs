//! `Settings` — application-wide user preferences.

use serde::{Deserialize, Serialize};

use super::CURRENT_SCHEMA_VERSION;

fn current_schema_version() -> u32 {
    CURRENT_SCHEMA_VERSION
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct CacheSettings {
    pub max_entries: usize,
    pub max_bytes: u64,
}

impl Default for CacheSettings {
    fn default() -> Self {
        Self {
            max_entries: 4096,
            max_bytes: 67_108_864,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct LoggingSettings {
    pub level: String,
}

impl Default for LoggingSettings {
    fn default() -> Self {
        Self {
            level: "info".into(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Settings {
    #[serde(default = "current_schema_version")]
    pub schema_version: u32,
    #[serde(default)]
    pub cache: CacheSettings,
    #[serde(default)]
    pub logging: LoggingSettings,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            schema_version: CURRENT_SCHEMA_VERSION,
            cache: CacheSettings::default(),
            logging: LoggingSettings::default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_roundtrip() {
        let s = Settings::default();
        let t = toml::to_string(&s).unwrap();
        let back: Settings = toml::from_str(&t).unwrap();
        assert_eq!(s, back);
    }

    #[test]
    fn empty_toml_yields_defaults() {
        let s: Settings = toml::from_str("schema_version = 1").unwrap();
        assert_eq!(s, Settings::default());
    }
}
