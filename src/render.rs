//! Layout: turns the block tree into lines of styled text ([`Line`]).
//!
//! No escape sequences are emitted here; this only decides what text each line
//! holds and how it is styled. Converting to terminal bytes happens in [`crate::output`].

use std::sync::Arc;

use anstyle::Style;

use crate::graphics::{self, Graphic, ImageOptions};
use crate::highlight;
use crate::parse::{self, Align, Block, InlineKind, Inlines, ListItem};
use crate::theme::{self, Look, Panel, Rich};
use crate::wrap::{self, Item, Piece, display_width};

/// Minimum layout width.
pub const MIN_WIDTH: usize = 10;

/// Marker placed before continuation lines of wrapped code.
const CONTINUATION_MARK: &str = "↪";
/// Columns taken by the marker plus the following space.
const CONTINUATION_WIDTH: usize = 2;

/// A run of styled text. `link` is set only when terminal hyperlinks are enabled.
///
/// `graphic` is an image to display here (only when image display is enabled). The image
/// occupies `rows` lines starting at this one; the following `rows - 1` lines are blank
/// lines reserved for it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Span {
    pub text: String,
    pub style: Style,
    pub link: Option<String>,
    pub graphic: Option<Arc<Graphic>>,
}

/// A laid-out line.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Line {
    pub spans: Vec<Span>,
}

impl Line {
    pub fn width(&self) -> usize {
        self.spans.iter().map(|s| display_width(&s.text)).sum()
    }

    /// Plain text with styles stripped.
    pub fn plain(&self) -> String {
        self.spans.iter().map(|s| s.text.as_str()).collect()
    }

    pub fn is_empty(&self) -> bool {
        self.spans
            .iter()
            .all(|s| s.text.is_empty() && s.graphic.is_none())
    }

    fn push(&mut self, text: impl Into<String>, style: Style) {
        self.spans.push(span(text, style));
    }

    fn styled(text: impl Into<String>, style: Style) -> Line {
        let mut l = Line::default();
        l.push(text, style);
        l
    }
}

/// Convert lines to plain text, each ending with `\n`. Used by tests and `--no-color`.
pub fn to_plain(lines: &[Line]) -> String {
    let mut s = String::new();
    for l in lines {
        s.push_str(&l.plain());
        s.push('\n');
    }
    s
}

#[derive(Clone, Debug)]
pub struct RenderOptions {
    /// Layout width in columns.
    pub width: usize,
    /// Whether color is output. When off, inline code is wrapped in backticks and code
    /// blocks are not highlighted.
    pub color: bool,
    /// Whether the terminal supports OSC 8 hyperlinks. If not, the URL is shown after
    /// the link text.
    pub hyperlinks: bool,
    /// Whether the terminal supports 24-bit truecolor (affects highlight colors).
    pub truecolor: bool,
    /// Enhanced styles for modern terminals; `None` uses the basic look. Only applies
    /// when `color` is true.
    pub rich: Option<Rich>,
    /// Display local images inline in the terminal; `None` shows images as text.
    pub images: Option<ImageOptions>,
}

impl RenderOptions {
    /// Plain-text layout with no color and no hyperlinks.
    pub fn plain(width: usize) -> Self {
        RenderOptions {
            width,
            color: false,
            hyperlinks: false,
            truecolor: false,
            rich: None,
            images: None,
        }
    }

    fn look(&self) -> Look {
        match &self.rich {
            Some(rich) if self.color => Look::rich(rich, self.truecolor),
            _ => Look::basic(),
        }
    }
}

/// Style type used by the wrapper.
#[derive(Clone, Debug, Default, PartialEq)]
struct SpanStyle {
    style: Style,
    link: Option<String>,
}

fn to_line(pieces: Vec<Piece<SpanStyle>>) -> Line {
    Line {
        spans: pieces
            .into_iter()
            .map(|p| Span {
                text: p.text,
                style: p.style.style,
                link: p.style.link,
                graphic: None,
            })
            .collect(),
    }
}

/// Parse and lay out Markdown.
pub fn render(markdown: &str, opts: &RenderOptions) -> Vec<Line> {
    render_blocks(&parse::parse(markdown), opts)
}

/// Lay out already-parsed blocks.
pub fn render_blocks(blocks: &[Block], opts: &RenderOptions) -> Vec<Line> {
    let r = Renderer {
        opts,
        look: opts.look(),
    };
    r.blocks(blocks, opts.width.max(MIN_WIDTH), 0, false)
}

struct Renderer<'a> {
    opts: &'a RenderOptions,
    look: Look,
}

/// Prefix each line: the first with `first`, the rest with `rest`. Empty lines keep only
/// the non-whitespace part of the prefix, to avoid trailing spaces.
fn prefix_lines(lines: Vec<Line>, first: &[Span], rest: &[Span]) -> Vec<Line> {
    lines
        .into_iter()
        .enumerate()
        .map(|(i, line)| {
            let prefix = if i == 0 { first } else { rest };
            let mut spans: Vec<Span> = prefix.to_vec();
            if line.is_empty() {
                while let Some(last) = spans.last_mut() {
                    let trimmed = last.text.trim_end().len();
                    if trimmed == 0 {
                        spans.pop();
                    } else {
                        last.text.truncate(trimmed);
                        break;
                    }
                }
            } else {
                spans.extend(line.spans);
            }
            Line { spans }
        })
        .collect()
}

fn span(text: impl Into<String>, style: Style) -> Span {
    Span {
        text: text.into(),
        style,
        ..Span::default()
    }
}

/// Expand tabs to spaces (tab stops every 4 columns).
fn expand_tabs(s: &str) -> String {
    if !s.contains('\t') {
        return s.to_string();
    }
    let mut out = String::new();
    let mut col = 0;
    for ch in s.chars() {
        if ch == '\t' {
            let n = 4 - col % 4;
            out.extend(std::iter::repeat_n(' ', n));
            col += n;
        } else {
            out.push(ch);
            col += unicode_width::UnicodeWidthChar::width(ch).unwrap_or(0);
        }
    }
    out
}

/// Allocate table column widths.
///
/// `natural` is each column's unwrapped width and `min` its longest word. If the total
/// fits, natural widths are used; otherwise the widest column is repeatedly shrunk by 1:
/// first only down to its longest word, then further if still needed, but never below
/// 2 columns (enough for one full-width character).
pub fn fit_columns(natural: &[usize], min: &[usize], available: usize) -> Vec<usize> {
    let mut widths: Vec<usize> = natural.iter().map(|&w| w.max(1)).collect();
    let mut total: usize = widths.iter().sum();
    let floor = |i: usize| natural[i].clamp(1, 2);
    let soft_floor = |i: usize| min.get(i).copied().unwrap_or(0).max(floor(i));
    for limit in [&soft_floor as &dyn Fn(usize) -> usize, &floor] {
        while total > available {
            let Some((i, &w)) = widths
                .iter()
                .enumerate()
                .filter(|&(i, &w)| w > limit(i))
                .max_by_key(|&(_, &w)| w)
            else {
                break;
            };
            widths[i] = w - 1;
            total -= 1;
        }
    }
    widths
}

impl Renderer<'_> {
    fn blocks(&self, blocks: &[Block], width: usize, depth: usize, tight: bool) -> Vec<Line> {
        let mut out = Vec::new();
        for (i, b) in blocks.iter().enumerate() {
            if i > 0 && !tight {
                out.push(Line::default());
            }
            out.extend(self.block(b, width, depth));
        }
        out
    }

    fn block(&self, block: &Block, width: usize, depth: usize) -> Vec<Line> {
        match block {
            Block::Heading { level, content } => self.heading(*level, content, width),
            Block::Paragraph(content) => self.paragraph(content, Style::new(), width),
            Block::CodeBlock { lang, code } => self.code_block(lang.as_deref(), code, width),
            Block::List {
                start,
                tight,
                items,
            } => self.list(*start, *tight, items, width, depth),
            Block::Quote { kind, blocks } => {
                let bar = theme::quote_bar(*kind);
                let mut inner = Vec::new();
                if let Some(k) = kind {
                    inner.push(Line::styled(theme::alert_label(*k), bar.bold()));
                }
                inner.extend(self.blocks(blocks, width.saturating_sub(2), depth, false));
                let prefix = [span(format!("{} ", self.look.quote_bar), bar)];
                prefix_lines(inner, &prefix, &prefix)
            }
            Block::Table { aligns, head, rows } => self.table(aligns, head, rows, width),
            Block::Rule => vec![Line::styled("─".repeat(width), theme::muted())],
            Block::Html(html) => {
                let html = html.trim_end_matches('\n');
                html.split('\n')
                    .flat_map(|l| {
                        let piece = Piece {
                            text: expand_tabs(l),
                            style: SpanStyle {
                                style: theme::muted(),
                                link: None,
                            },
                        };
                        wrap::hard_wrap(&[piece], width)
                    })
                    .map(to_line)
                    .collect()
            }
        }
    }

    fn span_style(&self, base: Style, s: &parse::InlineStyle) -> SpanStyle {
        SpanStyle {
            style: theme::inline(base, s, &self.look),
            link: if self.opts.hyperlinks {
                s.link.clone()
            } else {
                None
            },
        }
    }

    /// Inline content -> wrapper input. Handles image text, backticks when uncolored, and
    /// link URL suffixes.
    fn inline_items(&self, inlines: &Inlines, base: Style) -> Vec<Item<SpanStyle>> {
        let mut out = Vec::new();
        // Current link: (URL, link text).
        let mut group: Option<(String, String)> = None;
        for inline in inlines {
            let link = inline.style.link.as_deref();
            if group.as_ref().map(|(u, _)| u.as_str()) != link {
                if let Some((url, text)) = group.take() {
                    self.link_suffix(&url, &text, &mut out);
                }
                group = link.map(|u| (u.to_string(), String::new()));
            }
            match &inline.kind {
                InlineKind::Text(t) => {
                    if let Some((_, text)) = &mut group {
                        text.push_str(t);
                    }
                    let shown = if inline.style.image {
                        if t.trim().is_empty() {
                            "[image]".to_string()
                        } else {
                            format!("[image: {t}]")
                        }
                    } else if inline.style.code && !self.opts.color {
                        format!("`{t}`")
                    } else {
                        t.clone()
                    };
                    out.push(Item::Text(shown, self.span_style(base, &inline.style)));
                }
                InlineKind::SoftBreak => {
                    if let Some((_, text)) = &mut group {
                        text.push(' ');
                    }
                    out.push(Item::SoftBreak);
                }
                InlineKind::HardBreak => out.push(Item::HardBreak),
            }
        }
        if let Some((url, text)) = group {
            self.link_suffix(&url, &text, &mut out);
        }
        out
    }

    fn link_suffix(&self, url: &str, text: &str, out: &mut Vec<Item<SpanStyle>>) {
        if self.opts.hyperlinks || url.is_empty() {
            return;
        }
        let text = text.trim();
        if text == url || url.strip_prefix("mailto:") == Some(text) {
            return;
        }
        let style = SpanStyle {
            style: theme::link_url(),
            link: None,
        };
        out.push(Item::Text(" ".to_string(), style.clone()));
        out.push(Item::Text(format!("<{url}>"), style));
    }

    fn paragraph(&self, content: &Inlines, base: Style, width: usize) -> Vec<Line> {
        if let Some(lines) = self.image_paragraph(content, width) {
            return lines;
        }
        wrap::wrap(&self.inline_items(content, base), width)
            .into_iter()
            .map(to_line)
            .collect()
    }

    /// A paragraph containing only images: display them in the terminal with the alt text
    /// below. Returns `None` (render as text) when image display is disabled, the paragraph
    /// has other text, or an image can't be read.
    fn image_paragraph(&self, content: &Inlines, width: usize) -> Option<Vec<Line>> {
        let opts = self.opts.images.as_ref()?;
        let mut images = Vec::new();
        for inline in content {
            match &inline.kind {
                InlineKind::Text(alt) if inline.style.image => {
                    images.push((inline.style.link.as_deref().unwrap_or_default(), alt));
                }
                InlineKind::Text(t) if t.trim().is_empty() => {}
                InlineKind::SoftBreak | InlineKind::HardBreak => {}
                InlineKind::Text(_) => return None,
            }
        }
        if images.is_empty() {
            return None;
        }
        let loaded = images
            .into_iter()
            .map(|(src, alt)| graphics::load(src, opts, width).map(|g| (g, alt)))
            .collect::<Option<Vec<_>>>()?;

        let caption = SpanStyle {
            style: theme::muted().italic(),
            link: None,
        };
        let mut out = Vec::new();
        for (i, (g, alt)) in loaded.into_iter().enumerate() {
            if i > 0 {
                out.push(Line::default());
            }
            let rows = g.rows;
            out.push(Line {
                spans: vec![Span {
                    graphic: Some(Arc::new(g)),
                    ..Span::default()
                }],
            });
            out.extend(std::iter::repeat_n(Line::default(), rows.saturating_sub(1)));
            let alt = alt.trim();
            if !alt.is_empty() {
                let items = [Item::Text(alt.to_string(), caption.clone())];
                out.extend(wrap::wrap(&items, width).into_iter().map(to_line));
            }
        }
        Some(out)
    }

    fn heading(&self, level: u8, content: &Inlines, width: usize) -> Vec<Line> {
        let style = theme::heading(level);
        let mut items = Vec::new();
        if level >= 3 {
            items.push(Item::Text(
                format!("{} ", "#".repeat(level.into())),
                SpanStyle { style, link: None },
            ));
        }
        items.extend(self.inline_items(content, style));
        let mut lines: Vec<Line> = wrap::wrap(&items, width).into_iter().map(to_line).collect();
        if lines.is_empty() {
            lines.push(Line::default());
        }
        if level <= 2 {
            let w = lines
                .iter()
                .map(Line::width)
                .max()
                .unwrap_or(0)
                .clamp(1, width);
            let ch = if level == 1 { "━" } else { "─" };
            lines.push(Line::styled(ch.repeat(w), style));
        }
        lines
    }

    fn code_block(&self, lang: Option<&str>, code: &str, width: usize) -> Vec<Line> {
        let code = code.strip_suffix('\n').unwrap_or(code);
        let lines: Vec<String> = code
            .split('\n')
            .map(|l| expand_tabs(l.trim_end_matches('\r')))
            .collect();
        let highlighted = match lang {
            Some(lang) if self.opts.color => {
                let mut joined = lines.join("\n");
                joined.push('\n');
                highlight::highlight(&joined, lang, self.opts.truecolor, self.look.light)
                    .filter(|h| h.len() == lines.len())
            }
            _ => None,
        };
        let pieces = |i: usize, line: &String| -> Vec<Piece<SpanStyle>> {
            match &highlighted {
                Some(h) => h[i]
                    .iter()
                    .map(|(style, text)| Piece {
                        text: text.clone(),
                        style: SpanStyle {
                            style: *style,
                            link: None,
                        },
                    })
                    .collect(),
                None => vec![Piece {
                    text: line.clone(),
                    style: SpanStyle::default(),
                }],
            }
        };

        if let Some(panel) = &self.look.panel {
            return self.code_panel(panel, lang, &lines, pieces, width);
        }

        let border = theme::muted();
        let mut out = Vec::new();
        let mut head = Line::styled("╭─", border);
        if let Some(lang) = lang {
            head.push(" ", border);
            head.push(lang, theme::code_lang());
        }
        out.push(head);

        let inner = width.saturating_sub(2);
        for (i, line) in lines.iter().enumerate() {
            let pieces = pieces(i, line);
            let (indent, wrapped) = wrap_code_line(line, &pieces, inner);
            let first = [span("│ ", border)];
            let rest = [
                span("│ ", border),
                span(" ".repeat(indent), Style::new()),
                span(CONTINUATION_MARK, border),
                span(" ", Style::new()),
            ];
            out.extend(prefix_lines(wrapped, &first, &rest));
        }
        out.push(Line::styled("╰─", border));
        out
    }

    /// Code block in the enhanced look: a fully filled panel with the language name
    /// right-aligned at the top.
    ///
    /// ```text
    ///                    rust
    ///  fn main() {
    ///  }
    ///
    /// ```
    fn code_panel(
        &self,
        panel: &Panel,
        lang: Option<&str>,
        lines: &[String],
        pieces: impl Fn(usize, &String) -> Vec<Piece<SpanStyle>>,
        width: usize,
    ) -> Vec<Line> {
        let base = Style::new().bg_color(Some(panel.bg)).fg_color(panel.fg);
        let on_panel = |st: Style| {
            let st = st.bg_color(Some(panel.bg));
            if st.get_fg_color().is_none() {
                st.fg_color(panel.fg)
            } else {
                st
            }
        };
        // Pad a line to the panel width, with one blank column on each side.
        let fill = |spans: Vec<Span>| -> Line {
            let mut line = Line::styled(" ", base);
            line.spans.extend(spans);
            let pad = width.saturating_sub(line.width());
            if pad > 0 {
                line.push(" ".repeat(pad), base);
            }
            line
        };

        let mut out = Vec::new();
        let label = lang
            .filter(|l| display_width(l) + 2 <= width)
            .map(|l| {
                vec![
                    span(" ".repeat(width - display_width(l) - 2), base),
                    span(l, on_panel(panel.label)),
                ]
            })
            .unwrap_or_default();
        out.push(fill(label));

        let inner = width.saturating_sub(2);
        for (i, line) in lines.iter().enumerate() {
            let mut pieces = pieces(i, line);
            for p in &mut pieces {
                p.style.style = on_panel(p.style.style);
            }
            let (indent, wrapped) = wrap_code_line(line, &pieces, inner);
            for (j, l) in wrapped.into_iter().enumerate() {
                let mut spans = Vec::new();
                if j > 0 {
                    spans.push(span(" ".repeat(indent), base));
                    spans.push(span(CONTINUATION_MARK, on_panel(panel.label)));
                    spans.push(span(" ", base));
                }
                spans.extend(l.spans);
                out.push(fill(spans));
            }
        }
        out.push(fill(Vec::new()));
        out
    }

    fn list(
        &self,
        start: Option<u64>,
        tight: bool,
        items: &[ListItem],
        width: usize,
        depth: usize,
    ) -> Vec<Line> {
        const BULLETS: [&str; 3] = ["•", "◦", "▪"];
        let bullet = BULLETS[depth % BULLETS.len()];
        let num_width = start
            .map(|s| (s + items.len().saturating_sub(1) as u64).to_string().len())
            .unwrap_or(0);

        let mut out = Vec::new();
        for (i, item) in items.iter().enumerate() {
            if i > 0 && !tight {
                out.push(Line::default());
            }
            let mut marker: Vec<Span> = Vec::new();
            if let Some(s) = start {
                let n = s + i as u64;
                marker.push(span(format!("{n:>num_width$}."), theme::list_marker()));
                if item.task.is_some() {
                    marker.push(span(" ", Style::new()));
                }
            }
            match item.task {
                Some(true) => marker.push(span("[✓]", theme::task_done())),
                Some(false) => marker.push(span("[ ]", theme::task_todo())),
                None if start.is_none() => marker.push(span(bullet, theme::list_marker())),
                None => {}
            }
            marker.push(span(" ", Style::new()));
            let marker_width: usize = marker.iter().map(|s| display_width(&s.text)).sum();
            let indent = [span(" ".repeat(marker_width), Style::new())];

            let mut body = self.blocks(
                &item.blocks,
                width.saturating_sub(marker_width),
                depth + 1,
                tight,
            );
            if body.is_empty() {
                body.push(Line::default());
            }
            out.extend(prefix_lines(body, &marker, &indent));
        }
        out
    }

    fn table(
        &self,
        aligns: &[Align],
        head: &[Inlines],
        rows: &[Vec<Inlines>],
        width: usize,
    ) -> Vec<Line> {
        let ncols = rows
            .iter()
            .map(Vec::len)
            .chain([head.len()])
            .max()
            .unwrap_or(0);
        if ncols == 0 {
            return Vec::new();
        }
        let header_style = theme::table_header();
        let cell_items = |row: &[Inlines], c: usize, base: Style| -> Vec<Item<SpanStyle>> {
            row.get(c)
                .map(|cell| self.inline_items(cell, base))
                .unwrap_or_default()
        };

        // Each column's natural width (max unwrapped cell width) and longest word width.
        let mut natural = vec![0usize; ncols];
        let mut min = vec![0usize; ncols];
        for (r, row) in std::iter::once(head)
            .chain(rows.iter().map(Vec::as_slice))
            .enumerate()
        {
            let base = if r == 0 { header_style } else { Style::new() };
            for c in 0..ncols {
                let items = cell_items(row, c, base);
                let w = wrap::wrap(&items, usize::MAX)
                    .iter()
                    .map(|l| wrap::line_width(l))
                    .max()
                    .unwrap_or(0);
                natural[c] = natural[c].max(w);
                min[c] = min[c].max(wrap::min_width(&items));
            }
        }
        // Border overhead: one space on each side of every column, plus the bars.
        let overhead = 3 * ncols + 1;
        let widths = fit_columns(&natural, &min, width.saturating_sub(overhead));

        let border = theme::muted();
        let (top_l, top_r, bottom_l, bottom_r) = if self.look.rounded {
            ("╭", "╮", "╰", "╯")
        } else {
            ("┌", "┐", "└", "┘")
        };
        let rule = |l: &str, m: &str, r: &str| -> Line {
            let parts: Vec<String> = widths.iter().map(|w| "─".repeat(w + 2)).collect();
            Line::styled(format!("{l}{}{r}", parts.join(m)), border)
        };

        let render_row = |row: &[Inlines], base: Style| -> Vec<Line> {
            let cells: Vec<Vec<Line>> = (0..ncols)
                .map(|c| {
                    wrap::wrap(&cell_items(row, c, base), widths[c])
                        .into_iter()
                        .map(to_line)
                        .collect()
                })
                .collect();
            let height = cells.iter().map(Vec::len).max().unwrap_or(0).max(1);
            (0..height)
                .map(|i| {
                    let mut line = Line::styled("│", border);
                    for (c, cell) in cells.iter().enumerate() {
                        let content = cell.get(i).cloned().unwrap_or_default();
                        let pad = widths[c].saturating_sub(content.width());
                        let (left, right) = match aligns.get(c).copied().unwrap_or(Align::None) {
                            Align::Right => (pad, 0),
                            Align::Center => (pad / 2, pad - pad / 2),
                            Align::Left | Align::None => (0, pad),
                        };
                        line.push(" ".repeat(left + 1), Style::new());
                        line.spans.extend(content.spans);
                        line.push(" ".repeat(right + 1), Style::new());
                        line.push("│", border);
                    }
                    line
                })
                .collect()
        };

        let mut out = vec![rule(top_l, "┬", top_r)];
        out.extend(render_row(head, header_style));
        out.push(rule("├", "┼", "┤"));
        // Separate every body row: terminal lines have no spacing of their own, so stacked
        // rows look cramped, and wrapped rows need a boundary anyway. Always drawing them
        // also keeps a table looking the same whether or not any cell wraps.
        for (i, row) in rows.iter().enumerate() {
            if i > 0 {
                out.push(rule("├", "┼", "┤"));
            }
            out.extend(render_row(row, Style::new()));
        }
        out.push(rule(bottom_l, "┴", bottom_r));
        out
    }
}

/// Hard-wrap a code line to the width. Continuation lines align with the original
/// indentation and leave room for the ↪ marker; the indent is capped at half the width
/// so continuations still have room for content. Returns (continuation indent, lines).
fn wrap_code_line(line: &str, pieces: &[Piece<SpanStyle>], inner: usize) -> (usize, Vec<Line>) {
    let indent = line.len() - line.trim_start_matches(' ').len();
    let indent = indent.min(inner / 2);
    let rest_width = inner.saturating_sub(indent + CONTINUATION_WIDTH);
    let wrapped = wrap::hard_wrap_hanging(pieces, inner, rest_width)
        .into_iter()
        .map(to_line)
        .collect();
    (indent, wrapped)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plain(md: &str, width: usize) -> Vec<String> {
        render(md, &RenderOptions::plain(width))
            .iter()
            .map(Line::plain)
            .collect()
    }

    /// Display columns of all table bars in a line.
    fn bar_columns(line: &str) -> Vec<usize> {
        let mut cols = Vec::new();
        let mut col = 0;
        for ch in line.chars() {
            if matches!(
                ch,
                '│' | '┼' | '┬' | '┴' | '├' | '┤' | '┌' | '┐' | '└' | '┘'
            ) {
                cols.push(col);
            }
            col += unicode_width::UnicodeWidthChar::width(ch).unwrap_or(0);
        }
        cols
    }

    fn assert_table_aligned(lines: &[String]) {
        let expected = bar_columns(&lines[0]);
        assert!(expected.len() >= 2);
        for l in lines {
            assert_eq!(
                bar_columns(l),
                expected,
                "misaligned line: {l:?}\n{}",
                lines.join("\n")
            );
            assert_eq!(display_width(l), display_width(&lines[0]), "{l:?}");
        }
    }

    #[test]
    fn fit_columns_shrinks_widest_first() {
        assert_eq!(fit_columns(&[3, 5, 2], &[0; 3], 20), vec![3, 5, 2]);
        assert_eq!(fit_columns(&[4, 20, 6], &[0; 3], 20), vec![4, 10, 6]);
        assert_eq!(fit_columns(&[10, 10], &[0; 2], 10), vec![5, 5]);
        // Never below 2 columns, even when it doesn't fit.
        assert_eq!(fit_columns(&[10, 10], &[0; 2], 2), vec![2, 2]);
        assert_eq!(fit_columns(&[0, 1], &[0; 2], 0), vec![1, 1]);
    }

    #[test]
    fn fit_columns_avoids_splitting_words() {
        // Column 1 is a 10-column word; other columns should give up space.
        assert_eq!(fit_columns(&[10, 12], &[10, 2], 16), vec![10, 6]);
        // Words are split only when all columns are at word width and it still doesn't fit.
        assert_eq!(fit_columns(&[10, 12], &[10, 2], 8), vec![6, 2]);
    }

    #[test]
    fn cjk_table_is_aligned() {
        let md = "\
| 名称 | 说明 | 数量 |
|:-----|:----:|-----:|
| 苹果 | 红色的水果 | 3 |
| Banana | 黄色 | 12 |
| 混合 mixed 内容 | 中English文 | 100 |
";
        let lines = plain(md, 80);
        assert_eq!(lines.len(), 9);
        assert_table_aligned(&lines);
        // Alignment: left / center / right.
        assert_eq!(
            lines[3],
            "│ 苹果            │ 红色的水果  │ 数量 │".replace("数量", "   3")
        );
        assert!(lines[5].starts_with("│ Banana          │    黄色     │"));
        assert!(lines[7].ends_with("│  100 │"));
    }

    #[test]
    fn table_rows_get_separators() {
        let md = "\
| 项目 | 描述 |
|------|------|
| 生成 | 这一格的内容较长需要换行 |
| 判定 | 短 |
| 统计 | 也短 |
";
        let lines = plain(md, 24);
        assert_table_aligned(&lines);
        assert_eq!(
            lines,
            vec![
                "┌──────┬───────────────┐",
                "│ 项目 │ 描述          │",
                "├──────┼───────────────┤",
                "│ 生成 │ 这一格的内容  │",
                "│      │ 较长需要换行  │",
                "├──────┼───────────────┤",
                "│ 判定 │ 短            │",
                "├──────┼───────────────┤",
                "│ 统计 │ 也短          │",
                "└──────┴───────────────┘",
            ]
        );
        // Single-line rows are separated too, so rows don't look cramped.
        let lines = plain(md, 80);
        assert_table_aligned(&lines);
        assert_eq!(
            lines,
            vec![
                "┌──────┬──────────────────────────┐",
                "│ 项目 │ 描述                     │",
                "├──────┼──────────────────────────┤",
                "│ 生成 │ 这一格的内容较长需要换行 │",
                "├──────┼──────────────────────────┤",
                "│ 判定 │ 短                       │",
                "├──────┼──────────────────────────┤",
                "│ 统计 │ 也短                     │",
                "└──────┴──────────────────────────┘",
            ]
        );
    }

    #[test]
    fn narrow_table_wraps_cells_and_stays_aligned() {
        let md = "\
| 项目 | 描述 |
|------|------|
| 换行 | 这是一段很长的中文描述，需要在单元格里自动换行才能放得下 |
| 英文 | The quick brown fox jumps over the lazy dog |
";
        for width in [20, 24, 31, 40] {
            let lines = plain(md, width);
            assert_table_aligned(&lines);
            for l in &lines {
                assert!(display_width(l) <= width, "width={width} {l:?}");
            }
        }
    }

    #[test]
    fn nested_list_indentation() {
        let md = "- 第一层\n  - 第二层\n    - 第三层\n      - 第四层\n- 回到第一层\n";
        assert_eq!(
            plain(md, 40),
            vec![
                "• 第一层",
                "  ◦ 第二层",
                "    ▪ 第三层",
                "      • 第四层",
                "• 回到第一层",
            ]
        );
    }

    #[test]
    fn ordered_list_numbers_are_right_aligned() {
        let md = (1..=10)
            .map(|i| format!("{i}. 第{i}项\n"))
            .collect::<String>();
        let lines = plain(&md, 40);
        assert_eq!(lines[0], " 1. 第1项");
        assert_eq!(lines[9], "10. 第10项");
    }

    #[test]
    fn mixed_nested_lists_and_tasks() {
        let md = "1. 步骤一\n   - 细节 a\n   - [x] 已完成\n   - [ ] 未完成\n2. 步骤二\n";
        assert_eq!(
            plain(md, 40),
            vec![
                "1. 步骤一",
                "   ◦ 细节 a",
                "   [✓] 已完成",
                "   [ ] 未完成",
                "2. 步骤二",
            ]
        );
    }

    #[test]
    fn list_item_continuation_is_indented() {
        let md =
            "- 这是一个很长的列表项，内容会超过一行的宽度\n  - 嵌套的列表项同样很长需要换行显示\n";
        let lines = plain(md, 20);
        assert_eq!(
            lines,
            vec![
                "• 这是一个很长的列表",
                "  项，内容会超过一行",
                "  的宽度",
                "  ◦ 嵌套的列表项同样",
                "    很长需要换行显示",
            ]
        );
    }

    #[test]
    fn loose_list_has_blank_lines() {
        let lines = plain("- 甲\n\n- 乙\n", 40);
        assert_eq!(lines, vec!["• 甲", "", "• 乙"]);
    }

    #[test]
    fn blockquote_prefix_and_nesting() {
        let lines = plain("> 引用第一段\n>\n> > 嵌套引用\n", 40);
        assert_eq!(lines, vec!["│ 引用第一段", "│", "│ │ 嵌套引用"]);
    }

    #[test]
    fn links_without_hyperlink_support_show_url() {
        let lines = plain("见 [官网](https://www.rust-lang.org) 和 <https://a.b>", 80);
        assert_eq!(
            lines,
            vec!["见 官网 <https://www.rust-lang.org> 和 https://a.b"]
        );
    }

    #[test]
    fn links_with_hyperlinks_carry_url() {
        let opts = RenderOptions {
            hyperlinks: true,
            ..RenderOptions::plain(80)
        };
        let lines = render("[官网](https://www.rust-lang.org)", &opts);
        assert_eq!(lines[0].plain(), "官网");
        assert_eq!(
            lines[0].spans[0].link.as_deref(),
            Some("https://www.rust-lang.org")
        );
    }

    #[test]
    fn image_shows_alt_and_url() {
        let lines = plain("![示意图](img/a.png)", 80);
        assert_eq!(lines, vec!["[image: 示意图] <img/a.png>"]);
    }

    #[test]
    fn inline_code_gets_backticks_without_color() {
        assert_eq!(
            plain("运行 `cargo test` 即可", 80),
            vec!["运行 `cargo test` 即可"]
        );
        let colored = RenderOptions {
            color: true,
            ..RenderOptions::plain(80)
        };
        assert_eq!(render("`x`", &colored)[0].plain(), "x");
    }

    #[test]
    fn code_block_is_framed_and_hard_wrapped() {
        let lines = plain("```rust\nfn main() {\n\tprintln!(\"你好\");\n}\n```\n", 16);
        assert_eq!(
            lines,
            vec![
                "╭─ rust",
                "│ fn main() {",
                "│     println!(\"",
                "│     ↪ 你好\");",
                "│ }",
                "╰─",
            ]
        );
    }

    #[test]
    fn code_continuation_lines_are_indented_and_marked() {
        let lines = plain(
            "```\n  abcdefghijklmnopqrstuvwxyz\nno-indent-but-quite-long\n```\n",
            16,
        );
        assert_eq!(
            lines,
            vec![
                "╭─",
                "│   abcdefghijkl",
                "│   ↪ mnopqrstuv",
                "│   ↪ wxyz",
                "│ no-indent-but-",
                "│ ↪ quite-long",
                "╰─",
            ]
        );
        for l in &lines {
            assert!(display_width(l) <= 16, "{l:?}");
        }
    }

    #[test]
    fn inline_code_stands_out_in_headings() {
        let opts = RenderOptions {
            color: true,
            ..RenderOptions::plain(80)
        };
        let lines = render("### 标的：`config.toml`", &opts);
        let find = |t: &str| {
            lines[0]
                .spans
                .iter()
                .find(|s| s.text.contains(t))
                .unwrap()
                .style
        };
        let text = find("标的");
        let code = find("config.toml");
        assert_eq!(text, theme::heading(3));
        assert_eq!(code, theme::code_inline());
        assert_ne!(text.get_fg_color(), code.get_fg_color());
        assert!(!code.get_effects().contains(anstyle::Effects::BOLD));
    }

    #[test]
    fn headings() {
        let lines = plain("# 标题一\n\n## 标题二\n\n### 标题三\n", 40);
        assert_eq!(
            lines,
            vec!["标题一", "━━━━━━", "", "标题二", "──────", "", "### 标题三"]
        );
    }

    #[test]
    fn rule_spans_full_width() {
        assert_eq!(plain("---", 12), vec!["─".repeat(12)]);
    }

    fn rich_opts(width: usize, background: Option<(u8, u8, u8)>) -> RenderOptions {
        RenderOptions {
            color: true,
            truecolor: true,
            rich: Some(Rich {
                styled_underline: true,
                background,
            }),
            ..RenderOptions::plain(width)
        }
    }

    #[test]
    fn rich_code_block_is_a_filled_panel() {
        let opts = rich_opts(20, Some((0x28, 0x2c, 0x34)));
        let lines = render(
            "```rust\nfn main() {\n    let s = \"很长很长的字符串\";\n}\n```\n",
            &opts,
        );
        let text: Vec<String> = lines.iter().map(Line::plain).collect();
        assert_eq!(
            text,
            vec![
                "               rust ",
                " fn main() {        ",
                "     let s = \"很长  ",
                "     ↪ 很长的字符串 ",
                "     ↪ \";           ",
                " }                  ",
                "                    ",
            ]
        );
        let Look {
            panel: Some(panel), ..
        } = opts.look()
        else {
            panic!("rich look has a panel");
        };
        // Every cell is filled with the background.
        for l in &lines {
            assert_eq!(l.width(), 20);
            for s in &l.spans {
                assert_eq!(s.style.get_bg_color(), Some(panel.bg), "{s:?}");
            }
        }
        // On a dark background the panel is slightly lighter.
        assert_eq!(panel.bg, anstyle::Color::Rgb(anstyle::RgbColor(57, 61, 68)));
    }

    #[test]
    fn rich_look_follows_terminal_background() {
        let light = rich_opts(40, Some((255, 255, 255))).look();
        assert!(light.light);
        assert!(light.code_chip.is_some());
        // With an unknown background, use the highlight theme's own dark panel and text color.
        let unknown = rich_opts(40, None).look();
        assert!(!unknown.light);
        assert!(unknown.code_chip.is_none());
        assert!(unknown.panel.unwrap().fg.is_some());
        // The enhanced look doesn't apply without color.
        let no_color = RenderOptions {
            color: false,
            ..rich_opts(40, None)
        };
        assert_eq!(no_color.look(), Look::basic());
    }

    #[test]
    fn rich_quotes_tables_and_links() {
        let opts = rich_opts(40, Some((0, 0, 0)));
        let text: Vec<String> = render("> 引用\n\n| a |\n|---|\n| b |\n", &opts)
            .iter()
            .map(Line::plain)
            .collect();
        assert_eq!(
            text,
            vec!["▎ 引用", "", "╭───╮", "│ a │", "├───┤", "│ b │", "╰───╯"]
        );

        let lines = render("[链接](https://a.b) `代码`", &opts);
        let link = &lines[0].spans[0];
        assert!(
            link.style
                .get_effects()
                .contains(anstyle::Effects::DOTTED_UNDERLINE)
        );
        let code = lines[0].spans.iter().find(|s| s.text == "代码").unwrap();
        assert!(code.style.get_bg_color().is_some());
    }

    #[test]
    fn image_paragraphs_become_graphics() {
        let dir = std::env::temp_dir().join(format!("mdr-render-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        image::RgbImage::from_pixel(100, 60, image::Rgb([0, 120, 200]))
            .save(dir.join("pic.png"))
            .unwrap();
        let opts = RenderOptions {
            images: Some(ImageOptions {
                base_dir: dir.clone(),
                cell: (10, 20),
                max_rows: 20,
            }),
            ..rich_opts(40, None)
        };

        // 100×60 pixels -> 10 columns × 3 rows, with the alt text below.
        let lines = render("> ![示意图](pic.png)\n", &opts);
        assert_eq!(lines.len(), 4);
        let g = lines[0]
            .spans
            .iter()
            .find_map(|s| s.graphic.clone())
            .unwrap();
        assert_eq!((g.cols, g.rows), (10, 3));
        let text: Vec<String> = lines.iter().map(Line::plain).collect();
        assert_eq!(text, vec!["▎ ", "▎", "▎", "▎ 示意图"]);

        // Mixed with text, remote images, unreadable files: still shown as text.
        for md in [
            "看图 ![示意图](pic.png)",
            "![远程](https://a.b/x.png)",
            "![丢失](missing.png)",
        ] {
            let lines = render(md, &opts);
            assert!(
                lines
                    .iter()
                    .all(|l| l.spans.iter().all(|s| s.graphic.is_none()))
            );
            assert!(lines[0].plain().contains("[image: "), "{md}");
        }
        std::fs::remove_dir_all(dir).unwrap();
    }
}
