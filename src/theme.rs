//! 各元素的终端样式。文字颜色只用 16 色，跟随用户终端的配色方案。
//!
//! 在 Ghostty 等现代终端里（[`Rich`]）会额外使用背景色、样式化下划线等效果，
//! 具体取舍集中在 [`Look`] 里。

use anstyle::{AnsiColor, Color, Effects, Style};

use crate::highlight;
use crate::parse::{AlertKind, InlineStyle};
use crate::term;

/// 现代终端（Ghostty、kitty、WezTerm）的增强显示能力。
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Rich {
    /// 支持波浪线、点线等样式化下划线（SGR 4:x）。
    pub styled_underline: bool,
    /// 终端背景色（OSC 11 查询结果），用来调出和背景协调的代码底色。
    pub background: Option<(u8, u8, u8)>,
}

/// 代码块面板的配色。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Panel {
    pub bg: Color,
    /// 面板里的默认文字颜色；`None` 表示沿用终端前景色。
    pub fg: Option<Color>,
    /// 语言标签的样式。
    pub label: Style,
}

/// 一次排版使用的整体外观。
#[derive(Clone, Debug, PartialEq)]
pub struct Look {
    /// 链接使用点线下划线。
    pub styled_underline: bool,
    /// 行内代码的背景色。
    pub code_chip: Option<Color>,
    /// 代码块画成带底色的面板；`None` 时用边框线。
    pub panel: Option<Panel>,
    /// 代码高亮使用浅色主题。
    pub light: bool,
    /// 引用块左侧的竖条。
    pub quote_bar: &'static str,
    /// 表格使用圆角。
    pub rounded: bool,
}

impl Look {
    /// 基础外观：适用于所有终端。
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

    /// 现代终端的增强外观。知道终端背景色时，代码底色在背景色上略微提亮（深色背景）
    /// 或压暗（浅色背景），文字沿用终端前景色；不知道时用高亮主题自带的深色面板。
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

/// 代码底色：深色背景提亮一点，浅色背景压暗一点。
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
        // 避开绿色：那是行内代码的颜色。
        4 => fg(AnsiColor::BrightBlue).bold(),
        5 => fg(AnsiColor::Cyan).bold(),
        _ => fg(AnsiColor::Magenta).bold(),
    }
}

/// 行内代码：标准绿色（非亮色、不加粗），在深浅色终端里都不刺眼。
pub const fn code_inline() -> Style {
    fg(AnsiColor::Green)
}

/// 在块级基础样式上叠加行内样式。
///
/// 行内代码不继承块级样式（标题的颜色和加粗、表头的加粗），只保留自身的粗体/斜体/删除线，
/// 这样在标题里也能和周围的文字区分开。
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
