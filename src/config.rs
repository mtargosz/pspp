//! Persistent TOML configuration loading and atomic saving (`pspp.conf`).

use std::fs;
use std::io;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

pub const CONFIG_FILE_NAME: &str = "pspp.conf";
pub const MIN_UPDATE_MS: u64 = 100;
pub const MAX_UPDATE_MS: u64 = 86_400_000;
pub const DEFAULT_THEME_NAME: &str = "Default";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Config {
    pub update_ms: u64,
    pub mem_kind: String,
    pub agg_mode: String,
    pub graph_symbol: String,
    pub base10: bool,
    pub save_on_exit: bool,
    pub sort_key: String,
    pub sort_reverse: bool,
    /// btop-style theme name; `Default` is built in, others load from `<config dir>/themes`.
    pub color_theme: String,
    pub theme_background: bool,
    /// `auto` (detect from `COLORTERM`), `on` or `off`.
    pub truecolor: String,
    pub rounded_corners: bool,
    pub vim_keys: bool,
    pub disable_mouse: bool,
    /// strftime string shown in the top border; empty disables the clock.
    pub clock_format: String,
    pub background_update: bool,
    pub force_tty: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            update_ms: 2_000,
            mem_kind: "rss".to_owned(),
            agg_mode: "rollup".to_owned(),
            graph_symbol: "auto".to_owned(),
            base10: false,
            save_on_exit: true,
            sort_key: "agg".to_owned(),
            sort_reverse: true,
            color_theme: DEFAULT_THEME_NAME.to_owned(),
            theme_background: true,
            truecolor: "auto".to_owned(),
            rounded_corners: true,
            vim_keys: true,
            disable_mouse: false,
            clock_format: String::new(),
            background_update: true,
            force_tty: false,
        }
    }
}

impl Config {
    pub fn load() -> Self {
        let Some(path) = config_path() else {
            return Self::default();
        };

        let content = match fs::read_to_string(path) {
            Ok(content) => content,
            Err(err) if err.kind() == io::ErrorKind::NotFound => return Self::default(),
            Err(_) => return Self::default(),
        };

        let value = match toml::from_str::<toml::Value>(&content) {
            Ok(value) => value,
            Err(_) => return Self::default(),
        };

        let Some(table) = value.as_table() else {
            return Self::default();
        };

        let mut config = Self::default();
        if let Some(update_ms) = table
            .get("update_ms")
            .and_then(toml_integer_to_u64)
            .map(|value| value.clamp(MIN_UPDATE_MS, MAX_UPDATE_MS))
        {
            config.update_ms = update_ms;
        }
        if let Some(mem_kind) = table.get("mem_kind").and_then(toml_string_lower) {
            if is_valid_mem_kind(&mem_kind) {
                config.mem_kind = mem_kind;
            }
        }
        if let Some(agg_mode) = table.get("agg_mode").and_then(toml_string_lower) {
            if matches!(agg_mode.as_str(), "rollup" | "flat") {
                config.agg_mode = agg_mode;
            }
        }
        if let Some(graph_symbol) = table.get("graph_symbol").and_then(toml_string_lower) {
            if matches!(graph_symbol.as_str(), "auto" | "braille" | "block" | "tty") {
                config.graph_symbol = graph_symbol;
            }
        }
        if let Some(base10) = table.get("base10").and_then(|value| value.as_bool()) {
            config.base10 = base10;
        }
        if let Some(save_on_exit) = table.get("save_on_exit").and_then(|value| value.as_bool()) {
            config.save_on_exit = save_on_exit;
        }
        if let Some(sort_key) = table.get("sort_key").and_then(toml_string_lower) {
            if matches!(sort_key.as_str(), "agg" | "own" | "pid" | "comm") {
                config.sort_key = sort_key;
            }
        }
        if let Some(sort_reverse) = table.get("sort_reverse").and_then(|value| value.as_bool()) {
            config.sort_reverse = sort_reverse;
        }
        if let Some(name) = table
            .get("color_theme")
            .and_then(|value| value.as_str())
            .map(str::trim)
            .filter(|name| is_valid_theme_name(name))
        {
            config.color_theme = name.to_owned();
        }
        let bools: [(&str, &mut bool); 6] = [
            ("theme_background", &mut config.theme_background),
            ("rounded_corners", &mut config.rounded_corners),
            ("vim_keys", &mut config.vim_keys),
            ("disable_mouse", &mut config.disable_mouse),
            ("background_update", &mut config.background_update),
            ("force_tty", &mut config.force_tty),
        ];
        for (key, slot) in bools {
            if let Some(value) = table.get(key).and_then(|value| value.as_bool()) {
                *slot = value;
            }
        }
        if let Some(truecolor) = table.get("truecolor").and_then(toml_string_lower) {
            if matches!(truecolor.as_str(), "auto" | "on" | "off") {
                config.truecolor = truecolor;
            }
        }
        if let Some(clock) = table.get("clock_format").and_then(|value| value.as_str()) {
            config.clock_format = clock.to_owned();
        }

        config
    }

    pub fn save(&self) -> io::Result<()> {
        let dir = config_dir().ok_or_else(|| io::Error::other("no config directory"))?;
        fs::create_dir_all(&dir)?;
        let path = dir.join(CONFIG_FILE_NAME);

        let body = toml::to_string_pretty(self).map_err(|err| io::Error::other(err.to_string()))?;
        let content = format!(
            "# pspp configuration\n# Generated by pspp; edit while pspp is not running.\n\n{body}"
        );
        // Write-then-rename so a crash or concurrent load never sees a truncated file.
        let tmp = dir.join(format!("{CONFIG_FILE_NAME}.tmp"));
        fs::write(&tmp, content)?;
        fs::rename(&tmp, &path).inspect_err(|_| {
            let _ = fs::remove_file(&tmp);
        })
    }
}

/// Directory scanned for btop `.theme` files.
pub fn themes_dir() -> Option<PathBuf> {
    config_dir().map(|dir| dir.join("themes"))
}

/// Theme names become file names, so reject anything that could escape the themes directory.
pub fn is_valid_theme_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && name
            .chars()
            .all(|ch| ch.is_alphanumeric() || matches!(ch, '-' | '_' | '.' | ' '))
        && !name.starts_with('.')
}

pub fn config_path() -> Option<PathBuf> {
    config_dir().map(|dir| dir.join(CONFIG_FILE_NAME))
}

fn config_dir() -> Option<PathBuf> {
    match std::env::var("PSPP_CONFIG_DIR") {
        Ok(path) if !path.trim().is_empty() => Some(PathBuf::from(path)),
        _ => dirs::config_dir().map(|dir| dir.join("pspp")),
    }
}

fn toml_integer_to_u64(value: &toml::Value) -> Option<u64> {
    let value = value.as_integer()?;
    u64::try_from(value).ok()
}

fn toml_string_lower(value: &toml::Value) -> Option<String> {
    Some(value.as_str()?.trim().to_ascii_lowercase())
}

fn is_valid_mem_kind(value: &str) -> bool {
    matches!(value, "rss" | "vsz" | "footprint" | "pss" | "uss" | "swap")
}

#[cfg(test)]
mod tests {
    use super::Config;
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::sync::Mutex;
    use std::time::{SystemTime, UNIX_EPOCH};

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn roundtrip_save_and_load() {
        with_test_config_dir("roundtrip", |dir| {
            let expected = Config {
                update_ms: 3_500,
                mem_kind: "vsz".to_owned(),
                agg_mode: "flat".to_owned(),
                graph_symbol: "block".to_owned(),
                base10: true,
                save_on_exit: false,
                sort_key: "pid".to_owned(),
                sort_reverse: false,
                color_theme: "my-theme".to_owned(),
                theme_background: false,
                truecolor: "off".to_owned(),
                rounded_corners: false,
                vim_keys: false,
                disable_mouse: true,
                clock_format: "%H:%M:%S".to_owned(),
                background_update: false,
                force_tty: true,
            };

            expected.save().expect("save config");
            let loaded = Config::load();
            assert_eq!(loaded, expected);

            let saved_path = dir.join("pspp.conf");
            let saved = fs::read_to_string(saved_path).expect("read saved file");
            assert!(saved.starts_with("# pspp configuration"));
        });
    }

    #[test]
    fn update_ms_is_clamped_and_save_leaves_no_temp_file() {
        with_test_config_dir("clamp", |dir| {
            fs::write(dir.join("pspp.conf"), "update_ms = 9223372036854775807\n")
                .expect("write config");
            assert_eq!(Config::load().update_ms, super::MAX_UPDATE_MS);

            fs::write(dir.join("pspp.conf"), "update_ms = 5\n").expect("write config");
            assert_eq!(Config::load().update_ms, super::MIN_UPDATE_MS);

            Config::default().save().expect("save config");
            assert!(!dir.join("pspp.conf.tmp").exists());
            assert_eq!(Config::load(), Config::default());
        });
    }

    #[test]
    fn new_options_default_to_btop_like_values() {
        let defaults = Config::default();
        assert_eq!(defaults.color_theme, "Default");
        assert!(defaults.theme_background && defaults.rounded_corners && defaults.vim_keys);
        assert!(defaults.background_update);
        assert!(!defaults.disable_mouse && !defaults.force_tty);
        assert_eq!(defaults.truecolor, "auto");
        assert!(defaults.clock_format.is_empty());
    }

    #[test]
    fn invalid_new_option_values_fall_back_per_key() {
        with_test_config_dir("invalid-new", |dir| {
            fs::write(
                dir.join("pspp.conf"),
                "truecolor = \"maybe\"\nvim_keys = \"yes\"\ncolor_theme = \"../../etc/passwd\"\n\
                 rounded_corners = false\nclock_format = 5\nforce_tty = true\n",
            )
            .expect("write config");
            let loaded = Config::load();
            assert_eq!(loaded.truecolor, "auto");
            assert!(loaded.vim_keys, "non-bool keeps the default");
            assert_eq!(
                loaded.color_theme, "Default",
                "path-like names are rejected"
            );
            assert!(!loaded.rounded_corners, "valid keys still apply");
            assert!(loaded.force_tty);
            assert!(loaded.clock_format.is_empty());
        });
    }

    #[test]
    fn theme_name_validation_blocks_path_traversal() {
        assert!(super::is_valid_theme_name("Tokyo Night"));
        assert!(super::is_valid_theme_name("solarized-dark_2"));
        for bad in ["", "..", "../x", "a/b", ".hidden", "a\\b"] {
            assert!(!super::is_valid_theme_name(bad), "{bad}");
        }
    }

    #[test]
    fn missing_file_returns_defaults() {
        with_test_config_dir("missing", |_| {
            let loaded = Config::load();
            assert_eq!(loaded, Config::default());
        });
    }

    #[test]
    fn garbage_file_falls_back_without_panicking() {
        with_test_config_dir("garbage", |dir| {
            fs::write(dir.join("pspp.conf"), "this is not valid toml = [")
                .expect("write garbage config");
            let loaded = Config::load();
            assert_eq!(loaded, Config::default());
        });
    }

    fn with_test_config_dir<F>(name: &str, test: F)
    where
        F: FnOnce(&Path),
    {
        let _guard = ENV_LOCK.lock().expect("env lock");
        let dir = unique_test_dir(name);
        fs::create_dir_all(&dir).expect("create test config dir");

        let previous = std::env::var_os("PSPP_CONFIG_DIR");
        std::env::set_var("PSPP_CONFIG_DIR", &dir);

        test(&dir);

        if let Some(previous) = previous {
            std::env::set_var("PSPP_CONFIG_DIR", previous);
        } else {
            std::env::remove_var("PSPP_CONFIG_DIR");
        }

        let _ = fs::remove_dir_all(&dir);
    }

    fn unique_test_dir(name: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock drift")
            .as_nanos();
        let pid = std::process::id();
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join("test-config")
            .join(format!("{name}-{pid}-{nanos}"))
    }
}
