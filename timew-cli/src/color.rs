//! Terminal colors as Timewarrior specifies them in its configuration:
//! `[bold] [underline] [inverse] [<color>] [on [bright] <color>]`, where a
//! color is a basic name (`red`), `grayN`/`greyN` (0-23), `colorN` (0-255),
//! `rgbRGB` (each 0-5) or `0xRRGGBB`.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Shade {
    /// One of the 8 basic colors, 0 = black .. 7 = white.
    Basic(u8),
    /// An entry of the 256-color palette.
    Indexed(u8),
    /// A 24-bit color.
    Rgb(u32),
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Color {
    fg: Option<Shade>,
    bg: Option<Shade>,
    bold: bool,
    underline: bool,
    inverse: bool,
    /// Bright background (16-color mode only).
    bright: bool,
}

const NAMES: [&str; 8] = ["black", "red", "green", "yellow", "blue", "magenta", "cyan", "white"];

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Mode {
    Basic,
    Indexed,
    Rgb,
}

impl Color {
    pub fn none() -> Self {
        Color::default()
    }

    /// Parse a color specification. An empty string is "no color".
    pub fn parse(spec: &str) -> Result<Color, String> {
        let mut color = Color::default();
        let mut background = false;
        for word in spec.split(' ') {
            let word = word.trim().to_ascii_lowercase();
            let unrecognized = || format!("The color '{word}' is not recognized.");
            let shade = match word.as_str() {
                "" => continue,
                "bold" => {
                    color.bold = true;
                    continue;
                }
                "bright" => {
                    color.bright = true;
                    continue;
                }
                "underline" => {
                    color.underline = true;
                    continue;
                }
                "inverse" => {
                    color.inverse = true;
                    continue;
                }
                "on" => {
                    background = true;
                    continue;
                }
                "none" => continue,
                w => {
                    if let Some(i) = NAMES.iter().position(|n| *n == w) {
                        Shade::Basic(i as u8)
                    } else if let Some(n) = w.strip_prefix("gray").or_else(|| w.strip_prefix("grey")) {
                        let n = parse_number(n).filter(|n| *n <= 23).ok_or_else(unrecognized)?;
                        Shade::Indexed(232 + n as u8)
                    } else if let Some(rgb) = w.strip_prefix("rgb") {
                        let digits: Vec<u32> = rgb.chars().filter_map(|c| c.to_digit(10)).collect();
                        if rgb.len() != 3 || digits.len() != 3 || digits.iter().any(|d| *d > 5) {
                            return Err(unrecognized());
                        }
                        Shade::Indexed((16 + digits[0] * 36 + digits[1] * 6 + digits[2]) as u8)
                    } else if let Some(n) = w.strip_prefix("color") {
                        let n = parse_number(n).filter(|n| *n <= 255).ok_or_else(unrecognized)?;
                        Shade::Indexed(n as u8)
                    } else if let Some(hex) = w.strip_prefix("0x") {
                        let value = u32::from_str_radix(hex, 16).map_err(|_| unrecognized())?;
                        if value > 0xFF_FFFF {
                            return Err(unrecognized());
                        }
                        Shade::Rgb(value)
                    } else {
                        return Err(unrecognized());
                    }
                }
            };
            if background {
                color.bg = Some(shade);
            } else {
                color.fg = Some(shade);
            }
        }
        Ok(color)
    }

    /// Parse, treating an invalid specification as "no color".
    pub fn parse_or_none(spec: &str) -> Color {
        Color::parse(spec).unwrap_or_default()
    }

    pub fn underline() -> Color {
        Color { underline: true, ..Color::default() }
    }

    pub fn is_none(&self) -> bool {
        *self == Color::default()
    }

    /// Lay `other` over this color: its colors win, its attributes add up.
    pub fn blend(&mut self, other: &Color) {
        self.bold |= other.bold;
        self.underline |= other.underline;
        self.inverse |= other.inverse;
        self.bright |= other.bright;
        if other.fg.is_some() {
            self.fg = other.fg;
        }
        if other.bg.is_some() {
            self.bg = other.bg;
        }
    }

    fn mode(&self) -> Mode {
        let mode_of = |s: &Option<Shade>| match s {
            None | Some(Shade::Basic(_)) => Mode::Basic,
            Some(Shade::Indexed(_)) => Mode::Indexed,
            Some(Shade::Rgb(_)) => Mode::Rgb,
        };
        mode_of(&self.fg).max(mode_of(&self.bg))
    }

    /// The SGR parameter for the foreground or background, in `mode`.
    fn shade_code(&self, shade: Shade, background: bool, mode: Mode) -> String {
        let (base, ext) = if background { (40, 48) } else { (30, 38) };
        match (mode, shade) {
            (Mode::Basic, Shade::Basic(i)) => {
                let base = if background && self.bright { 100 } else { base };
                format!("{}", base + i as u32)
            }
            (Mode::Indexed, s) => format!("{ext};5;{}", self.as_index(s, background)),
            (Mode::Rgb, s) => {
                let rgb = match s {
                    Shade::Rgb(v) => v,
                    other => index_to_rgb(self.as_index(other, background)),
                };
                format!("{ext};2;{};{};{}", (rgb >> 16) & 0xFF, (rgb >> 8) & 0xFF, rgb & 0xFF)
            }
            (Mode::Basic, _) => unreachable!("mode covers the shade"),
        }
    }

    fn as_index(&self, shade: Shade, background: bool) -> u32 {
        match shade {
            Shade::Basic(i) if background && self.bright => i as u32 + 8,
            Shade::Basic(i) => i as u32,
            Shade::Indexed(i) => i as u32,
            Shade::Rgb(_) => 0,
        }
    }

    /// The opening escape sequence, or "" for no color.
    pub fn code(&self) -> String {
        if self.is_none() {
            return String::new();
        }
        let mode = self.mode();
        let mut parts = Vec::new();
        if self.bold {
            parts.push("1".to_string());
        }
        if self.underline {
            parts.push("4".to_string());
        }
        if self.inverse {
            parts.push("7".to_string());
        }
        if let Some(fg) = self.fg {
            parts.push(self.shade_code(fg, false, mode));
        }
        if let Some(bg) = self.bg {
            parts.push(self.shade_code(bg, true, mode));
        }
        format!("\x1b[{}m", parts.join(";"))
    }

    pub fn end(&self) -> &'static str {
        if self.is_none() { "" } else { "\x1b[0m" }
    }

    pub fn colorize(&self, text: &str) -> String {
        if self.is_none() {
            return text.to_string();
        }
        format!("{}{text}\x1b[0m", self.code())
    }
}

fn parse_number(s: &str) -> Option<u32> {
    if s.is_empty() || !s.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    s.parse().ok()
}

/// Approximate RGB value of a 256-palette entry.
fn index_to_rgb(index: u32) -> u32 {
    match index {
        0..=7 => (index & 1) * 0x80_0000 + ((index >> 1) & 1) * 0x00_8000 + ((index >> 2) & 1) * 0x00_0080,
        8..=15 => (index & 1) * 0xFF_0000 + ((index >> 1) & 1) * 0x00_FF00 + ((index >> 2) & 1) * 0x00_00FF,
        16..=231 => {
            let i = index - 16;
            (i / 36) * 0x2a_0000 + ((i % 36) / 6) * 0x00_2a00 + (i % 6) * 0x00_002a
        }
        _ => (index - 231) * 0x0a_0a0a,
    }
}

/// A rotating list of colors used to tell tags apart in charts.
pub struct Palette {
    colors: Vec<Color>,
    next: usize,
    pub enabled: bool,
}

impl Palette {
    pub fn new(enabled: bool) -> Self {
        let colors = [
            "white on red",
            "white on blue",
            "black on green",
            "black on magenta",
            "black on cyan",
            "black on yellow",
            "black on white",
            "white on bright red",
            "white on bright blue",
            "black on bright green",
            "black on bright magenta",
            "black on bright cyan",
            "black on bright yellow",
        ]
        .iter()
        .map(|s| Color::parse_or_none(s))
        .collect();
        Palette { colors, next: 0, enabled }
    }

    pub fn with_colors(colors: Vec<Color>, enabled: bool) -> Self {
        Palette { colors, next: 0, enabled }
    }

    pub fn len(&self) -> usize {
        self.colors.len()
    }

    pub fn next_color(&mut self) -> Color {
        if !self.enabled || self.colors.is_empty() {
            return Color::none();
        }
        let c = self.colors[self.next % self.colors.len()];
        self.next += 1;
        c
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basic_codes() {
        assert_eq!(Color::parse("red").unwrap().colorize("x"), "\x1b[31mx\x1b[0m");
        assert_eq!(Color::parse("white on red").unwrap().code(), "\x1b[37;41m");
        assert_eq!(Color::parse("white on bright red").unwrap().code(), "\x1b[37;101m");
        assert_eq!(Color::parse("underline").unwrap().code(), "\x1b[4m");
        assert_eq!(Color::parse("").unwrap().colorize("x"), "x");
    }

    #[test]
    fn indexed_codes() {
        assert_eq!(Color::parse("gray8 on gray4").unwrap().code(), "\x1b[38;5;240;48;5;236m");
        assert_eq!(Color::parse("color12").unwrap().code(), "\x1b[38;5;12m");
        assert_eq!(Color::parse("red on gray4").unwrap().code(), "\x1b[38;5;1;48;5;236m");
        assert!(Color::parse("gray30").is_err());
        assert!(Color::parse("purple").is_err());
    }

    #[test]
    fn blending() {
        let mut c = Color::parse("white on red").unwrap();
        c.blend(&Color::parse("underline").unwrap());
        assert_eq!(c.code(), "\x1b[4;37;41m");
        c.blend(&Color::parse("black").unwrap());
        assert_eq!(c.code(), "\x1b[4;30;41m");
    }
}
