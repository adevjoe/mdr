//! Syntax highlighting for code blocks (syntect).

use std::sync::LazyLock;

use anstyle::{Ansi256Color, Color, RgbColor, Style};
use syntect::easy::HighlightLines;
use syntect::highlighting::{FontStyle, Theme, ThemeSet};
use syntect::parsing::{SyntaxReference, SyntaxSet};
use syntect::util::LinesWithEndings;

static SYNTAXES: LazyLock<SyntaxSet> = LazyLock::new(SyntaxSet::load_defaults_newlines);
static THEMES: LazyLock<(Theme, Theme)> = LazyLock::new(|| {
    let mut set = ThemeSet::load_defaults();
    let dark = set.themes.remove("base16-ocean.dark").unwrap_or_default();
    let light = set.themes.remove("InspiredGitHub").unwrap_or_default();
    (dark, light)
});

/// Colors built into the dark highlighting theme.
pub struct ThemeColors {
    pub bg: (u8, u8, u8),
    pub fg: (u8, u8, u8),
    pub comment: (u8, u8, u8),
}

/// Background, foreground and comment colors of the dark theme (base16-ocean.dark).
pub fn dark_colors() -> ThemeColors {
    let s = &THEMES.0.settings;
    let c =
        |c: Option<syntect::highlighting::Color>, d: (u8, u8, u8)| c.map_or(d, |c| (c.r, c.g, c.b));
    ThemeColors {
        bg: c(s.background, (0x2b, 0x30, 0x3b)),
        fg: c(s.foreground, (0xc0, 0xc5, 0xce)),
        comment: (0x65, 0x73, 0x7e),
    }
}

fn find_syntax(lang: &str) -> Option<&'static SyntaxReference> {
    let ss = &*SYNTAXES;
    let lower = lang.to_ascii_lowercase();
    // syntect's bundled syntaxes lack these names; map them to similar syntaxes.
    let alias = match lower.as_str() {
        "ts" | "typescript" | "tsx" | "jsx" | "mjs" | "cjs" => "js",
        "shell" | "zsh" | "console" | "shellsession" => "bash",
        "jsonc" | "json5" => "json",
        "toml" | "ini" | "conf" => "ini",
        "golang" => "go",
        "py3" => "py",
        "c++" => "cpp",
        other => other,
    };
    ss.find_syntax_by_token(alias)
        .or_else(|| ss.find_syntax_by_name(lang))
        .or_else(|| {
            (alias == "ini")
                .then(|| ss.find_syntax_by_token("yaml"))
                .flatten()
        })
}

/// Whether the language is recognized.
pub fn supports(lang: &str) -> bool {
    find_syntax(lang).is_some()
}

/// Highlights code, returning (style, text) segments for each line; returns `None` for unknown
/// languages.
///
/// Text matching the theme's default foreground is left uncolored so it stays readable on light
/// terminals.
/// When `light` is true, the light theme (InspiredGitHub) is used.
pub fn highlight(
    code: &str,
    lang: &str,
    truecolor: bool,
    light: bool,
) -> Option<Vec<Vec<(Style, String)>>> {
    let syntax = find_syntax(lang)?;
    let theme = if light { &THEMES.1 } else { &THEMES.0 };
    let default_fg = theme.settings.foreground;
    let mut h = HighlightLines::new(syntax, theme);
    let mut out = Vec::new();
    for line in LinesWithEndings::from(code) {
        let ranges = h.highlight_line(line, &SYNTAXES).ok()?;
        let spans = ranges
            .into_iter()
            .filter_map(|(st, text)| {
                let text = text.trim_end_matches(['\n', '\r']);
                if text.is_empty() {
                    return None;
                }
                let mut style = Style::new();
                if Some(st.foreground) != default_fg {
                    let c = st.foreground;
                    style = style.fg_color(Some(rgb(c.r, c.g, c.b, truecolor)));
                }
                if st.font_style.contains(FontStyle::BOLD) {
                    style = style.bold();
                }
                if st.font_style.contains(FontStyle::ITALIC) {
                    style = style.italic();
                }
                if st.font_style.contains(FontStyle::UNDERLINE) {
                    style = style.underline();
                }
                Some((style, text.to_string()))
            })
            .collect();
        out.push(spans);
    }
    Some(out)
}

/// A 24-bit color; falls back to the nearest 256-color value when the terminal lacks truecolor.
pub fn rgb(r: u8, g: u8, b: u8, truecolor: bool) -> Color {
    if truecolor {
        Color::Rgb(RgbColor(r, g, b))
    } else {
        Color::Ansi256(Ansi256Color(rgb_to_ansi256(r, g, b)))
    }
}

/// Maps a 24-bit color to the xterm 256-color palette.
pub fn rgb_to_ansi256(r: u8, g: u8, b: u8) -> u8 {
    if r == g && g == b {
        return match r {
            0..8 => 16,
            249.. => 231,
            _ => 232 + ((u16::from(r) - 8) * 24 / 247) as u8,
        };
    }
    let q = |v: u8| -> u8 {
        match v {
            0..48 => 0,
            48..115 => 1,
            _ => (v - 35) / 40,
        }
    };
    16 + 36 * q(r) + 6 * q(g) + q(b)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ansi256_mapping() {
        assert_eq!(rgb_to_ansi256(0, 0, 0), 16);
        assert_eq!(rgb_to_ansi256(255, 255, 255), 231);
        assert_eq!(rgb_to_ansi256(255, 0, 0), 196);
        assert_eq!(rgb_to_ansi256(0, 255, 0), 46);
    }

    #[test]
    fn known_languages() {
        for lang in [
            "rust", "python", "js", "ts", "bash", "json", "go", "toml", "yaml",
        ] {
            assert!(supports(lang), "{lang}");
        }
        assert!(!supports("no-such-language"));
        let lines = highlight("fn main() {}\nlet x = 1;\n", "rust", true, false).unwrap();
        assert!(highlight("fn main() {}\n", "rust", true, true).is_some());
        assert_eq!(lines.len(), 2);
        let joined: String = lines[0].iter().map(|(_, t)| t.as_str()).collect();
        assert_eq!(joined, "fn main() {}");
    }
}
