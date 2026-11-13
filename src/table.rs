//! Column layout for the text reports.
//!
//! Every column is as wide as its widest cell (or header). If that doesn't
//! fit in the terminal, columns shrink down to their longest word and cell
//! text wraps. Without color, headers are underlined with dashes; with
//! color, they are drawn underlined instead.

use crate::color::Color;
use crate::text::{display_width, left_justify, longest_line, longest_word, right_justify, strip_ansi, wrap_text};

pub struct Table {
    headers: Vec<String>,
    align_left: Vec<bool>,
    rows: Vec<Vec<(String, Color)>>,
    width: usize,
    color: bool,
}

impl Table {
    /// A table for a terminal `width` columns wide.
    pub fn new(width: usize, color: bool) -> Self {
        Table { headers: Vec::new(), align_left: Vec::new(), rows: Vec::new(), width, color }
    }

    pub fn add_column(&mut self, header: &str, align_left: bool) {
        self.headers.push(header.to_string());
        self.align_left.push(align_left);
    }

    pub fn add_row(&mut self) -> usize {
        self.rows.push(vec![(String::new(), Color::none()); self.headers.len()]);
        self.rows.len() - 1
    }

    pub fn set(&mut self, row: usize, col: usize, value: impl Into<String>) {
        self.rows[row][col].0 = value.into();
    }

    pub fn set_colored(&mut self, row: usize, col: usize, value: impl Into<String>, color: Color) {
        self.rows[row][col] = (value.into(), color);
    }

    fn widths(&self) -> Vec<usize> {
        let n = self.headers.len();
        let mut minimal: Vec<usize> = self.headers.iter().map(|h| display_width(h)).collect();
        let mut ideal = minimal.clone();
        for row in &self.rows {
            for (c, (value, _)) in row.iter().enumerate() {
                let plain = strip_ansi(value);
                minimal[c] = minimal[c].max(longest_word(&plain));
                ideal[c] = ideal[c].max(longest_line(&plain));
            }
        }
        let available = self.width as i64 - (n.saturating_sub(1)) as i64;
        let sum_min: usize = minimal.iter().sum();
        let sum_ideal: usize = ideal.iter().sum();
        if sum_ideal as i64 <= available {
            return ideal;
        }
        if sum_min as i64 > available || available < 0 {
            return minimal;
        }
        // Hand out the remaining space one column at a time.
        let mut widths = minimal;
        let mut spare = available as usize - sum_min;
        while spare > 0 {
            for i in 0..n {
                if spare > 0 && widths[i] < ideal[i] {
                    widths[i] += 1;
                    spare -= 1;
                }
            }
        }
        widths
    }

    fn cell_lines(value: &str, width: usize, align_left: bool, color: &Color) -> Vec<String> {
        wrap_text(value, width)
            .into_iter()
            .map(|line| {
                let justified = if align_left { left_justify(&line, width) } else { right_justify(&line, width) };
                color.colorize(&justified)
            })
            .collect()
    }

    pub fn render(&self) -> String {
        let widths = self.widths();
        let header_color = if self.color { Color::underline() } else { Color::none() };
        let mut out = String::new();

        let headers: Vec<Vec<String>> = self
            .headers
            .iter()
            .enumerate()
            .map(|(c, h)| Self::cell_lines(h, widths[c], self.align_left[c], &header_color))
            .collect();
        let header_lines = headers.iter().map(Vec::len).max().unwrap_or(0);
        for i in 0..header_lines {
            let mut line = String::new();
            for (c, lines) in headers.iter().enumerate() {
                if c > 0 {
                    line.push(' ');
                }
                let skip = header_lines - lines.len();
                if i < skip {
                    line.push_str(&header_color.colorize(&" ".repeat(widths[c])));
                } else {
                    line.push_str(&lines[i - skip]);
                }
            }
            out.push_str(line.trim_end_matches(' '));
            out.push('\n');
        }
        if !self.color {
            let dashes: Vec<String> = widths.iter().map(|w| "-".repeat(*w)).collect();
            out.push_str(&dashes.join(" "));
            out.push('\n');
        }

        for row in &self.rows {
            let cells: Vec<Vec<String>> = row
                .iter()
                .enumerate()
                .map(|(c, (value, color))| {
                    let color = if self.color { *color } else { Color::none() };
                    Self::cell_lines(value, widths[c], self.align_left[c], &color)
                })
                .collect();
            let lines = cells.iter().map(Vec::len).max().unwrap_or(0);
            for i in 0..lines {
                let mut line = String::new();
                for (c, cell) in cells.iter().enumerate() {
                    if c > 0 {
                        line.push(' ');
                    }
                    match cell.get(i) {
                        Some(text) => line.push_str(text),
                        None => {
                            let color = if self.color { row[c].1 } else { Color::none() };
                            line.push_str(&color.colorize(&" ".repeat(widths[c])));
                        }
                    }
                }
                out.push_str(line.trim_end_matches(' '));
                out.push('\n');
            }
        }
        out
    }
}

/// Width of the terminal on stdin, else `$COLUMNS`, else 80.
pub fn terminal_width() -> usize {
    #[cfg(unix)]
    {
        #[repr(C)]
        struct WinSize {
            rows: u16,
            cols: u16,
            x: u16,
            y: u16,
        }
        unsafe extern "C" {
            fn ioctl(fd: i32, request: u64, ...) -> i32;
        }
        #[cfg(target_os = "linux")]
        const TIOCGWINSZ: u64 = 0x5413;
        #[cfg(not(target_os = "linux"))]
        const TIOCGWINSZ: u64 = 0x40087468;
        let mut ws = WinSize { rows: 0, cols: 0, x: 0, y: 0 };
        // SAFETY: TIOCGWINSZ writes a `struct winsize` to the pointer.
        let ok = unsafe { ioctl(0, TIOCGWINSZ, &mut ws as *mut WinSize) } == 0;
        if ok && ws.cols > 0 {
            return ws.cols as usize;
        }
    }
    std::env::var("COLUMNS").ok().and_then(|c| c.parse().ok()).filter(|c| *c > 0).unwrap_or(80)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_table() {
        let mut t = Table::new(80, false);
        t.add_column("Tag", true);
        t.add_column("Count", false);
        let r = t.add_row();
        t.set(r, 0, "alpha");
        t.set(r, 1, "3");
        let r = t.add_row();
        t.set(r, 0, "b");
        assert_eq!(t.render(), "Tag   Count\n----- -----\nalpha     3\nb\n");
    }

    #[test]
    fn wraps_when_narrow() {
        let mut t = Table::new(12, false);
        t.add_column("A", true);
        t.add_column("B", true);
        let r = t.add_row();
        t.set(r, 0, "one two three");
        t.set(r, 1, "x");
        assert_eq!(t.render(), "A          B\n---------- -\none two    x\nthree\n");
    }
}
