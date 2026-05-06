//! Persistent user config (`~/.config/syswatch/config.toml`).
//!
//! Mirrors netwatch's pattern: a single struct with `Default`, plus
//! `path` / `load` / `save` accessors. `load()` is fail-soft — any
//! filesystem or parse error returns defaults so a missing or
//! hand-corrupted config file never blocks startup.

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct SyswatchConfig {
    /// Active theme name (must match a built-in from `crate::ui::theme::THEME_NAMES`).
    pub theme: String,
    /// Active graph style — `"bars"` or `"dots"`.
    pub graph_style: String,
    /// Tab the app opens on. Lowercase tab name, e.g. `"overview"` / `"cpu"`.
    pub default_tab: String,
    /// Sample interval in milliseconds. Clamped to `[100, 5000]` on load.
    pub tick_ms: u64,
}

impl Default for SyswatchConfig {
    fn default() -> Self {
        Self {
            theme: "dark".into(),
            graph_style: "bars".into(),
            default_tab: "overview".into(),
            tick_ms: 1000,
        }
    }
}

impl SyswatchConfig {
    /// `~/.config/syswatch/config.toml` on every platform `dirs` knows.
    pub fn path() -> Option<PathBuf> {
        dirs::config_dir().map(|d| d.join("syswatch").join("config.toml"))
    }

    /// Read from disk, returning defaults on any error or missing file.
    pub fn load() -> Self {
        let Some(path) = Self::path() else {
            return Self::default();
        };
        let Ok(contents) = fs::read_to_string(&path) else {
            return Self::default();
        };
        let mut cfg: Self = toml::from_str(&contents).unwrap_or_default();
        cfg.validate();
        cfg
    }

    /// Clamp / repair fields that may arrive out of range from a hand-edited
    /// config. Called automatically by `load()`; also exercised by tests.
    pub fn validate(&mut self) {
        self.tick_ms = self.tick_ms.clamp(100, 5000);
        if self.theme.is_empty() {
            self.theme = "dark".into();
        }
        if self.graph_style.is_empty() {
            self.graph_style = "bars".into();
        }
        if self.default_tab.is_empty() {
            self.default_tab = "overview".into();
        }
    }

    /// Write to disk, creating parent dirs as needed.
    pub fn save(&self) -> anyhow::Result<()> {
        let path =
            Self::path().ok_or_else(|| anyhow::anyhow!("cannot determine config directory"))?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let contents = toml::to_string_pretty(self)?;
        fs::write(path, contents)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_stable() {
        let c = SyswatchConfig::default();
        assert_eq!(c.theme, "dark");
        assert_eq!(c.graph_style, "bars");
        assert_eq!(c.default_tab, "overview");
        assert_eq!(c.tick_ms, 1000);
    }

    #[test]
    fn validate_clamps_tick_ms() {
        let mut c = SyswatchConfig {
            tick_ms: 50,
            ..Default::default()
        };
        c.validate();
        assert_eq!(c.tick_ms, 100);

        c.tick_ms = 10_000;
        c.validate();
        assert_eq!(c.tick_ms, 5000);
    }

    #[test]
    fn validate_fills_empty_strings() {
        let mut c = SyswatchConfig {
            theme: "".into(),
            graph_style: "".into(),
            default_tab: "".into(),
            tick_ms: 1000,
        };
        c.validate();
        assert_eq!(c.theme, "dark");
        assert_eq!(c.graph_style, "bars");
        assert_eq!(c.default_tab, "overview");
    }

    #[test]
    fn validate_preserves_valid_values() {
        let mut c = SyswatchConfig {
            theme: "dracula".into(),
            graph_style: "dots".into(),
            default_tab: "cpu".into(),
            tick_ms: 500,
        };
        let before = c.clone();
        c.validate();
        // Validate is idempotent for in-range values.
        assert_eq!(c.theme, before.theme);
        assert_eq!(c.graph_style, before.graph_style);
        assert_eq!(c.default_tab, before.default_tab);
        assert_eq!(c.tick_ms, before.tick_ms);
    }

    #[test]
    fn round_trip_through_toml() {
        let original = SyswatchConfig {
            theme: "nord".into(),
            graph_style: "dots".into(),
            default_tab: "memory".into(),
            tick_ms: 750,
        };
        let s = toml::to_string_pretty(&original).unwrap();
        let parsed: SyswatchConfig = toml::from_str(&s).unwrap();
        assert_eq!(parsed.theme, original.theme);
        assert_eq!(parsed.graph_style, original.graph_style);
        assert_eq!(parsed.default_tab, original.default_tab);
        assert_eq!(parsed.tick_ms, original.tick_ms);
    }

    #[test]
    fn missing_fields_use_defaults_via_serde() {
        // `#[serde(default)]` on the struct means an empty file deserializes
        // to the same shape as Default::default().
        let parsed: SyswatchConfig = toml::from_str("").unwrap();
        let default = SyswatchConfig::default();
        assert_eq!(parsed.theme, default.theme);
        assert_eq!(parsed.graph_style, default.graph_style);
        assert_eq!(parsed.tick_ms, default.tick_ms);
    }
}
