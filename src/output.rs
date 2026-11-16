//! 把排好的行转成终端字节流：ANSI 样式、OSC 8 超链接和 kitty 图形协议都在这里生成。

use std::fmt::Write as _;

use crate::graphics;
use crate::render::Line;

/// 同步输出（DEC 模式 2026）：终端收到结束标记后再一次性刷新，避免大段输出闪烁。
pub const SYNC_BEGIN: &str = "\x1b[?2026h";
pub const SYNC_END: &str = "\x1b[?2026l";

/// 去掉控制字符，避免文档内容里的转义序列被终端执行。
fn sanitize(s: &str) -> impl Iterator<Item = char> + '_ {
    s.chars().filter(|c| !c.is_control())
}

/// 转成字符串。`color` 为 false 时输出纯文本（样式和超链接都被丢弃）。
pub fn to_ansi(lines: &[Line], color: bool) -> String {
    let mut out = String::new();
    for line in lines {
        // 图片会向下占好几行。先用换行把这些行腾出来（在屏幕底部时会滚屏），再把光标移回来，
        // 这样图片不会画到屏幕外面。
        let rows = line
            .spans
            .iter()
            .filter_map(|s| s.graphic.as_ref())
            .map(|g| g.rows)
            .max()
            .unwrap_or(0);
        if color && rows > 1 {
            for _ in 1..rows {
                out.push('\n');
            }
            let _ = write!(out, "\x1b[{}A", rows - 1);
        }
        for span in &line.spans {
            if !color {
                out.extend(sanitize(&span.text));
                continue;
            }
            if let Some(g) = &span.graphic {
                out.push_str(&graphics::kitty_sequence(g));
            }
            if let Some(url) = &span.link {
                out.push_str("\x1b]8;;");
                out.extend(sanitize(url));
                out.push_str("\x1b\\");
            }
            let styled = span.style != anstyle::Style::new();
            if styled {
                let _ = write!(out, "{}", span.style.render());
            }
            out.extend(sanitize(&span.text));
            if styled {
                let _ = write!(out, "{}", span.style.render_reset());
            }
            if span.link.is_some() {
                out.push_str("\x1b]8;;\x1b\\");
            }
        }
        out.push('\n');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::{RenderOptions, render};

    #[test]
    fn plain_output_has_no_escapes() {
        let lines = render(
            "# 标题\n\n**粗体** [链接](https://a.b)",
            &RenderOptions::plain(40),
        );
        let s = to_ansi(&lines, false);
        assert!(!s.contains('\x1b'));
        assert!(s.contains("粗体 链接 <https://a.b>"));
    }

    #[test]
    fn colored_output_has_styles_and_hyperlinks() {
        let opts = RenderOptions {
            color: true,
            hyperlinks: true,
            ..RenderOptions::plain(40)
        };
        let s = to_ansi(&render("**粗体** [链接](https://a.b)", &opts), true);
        assert!(s.contains("\x1b[1m粗体\x1b[0m"));
        assert!(s.contains("\x1b]8;;https://a.b\x1b\\"));
        assert!(s.contains("\x1b]8;;\x1b\\"));
        assert!(!s.contains("<https://a.b>"));
    }

    #[test]
    fn control_characters_are_stripped() {
        let lines = render("危险\x1b[31m文本", &RenderOptions::plain(40));
        assert_eq!(to_ansi(&lines, true), "危险[31m文本\n");
    }
}
