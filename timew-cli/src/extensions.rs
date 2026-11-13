//! Extension reports: executables in the `extensions` directory. A report
//! gets the configuration and the selected intervals on stdin and writes
//! the report to stdout.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

#[derive(Debug, Clone, Default)]
pub struct Extensions {
    /// Full paths, sorted.
    pub scripts: Vec<PathBuf>,
}

impl Extensions {
    pub fn load(dir: &Path) -> Result<Extensions, String> {
        let entries = fs::read_dir(dir).map_err(|_| format!("Extension directory not readable: {}", dir.display()))?;
        let mut scripts: Vec<PathBuf> = entries.filter_map(|e| e.ok()).map(|e| e.path()).collect();
        scripts.sort();
        Ok(Extensions { scripts })
    }

    /// File names, used as command names.
    pub fn names(&self) -> Vec<String> {
        self.scripts.iter().filter_map(|p| p.file_name()?.to_str().map(str::to_string)).collect()
    }

    pub fn find(&self, name: &str) -> Option<&PathBuf> {
        self.scripts.iter().find(|p| p.file_name().and_then(|n| n.to_str()) == Some(name))
    }

    /// Run `script` with `input` on stdin. Returns its exit status and
    /// output lines.
    pub fn call(script: &Path, input: &str) -> Result<(i32, Vec<String>), String> {
        let mut child = Command::new(script)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .map_err(|e| format!("Could not run '{}': {e}", script.display()))?;
        if let Some(mut stdin) = child.stdin.take() {
            // A script that doesn't read its input is fine.
            let _ = stdin.write_all(input.as_bytes());
        }
        let output = child.wait_with_output().map_err(|e| format!("Could not run '{}': {e}", script.display()))?;
        let text = String::from_utf8_lossy(&output.stdout);
        let lines = text.split('\n').map(str::to_string).collect::<Vec<_>>();
        let lines = match lines.as_slice() {
            [rest @ .., last] if last.is_empty() => rest.to_vec(),
            _ => lines,
        };
        Ok((output.status.code().unwrap_or(-1), lines))
    }
}

/// `-rwx`-style status of a file, for `timew extensions`.
pub fn status(path: &Path) -> String {
    let readable = fs::File::open(path).is_ok();
    let executable = is_executable(path);
    let mut status = if !readable {
        "Not readable".to_string()
    } else if !executable {
        "No executable".to_string()
    } else {
        "Active".to_string()
    };
    if fs::symlink_metadata(path).map(|m| m.file_type().is_symlink()).unwrap_or(false) {
        status.push_str(" (link)");
    }
    status
}

pub fn is_executable(path: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::metadata(path).map(|m| m.is_file() && m.permissions().mode() & 0o111 != 0).unwrap_or(false)
    }
    #[cfg(not(unix))]
    {
        path.is_file()
    }
}
