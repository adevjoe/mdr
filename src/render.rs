//! 排版：把块级树排成一行行带样式的文字（[`Line`]）。
//!
//! 这里不输出任何转义序列，只决定每行有哪些文字、各自什么样式；
//! 转成终端字节流的工作在 [`crate::output`]。

use anstyle::Style;

use crate::highlight;
use crate::parse::{self, Align, Block, InlineKind, Inlines, ListItem};
use crate::theme;
use crate::wrap::{self, Item, Piece, display_width};

/// 最小排版宽度。
pub const MIN_WIDTH: usize = 10;

/// 一段带样式的文字。`link` 只在启用终端超链接时才会设置。
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Span {
    pub text: String,
    pub style: Style,
    pub link: Option<String>,
}

/// 排版后的一行。
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Line {
    pub spans: Vec<Span>,
}

impl Line {
    pub fn width(&self) -> usize {
        self.spans.iter().map(|s| display_width(&s.text)).sum()
    }

    /// 去掉样式后的纯文本。
    pub fn plain(&self) -> String {
        self.spans.iter().map(|s| s.text.as_str()).collect()
    }

    pub fn is_empty(&self) -> bool {
        self.spans.iter().all(|s| s.text.is_empty())
    }

    fn push(&mut self, text: impl Into<String>, style: Style) {
        self.spans.push(Span {
            text: text.into(),
            style,
            link: None,
        });
    }

    fn styled(text: impl Into<String>, style: Style) -> Line {
        let mut l = Line::default();
        l.push(text, style);
        l
    }
}

/// 把多行文字转成纯文本，每行以 `\n` 结尾。测试和 `--no-color` 都用它。
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
    /// 排版宽度（列）。
    pub width: usize,
    /// 是否会输出颜色。关闭时行内代码用反引号标出，代码块不做高亮。
    pub color: bool,
    /// 终端是否支持 OSC 8 超链接。不支持时在链接文字后显示网址。
    pub hyperlinks: bool,
    /// 终端是否支持 24 位真彩色（影响代码高亮的配色）。
    pub truecolor: bool,
}

impl RenderOptions {
    /// 无颜色、无超链接的纯文本排版。
    pub fn plain(width: usize) -> Self {
        RenderOptions {
            width,
            color: false,
            hyperlinks: false,
            truecolor: false,
        }
    }
}

/// 换行器使用的样式类型。
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
            })
            .collect(),
    }
}

/// 解析并排版 Markdown。
pub fn render(markdown: &str, opts: &RenderOptions) -> Vec<Line> {
    render_blocks(&parse::parse(markdown), opts)
}

/// 排版已经解析好的块。
pub fn render_blocks(blocks: &[Block], opts: &RenderOptions) -> Vec<Line> {
    let r = Renderer { opts };
    r.blocks(blocks, opts.width.max(MIN_WIDTH), 0, false)
}

struct Renderer<'a> {
    opts: &'a RenderOptions,
}

/// 给每行加前缀：第一行用 `first`，其余行用 `rest`。空行只保留前缀里的非空白部分，
/// 避免行尾多出空格。
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
        link: None,
    }
}

/// 把制表符展开成空格（制表位每 4 列）。
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

/// 表格列宽分配。
///
/// `natural` 是各列不换行时的宽度，`min` 是各列最长单词的宽度。总宽放得下就用自然宽度；
/// 否则反复把最宽的一列减 1：先只缩到不拆单词为止，还放不下再继续缩，
/// 但每列至少保留 2 列（放得下一个全角字符）。
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
                let prefix = [span("│ ", bar)];
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
            style: theme::inline(base, s),
            link: if self.opts.hyperlinks {
                s.link.clone()
            } else {
                None
            },
        }
    }

    /// 行内内容 → 换行器输入。处理图片文字、无颜色时的反引号、以及链接网址后缀。
    fn inline_items(&self, inlines: &Inlines, base: Style) -> Vec<Item<SpanStyle>> {
        let mut out = Vec::new();
        // 当前链接：(网址, 链接文字)。
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
                            "[图片]".to_string()
                        } else {
                            format!("[图片: {t}]")
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
        wrap::wrap(&self.inline_items(content, base), width)
            .into_iter()
            .map(to_line)
            .collect()
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
        let border = theme::muted();
        let mut out = Vec::new();

        let mut head = Line::styled("╭─", border);
        if let Some(lang) = lang {
            head.push(" ", border);
            head.push(lang, theme::code_lang());
        }
        out.push(head);

        let code = code.strip_suffix('\n').unwrap_or(code);
        let lines: Vec<String> = code
            .split('\n')
            .map(|l| expand_tabs(l.trim_end_matches('\r')))
            .collect();
        let highlighted = match lang {
            Some(lang) if self.opts.color => {
                let mut joined = lines.join("\n");
                joined.push('\n');
                highlight::highlight(&joined, lang, self.opts.truecolor)
                    .filter(|h| h.len() == lines.len())
            }
            _ => None,
        };

        let inner = width.saturating_sub(2);
        for (i, line) in lines.iter().enumerate() {
            let pieces: Vec<Piece<SpanStyle>> = match &highlighted {
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
            };
            let wrapped: Vec<Line> = wrap::hard_wrap(&pieces, inner)
                .into_iter()
                .map(to_line)
                .collect();
            let prefix = [span("│ ", border)];
            out.extend(prefix_lines(wrapped, &prefix, &prefix));
        }
        out.push(Line::styled("╰─", border));
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

        // 每列的自然宽度（单元格不换行时的最大宽度）和最长单词宽度。
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
        // 边框占用：每列左右各一个空格 + 竖线。
        let overhead = 3 * ncols + 1;
        let widths = fit_columns(&natural, &min, width.saturating_sub(overhead));

        let border = theme::muted();
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

        let mut out = vec![rule("┌", "┬", "┐")];
        out.extend(render_row(head, header_style));
        out.push(rule("├", "┼", "┤"));
        for row in rows {
            out.extend(render_row(row, Style::new()));
        }
        out.push(rule("└", "┴", "┘"));
        out
    }
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

    /// 一行中所有表格竖线所在的显示列。
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
                "未对齐的行: {l:?}\n{}",
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
        // 放不下时也不低于 2 列。
        assert_eq!(fit_columns(&[10, 10], &[0; 2], 2), vec![2, 2]);
        assert_eq!(fit_columns(&[0, 1], &[0; 2], 0), vec![1, 1]);
    }

    #[test]
    fn fit_columns_avoids_splitting_words() {
        // 第 1 列是一个 10 列宽的单词，应该由其他列让出空间。
        assert_eq!(fit_columns(&[10, 12], &[10, 2], 16), vec![10, 6]);
        // 所有列都到单词宽度了还放不下，才拆单词。
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
        assert_eq!(lines.len(), 7);
        assert_table_aligned(&lines);
        // 对齐方式：左 / 居中 / 右。
        assert_eq!(
            lines[3],
            "│ 苹果            │ 红色的水果  │ 数量 │".replace("数量", "   3")
        );
        assert!(lines[4].starts_with("│ Banana          │    黄色     │"));
        assert!(lines[5].ends_with("│  100 │"));
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
        assert_eq!(lines, vec!["[图片: 示意图] <img/a.png>"]);
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
                "│ 你好\");",
                "│ }",
                "╰─",
            ]
        );
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
}
