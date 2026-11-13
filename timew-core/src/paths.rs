//! Where the configuration and the database live.
//!
//! 1. `$TIMEWARRIORDB`, for both, if set.
//! 2. `~/.timewarrior`, for both, if that directory exists (the old layout).
//! 3. Otherwise `$XDG_CONFIG_HOME/timewarrior` (default `~/.config/…`) for
//!    the configuration and extensions, and `$XDG_DATA_HOME/timewarrior`
//!    (default `~/.local/share/…`) for the data.

use std::env;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct Paths {
    pub config_dir: PathBuf,
    pub db_dir: PathBuf,
}

fn home() -> PathBuf {
    env::var_os("HOME").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("."))
}

impl Paths {
    pub fn from_env() -> Paths {
        if let Some(db) = env::var_os("TIMEWARRIORDB") {
            let db = PathBuf::from(db);
            return Paths { config_dir: db.clone(), db_dir: db };
        }
        let legacy = home().join(".timewarrior");
        if legacy.is_dir() {
            return Paths { config_dir: legacy.clone(), db_dir: legacy };
        }
        let xdg = |var: &str, default: &str| {
            env::var_os(var).filter(|v| !v.is_empty()).map(PathBuf::from).unwrap_or_else(|| home().join(default))
        };
        Paths {
            config_dir: xdg("XDG_CONFIG_HOME", ".config").join("timewarrior"),
            db_dir: xdg("XDG_DATA_HOME", ".local/share").join("timewarrior"),
        }
    }

    pub fn config_file(&self) -> PathBuf {
        self.config_dir.join("timewarrior.cfg")
    }

    pub fn data_dir(&self) -> PathBuf {
        self.db_dir.join("data")
    }

    pub fn extensions_dir(&self) -> PathBuf {
        self.config_dir.join("extensions")
    }

    /// Create whatever is missing: the directories (after asking, if
    /// `ask` says so), the data and extensions directories, and a new
    /// configuration file. Returns whether the database is new.
    pub fn initialize(&self, ask: &mut dyn FnMut(&str) -> bool) -> Result<bool, String> {
        let usable = |dir: &Path| fs::read_dir(dir).is_ok();
        let config_exists = self.config_dir.is_dir();
        if config_exists && !usable(&self.config_dir) {
            return Err(format!("Config is not readable at '{}'", self.config_dir.display()));
        }
        let db_exists = self.db_dir.is_dir();
        if db_exists && !usable(&self.db_dir) {
            return Err(format!("Database is not readable at '{}'", self.db_dir.display()));
        }

        let mut questions = Vec::new();
        if !config_exists {
            questions.push(format!("Create new config in {}?", self.config_dir.display()));
        }
        if !db_exists && self.config_dir != self.db_dir {
            questions.push(format!("Create new database in {}?", self.db_dir.display()));
        }
        if !questions.is_empty() {
            if !ask(&questions.join("\n")) {
                return Err("Initial setup aborted by user".to_string());
            }
            for dir in [&self.config_dir, &self.db_dir] {
                create_private_dir(dir)?;
            }
        }
        create_private_dir(&self.extensions_dir())?;
        create_private_dir(&self.data_dir())?;

        let cfg = self.config_file();
        if !cfg.exists() {
            fs::write(&cfg, crate::rules::NEW_FILE).map_err(|e| format!("Could not create {}: {e}", cfg.display()))?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let _ = fs::set_permissions(&cfg, fs::Permissions::from_mode(0o600));
            }
        }
        Ok(!db_exists)
    }
}

fn create_private_dir(dir: &Path) -> Result<(), String> {
    if dir.is_dir() {
        return Ok(());
    }
    fs::create_dir_all(dir).map_err(|e| format!("Could not create {}: {e}", dir.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(dir, fs::Permissions::from_mode(0o700));
    }
    Ok(())
}
