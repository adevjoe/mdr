//! Terminal styles for each element. Text colors use only the 16 ANSI colors,
//! so they follow the user's terminal color scheme.
//!
//! On modern terminals such as Ghostty ([`Rich`]), extra effects like background
//! colors and styled underlines are used; those choices live in [`Look`].

use anstyle::{AnsiColor, Color, Effects, Style};

use crate::highlight;
use crate::parse::{AlertKind, InlineStyle};
use crate::term;

/// Enhanced display capabilities of modern terminals (Ghostty, kitty, WezTerm).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Rich {
    /// Supports styled underlines such as curly and dotted (SGR 4:x).
    pub styled_underline: bool,
    /// Terminal background color (from an OSC 11 query), used to derive a code
    /// background that blends with it.
    pub background: Option<(u8, u8, u8)>,
}

/// Colors of the code block panel.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Panel {
    pub bg: Color,
    /// Default text color inside the panel; `None` keeps the terminal foreground.
    pub fg: Option<Color>,
    /// Style of the language label.
    pub label: Style,
}

/// Overall look used for one layout pass.
#[derive(Clone, Debug, PartialEq)]
pub struct Look {
    /// Links use a dotted underline.
    pub styled_underline: bool,
    /// Background color of inline code.
    pub code_chip: Option<Color>,
    /// Draw code blocks as a filled panel; `None` uses border lines.
    pub panel: Option<Panel>,
    /// Use a light theme for syntax highlighting.
    pub light: bool,
    /// Vertical bar on the left of block quotes.
    pub quote_bar: &'static str,
    /// Tables use rounded corners.
    pub rounded: bool,
}

impl Look {
    /// Basic look: works on every terminal.
    pub fn basic() -> Self {
        Look {
            styled_underline: false,
            code_chip: None,
            panel: None,
            light: false,
            quote_bar: "│",
            rounded: false,
        }
    }

    /// Enhanced look for modern terminals. When the terminal background is known, the
    /// code background is slightly lighter (dark background) or darker (light background)
    /// than it, and text keeps the terminal foreground; otherwise the highlight theme's
    /// own dark panel is used.
    pub fn rich(rich: &Rich, truecolor: bool) -> Self {
        let (panel, light, chip) = match rich.background {
            Some(bg) => {
                let light = term::is_light(bg);
                let (r, g, b) = shade(bg, light);
                let bg = highlight::rgb(r, g, b, truecolor);
                let panel = Panel {
                    bg,
                    fg: None,
                    label: muted().italic(),
                };
                (panel, light, Some(bg))
            }
            None => {
                let c = highlight::dark_colors();
                let color = |(r, g, b)| highlight::rgb(r, g, b, truecolor);
                let panel = Panel {
                    bg: color(c.bg),
                    fg: Some(color(c.fg)),
                    label: Style::new().fg_color(Some(color(c.comment))).italic(),
                };
                (panel, false, None)
            }
        };
        Look {
            styled_underline: rich.styled_underline,
            code_chip: chip,
            panel: Some(panel),
            light,
            quote_bar: "▎",
            rounded: true,
        }
    }
}

/// Code background: slightly lighter on dark backgrounds, slightly darker on light ones.
pub fn shade((r, g, b): (u8, u8, u8), light: bool) -> (u8, u8, u8) {
    let f = |c: u8| -> u8 {
        let c = f64::from(c);
        let v = if light {
            c * 0.955
        } else {
            c + (255.0 - c) * 0.08
        };
        v.round() as u8
    };
    (f(r), f(g), f(b))
}

const fn fg(c: AnsiColor) -> Style {
    Style::new().fg_color(Some(Color::Ansi(c)))
}

pub fn heading(level: u8) -> Style {
    match level {
        1 => fg(AnsiColor::BrightMagenta).bold(),
        2 => fg(AnsiColor::BrightCyan).bold(),
        3 => fg(AnsiColor::BrightYellow).bold(),
        // Avoid green: that's the inline code color.
        4 => fg(AnsiColor::BrightBlue).bold(),
        5 => fg(AnsiColor::Cyan).bold(),
        _ => fg(AnsiColor::Magenta).bold(),
    }
}

/// Inline code: standard green (not bright, not bold), easy on the eyes in both dark
/// and light terminals.
pub const fn code_inline() -> Style {
    fg(AnsiColor::Green)
}

/// Layer inline styles on top of the block's base style.
///
/// Inline code does not inherit the block style (heading color and bold, table header
/// bold); it keeps only its own bold/italic/strikethrough, so it stands out from the
/// surrounding text even inside headings.
pub fn inline(base: Style, s: &InlineStyle, look: &Look) -> Style {
    let mut st = if s.code {
        code_inline().bg_color(look.code_chip)
    } else {
        base
    };
    if s.bold {
        st = st.bold();
    }
    if s.italic {
        st = st.italic();
    }
    if s.strike {
        st = st.strikethrough();
    }
    if s.image {
        st = st.fg_color(Some(Color::Ansi(AnsiColor::Magenta)));
    } else if s.link.is_some() {
        st = st.fg_color(Some(Color::Ansi(AnsiColor::Blue)));
        st = if look.styled_underline {
            st.effects(st.get_effects() | Effects::DOTTED_UNDERLINE)
        } else {
            st.underline()
        };
    }
    st
}

/// URL shown after link text when hyperlinks are not supported.
pub const fn link_url() -> Style {
    fg(AnsiColor::BrightBlack)
}

pub const fn muted() -> Style {
    fg(AnsiColor::BrightBlack)
}

pub const fn code_lang() -> Style {
    fg(AnsiColor::BrightBlack).italic()
}

pub const fn list_marker() -> Style {
    fg(AnsiColor::Cyan)
}

pub const fn task_done() -> Style {
    fg(AnsiColor::Green)
}

pub const fn task_todo() -> Style {
    fg(AnsiColor::BrightBlack)
}

pub const fn table_header() -> Style {
    Style::new().bold()
}

pub fn quote_bar(kind: Option<AlertKind>) -> Style {
    match kind {
        None => fg(AnsiColor::BrightBlack),
        Some(AlertKind::Note) => fg(AnsiColor::Blue),
        Some(AlertKind::Tip) => fg(AnsiColor::Green),
        Some(AlertKind::Important) => fg(AnsiColor::Magenta),
        Some(AlertKind::Warning) => fg(AnsiColor::Yellow),
        Some(AlertKind::Caution) => fg(AnsiColor::Red),
    }
}

pub fn alert_label(kind: AlertKind) -> &'static str {
    match kind {
        AlertKind::Note => "Note",
        AlertKind::Tip => "Tip",
        AlertKind::Important => "Important",
        AlertKind::Warning => "Warning",
        AlertKind::Caution => "Caution",
    }
}
