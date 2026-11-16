//! 各元素的终端样式。只用 16 色，跟随用户终端的配色方案。

use anstyle::{AnsiColor, Color, Style};

use crate::parse::{AlertKind, InlineStyle};

const fn fg(c: AnsiColor) -> Style {
    Style::new().fg_color(Some(Color::Ansi(c)))
}

pub fn heading(level: u8) -> Style {
    match level {
        1 => fg(AnsiColor::BrightMagenta).bold(),
        2 => fg(AnsiColor::BrightCyan).bold(),
        3 => fg(AnsiColor::BrightYellow).bold(),
        4 => fg(AnsiColor::BrightGreen).bold(),
        5 => fg(AnsiColor::BrightBlue).bold(),
        _ => fg(AnsiColor::Magenta).bold(),
    }
}

/// 在块级基础样式上叠加行内样式。
pub fn inline(base: Style, s: &InlineStyle) -> Style {
    let mut st = base;
    if s.bold {
        st = st.bold();
    }
    if s.italic {
        st = st.italic();
    }
    if s.strike {
        st = st.strikethrough();
    }
    if s.code {
        st = st.fg_color(Some(Color::Ansi(AnsiColor::Yellow)));
    }
    if s.image {
        st = st.fg_color(Some(Color::Ansi(AnsiColor::Magenta)));
    } else if s.link.is_some() {
        st = st.fg_color(Some(Color::Ansi(AnsiColor::Blue))).underline();
    }
    st
}

/// 不支持超链接时，跟在链接文字后面的网址。
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
