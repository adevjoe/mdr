//! Turn laid-out lines into terminal output: ANSI styles, OSC 8 hyperlinks and kitty graphics
//! escapes are all generated here.

use std::fmt::Write as _;

use crate::graphics;
use crate::render::Line;

/// Synchronized output (DEC mode 2026): the terminal repaints once on the end marker, avoiding
/// flicker on large output.
pub const SYNC_BEGIN: &str = "\x1b[?2026h";
pub const SYNC_END: &str = "\x1b[?2026l";

/// Strip control characters so escape sequences in the document aren't executed by the terminal.
fn sanitize(s: &str) -> impl Iterator<Item = char> + '_ {
    s.chars().filter(|c| !c.is_control())
}

/// Convert to a string. When `color` is false, output plain text (styles and hyperlinks are dropped).
pub fn to_ansi(lines: &[Line], color: bool) -> String {
    let mut out = String::new();
    for line in lines {
        // An image spans several rows. Emit newlines first to make room (scrolling if at the
        // bottom of the screen), then move the cursor back, so the image isn't drawn off-screen.
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
