//! `timew diagnostics`: version, configuration and file details, for bug
//! reports.

use std::fs;
use std::path::Path;

use crate::color::Palette;
use crate::session::Session;

fn describe(path: &Path) -> String {
    match fs::symlink_metadata(path) {
        Err(e) => format!("{} ({e})", path.display()),
        Ok(meta) => {
            let kind = if meta.file_type().is_symlink() {
                'l'
            } else if meta.is_dir() {
                'd'
            } else {
                '-'
            };
            let readable = if fs::File::open(path).is_ok() { 'r' } else { '-' };
            let writable = if !meta.permissions().readonly() { 'w' } else { '-' };
            #[cfg(unix)]
            let executable = {
                use std::os::unix::fs::PermissionsExt;
                if meta.permissions().mode() & 0o111 != 0 { 'x' } else { '-' }
            };
            #[cfg(not(unix))]
            let executable = '-';
            format!("{} ({kind}{readable}{writable}{executable} {} bytes)", path.display(), meta.len())
        }
    }
}

pub fn run(s: &mut Session) -> Result<i32, String> {
    let mut out = format!("\ntimew-rs {}\n       Platform: {} {}\n\n", crate::VERSION, std::env::consts::OS, std::env::consts::ARCH);
    out.push_str("Configuration\n");
    out.push_str(&format!(
        "  TIMEWARRIORDB: {}\n",
        std::env::var("TIMEWARRIORDB").unwrap_or_else(|_| "-".to_string())
    ));
    out.push_str(&format!("            Cfg: {}\n", describe(&s.paths.config_file())));
    out.push_str(&format!("       Database: {}\n", describe(&s.paths.db_dir)));
    for name in s.db.file_names() {
        out.push_str(&format!("                 {}\n", describe(&s.paths.data_dir().join(name))));
    }
    if let Ok(editor) = std::env::var("VISUAL") {
        out.push_str(&format!("        $VISUAL: {editor}\n"));
    } else if let Ok(editor) = std::env::var("EDITOR") {
        out.push_str(&format!("        $EDITOR: {editor}\n"));
    }
    out.push_str(&format!(
        "    Color theme: {}\n",
        s.rules.get("theme.description").unwrap_or("Default theme")
    ));
    if s.color() {
        let mut palette = Palette::new(true);
        out.push_str("                ");
        for i in 0..palette.len() {
            if i > 0 && i % 16 == 0 {
                out.push_str("\n                ");
            }
            out.push(' ');
            out.push_str(&palette.next_color().colorize(&format!("{i:02}")));
        }
        out.push('\n');
    }
    out.push('\n');
    out.push_str(&format!("Extensions\n       Location: {}\n", describe(&s.paths.extensions_dir())));
    if s.extensions.scripts.is_empty() {
        out.push_str("               (None)\n");
    } else {
        for script in &s.extensions.scripts {
            out.push_str(&format!("                 {}\n", describe(script)));
        }
    }
    out.push('\n');
    s.out.push_str(&out);
    Ok(0)
}
