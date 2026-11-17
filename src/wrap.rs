//! Line wrapping by display width.
//!
//! Full-width characters (Chinese, Japanese, etc.) occupy 2 columns in a terminal. `unicode-width`
//! computes widths; this module decides where lines may break:
//!
//! - Latin text breaks at spaces; words are never split (unless wider than the whole line);
//! - a break is allowed between any two full-width characters;
//! - simple kinsoku shori (CJK line-breaking rules): closing punctuation such as `，。）」` never
//!   starts a line, and opening punctuation such as `（「《` never ends a line;
//! - soft line breaks between CJK characters in the source do not produce extra spaces.
//!
//! The wrapper is generic over the style type `S`, so it does not depend on terminal output and is
//! easy to unit test.

use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

/// Number of terminal columns a string occupies.
pub fn display_width(s: &str) -> usize {
    UnicodeWidthStr::width(s)
}

/// A run of styled text.
#[derive(Clone, Debug, PartialEq)]
pub struct Piece<S> {
    pub text: String,
    pub style: S,
}

/// Input to the wrapper.
#[derive(Clone, Debug, PartialEq)]
pub enum Item<S> {
    Text(String, S),
    /// A Markdown soft break: usually equivalent to a space, but nothing at all between CJK
    /// characters.
    SoftBreak,
    /// A hard line break.
    HardBreak,
}

/// A single wrapped line.
pub type WrappedLine<S> = Vec<Piece<S>>;

/// Display width of a line.
pub fn line_width<S>(line: &[Piece<S>]) -> usize {
    line.iter().map(|p| display_width(&p.text)).sum()
}

/// Punctuation that must not start a line (kinsoku: no-line-start).
const CLOSING: &[&str] = &[
    "，", "。", "、", "；", "：", "？", "！", "）", "」", "』", "》", "〉", "】", "〕", "］", "｝",
    "”", "’", "…", "～", ",", ".", ";", ":", "!", "?", ")", "]", "}", "%",
];

/// Punctuation that must not end a line (kinsoku: no-line-end).
const OPENING: &[&str] = &[
    "（", "「", "『", "《", "〈", "【", "〔", "［", "｛", "“", "‘", "(", "[", "{",
];

fn is_closing(g: &str) -> bool {
    CLOSING.contains(&g)
}

fn is_opening(g: &str) -> bool {
    OPENING.contains(&g)
}

fn is_space(g: &str) -> bool {
    // A non-breaking space (NBSP) is not a break opportunity.
    g != "\u{a0}" && g.chars().all(char::is_whitespace)
}

fn append<S: Clone + PartialEq>(pieces: &mut Vec<Piece<S>>, text: &str, style: &S) {
    match pieces.last_mut() {
        Some(last) if last.style == *style => last.text.push_str(text),
        _ => pieces.push(Piece {
            text: text.to_string(),
            style: style.clone(),
        }),
    }
}

enum Tok<S> {
    /// An unbreakable segment (a Latin word, a single CJK character with its attached
    /// punctuation, ...) and its width. A break is allowed between two adjacent Words.
    Word(Vec<Piece<S>>, usize),
    /// A space: a break opportunity, dropped when the line breaks there.
    Space(S),
    Hard,
}

struct Tokenizer<S> {
    toks: Vec<Tok<S>>,
    cur: Vec<Piece<S>>,
    cur_w: usize,
    /// Just saw opening punctuation; the next character must stick to it.
    glue: bool,
    /// A soft break not yet resolved into a space or nothing.
    soft: Option<S>,
    last_wide: bool,
    last_style: Option<S>,
}

impl<S: Clone + PartialEq> Tokenizer<S> {
    fn new() -> Self {
        Tokenizer {
            toks: Vec::new(),
            cur: Vec::new(),
            cur_w: 0,
            glue: false,
            soft: None,
            last_wide: false,
            last_style: None,
        }
    }

    fn push_cur(&mut self, g: &str, s: &S) {
        append(&mut self.cur, g, s);
        self.cur_w += display_width(g);
    }

    fn flush(&mut self) {
        if !self.cur.is_empty() {
            let pieces = std::mem::take(&mut self.cur);
            let w = std::mem::take(&mut self.cur_w);
            self.toks.push(Tok::Word(pieces, w));
        }
    }

    fn space(&mut self, s: &S) {
        self.flush();
        self.glue = false;
        if !matches!(self.toks.last(), Some(Tok::Space(_))) {
            self.toks.push(Tok::Space(s.clone()));
        }
    }

    fn grapheme(&mut self, g: &str, s: &S) {
        self.last_style = Some(s.clone());
        if is_space(g) {
            self.soft = None;
            self.space(s);
            return;
        }
        let w = display_width(g);
        let wide = w >= 2;

        // Soft break: join directly if both sides are full-width, otherwise treat as a space.
        if let Some(soft_style) = self.soft.take()
            && !(self.last_wide && (wide || is_closing(g)))
        {
            self.space(&soft_style);
        }

        if is_closing(g) {
            if !self.cur.is_empty() {
                self.push_cur(g, s);
            } else if let Some(Tok::Word(pieces, pw)) = self.toks.last_mut() {
                append(pieces, g, s);
                *pw += w;
            } else {
                self.push_cur(g, s);
            }
            self.glue = false;
            if wide {
                self.flush();
            }
        } else if is_opening(g) {
            if wide || g == "“" || g == "‘" {
                self.flush();
            }
            self.push_cur(g, s);
            self.glue = true;
        } else if wide {
            if self.glue {
                self.push_cur(g, s);
                self.flush();
            } else {
                self.flush();
                self.push_cur(g, s);
                self.flush();
            }
            self.glue = false;
        } else {
            self.glue = false;
            self.push_cur(g, s);
        }
        self.last_wide = wide;
    }

    fn soft_break(&mut self) {
        self.flush();
        self.glue = false;
        if let Some(s) = self.last_style.clone() {
            self.soft = Some(s);
        }
    }

    fn hard_break(&mut self) {
        self.flush();
        self.glue = false;
        self.soft = None;
        self.toks.push(Tok::Hard);
    }

    fn finish(mut self) -> Vec<Tok<S>> {
        self.flush();
        self.toks
    }
}

fn tokenize<S: Clone + PartialEq>(items: &[Item<S>]) -> Vec<Tok<S>> {
    let mut t = Tokenizer::new();
    for item in items {
        match item {
            Item::Text(text, style) => {
                for g in text.graphemes(true) {
                    t.grapheme(g, style);
                }
            }
            Item::SoftBreak => t.soft_break(),
            Item::HardBreak => t.hard_break(),
        }
    }
    t.finish()
}

/// Wraps styled inline content to `width` columns.
///
/// Every returned line is at most `width` wide (`width` is treated as at least 2 so a full-width
/// character always fits). Returns an empty list for empty content.
pub fn wrap<S: Clone + PartialEq>(items: &[Item<S>], width: usize) -> Vec<WrappedLine<S>> {
    let width = width.max(2);
    let mut lines = Vec::new();
    let mut line: WrappedLine<S> = Vec::new();
    let mut w = 0usize;
    let mut pending_space: Option<S> = None;

    for tok in tokenize(items) {
        match tok {
            Tok::Hard => {
                lines.push(std::mem::take(&mut line));
                w = 0;
                pending_space = None;
            }
            Tok::Space(s) => {
                if w > 0 {
                    pending_space = Some(s);
                }
            }
            Tok::Word(pieces, ww) => {
                let sp = usize::from(pending_space.is_some());
                if w > 0 && w + sp + ww > width {
                    lines.push(std::mem::take(&mut line));
                    w = 0;
                    pending_space = None;
                }
                if let Some(s) = pending_space.take() {
                    append(&mut line, " ", &s);
                    w += 1;
                }
                if w + ww <= width {
                    for p in &pieces {
                        append(&mut line, &p.text, &p.style);
                    }
                    w += ww;
                } else {
                    // A single "word" is wider than the line: hard-split it by grapheme cluster.
                    for p in &pieces {
                        for g in p.text.graphemes(true) {
                            let gw = display_width(g);
                            if w > 0 && w + gw > width {
                                lines.push(std::mem::take(&mut line));
                                w = 0;
                            }
                            append(&mut line, g, &p.style);
                            w += gw;
                        }
                    }
                }
            }
        }
    }
    if !line.is_empty() {
        lines.push(line);
    }
    lines
}

/// Width of the longest unbreakable segment (e.g. an English word), i.e. the minimum width
/// achievable without hard-splitting words.
pub fn min_width<S: Clone + PartialEq>(items: &[Item<S>]) -> usize {
    tokenize(items)
        .iter()
        .map(|t| match t {
            Tok::Word(_, w) => *w,
            _ => 0,
        })
        .max()
        .unwrap_or(0)
}

/// Splits strictly by width, ignoring word boundaries (used for code blocks). Always returns at
/// least one line.
pub fn hard_wrap<S: Clone + PartialEq>(pieces: &[Piece<S>], width: usize) -> Vec<WrappedLine<S>> {
    hard_wrap_hanging(pieces, width, width)
}

/// Like [`hard_wrap`], but the first line is `first` wide and subsequent lines are `rest` wide
/// (leaving room for continuation-line indentation and markers).
pub fn hard_wrap_hanging<S: Clone + PartialEq>(
    pieces: &[Piece<S>],
    first: usize,
    rest: usize,
) -> Vec<WrappedLine<S>> {
    let (first, rest) = (first.max(2), rest.max(2));
    let mut lines = Vec::new();
    let mut line: WrappedLine<S> = Vec::new();
    let mut w = 0usize;
    for p in pieces {
        for g in p.text.graphemes(true) {
            let gw = display_width(g);
            let width = if lines.is_empty() { first } else { rest };
            if w > 0 && w + gw > width {
                lines.push(std::mem::take(&mut line));
                w = 0;
            }
            append(&mut line, g, &p.style);
            w += gw;
        }
    }
    lines.push(line);
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(s: &str) -> Item<()> {
        Item::Text(s.to_string(), ())
    }

    fn plain(lines: &[WrappedLine<()>]) -> Vec<String> {
        lines
            .iter()
            .map(|l| l.iter().map(|p| p.text.as_str()).collect())
            .collect()
    }

    fn wrap_str(s: &str, width: usize) -> Vec<String> {
        plain(&wrap(&[text(s)], width))
    }

    #[test]
    fn cjk_counts_as_two_columns() {
        assert_eq!(display_width("中文"), 4);
        assert_eq!(display_width("ab中"), 4);
        assert_eq!(wrap_str("中文字符测试", 6), vec!["中文字", "符测试"]);
    }

    #[test]
    fn odd_width_never_splits_a_wide_char() {
        let lines = wrap_str("中文中文中文", 5);
        assert_eq!(lines, vec!["中文", "中文", "中文"]);
    }

    #[test]
    fn latin_words_are_kept_whole() {
        assert_eq!(
            wrap_str("hello world foo bar", 11),
            vec!["hello world", "foo bar"]
        );
    }

    #[test]
    fn overlong_word_is_hard_split() {
        assert_eq!(wrap_str("abcdefghij", 4), vec!["abcd", "efgh", "ij"]);
    }

    #[test]
    fn mixed_cjk_and_latin() {
        let lines = wrap_str("Rust 是一门注重安全和性能的系统编程语言", 12);
        for l in &lines {
            assert!(display_width(l) <= 12, "{l:?} too wide");
        }
        assert_eq!(
            lines.concat().replace(' ', ""),
            "Rust是一门注重安全和性能的系统编程语言"
        );
        assert_eq!(lines[0], "Rust 是一门");
    }

    #[test]
    fn closing_punctuation_never_starts_a_line() {
        // At width 6 the "，" after "三" would start a line; it must move to the next line together
        // with "三".
        let lines = wrap_str("一二三，四五六。七八九", 6);
        assert_eq!(lines, vec!["一二", "三，四", "五六。", "七八九"]);
        for l in &lines {
            assert!(!l.starts_with('，') && !l.starts_with('。'));
        }
    }

    #[test]
    fn opening_punctuation_never_ends_a_line() {
        // Without this rule we would get "一二（" + "三四".
        let lines = wrap_str("一二（三四", 6);
        assert_eq!(lines, vec!["一二", "（三四"]);
    }

    #[test]
    fn soft_break_between_cjk_adds_no_space() {
        let items = vec![text("中文"), Item::SoftBreak, text("继续")];
        assert_eq!(plain(&wrap(&items, 80)), vec!["中文继续"]);
        let items = vec![text("foo"), Item::SoftBreak, text("bar")];
        assert_eq!(plain(&wrap(&items, 80)), vec!["foo bar"]);
        let items = vec![text("中文"), Item::SoftBreak, text("abc")];
        assert_eq!(plain(&wrap(&items, 80)), vec!["中文 abc"]);
    }

    #[test]
    fn hard_break_forces_new_line() {
        let items = vec![text("第一行"), Item::HardBreak, text("第二行")];
        assert_eq!(plain(&wrap(&items, 80)), vec!["第一行", "第二行"]);
    }

    #[test]
    fn styles_survive_wrapping() {
        let items = vec![
            Item::Text("普通".to_string(), 0),
            Item::Text("加粗文字".to_string(), 1),
        ];
        let lines = wrap(&items, 6);
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0][0].text, "普通");
        assert_eq!(lines[0][1].text, "加");
        assert_eq!(lines[0][1].style, 1);
        assert_eq!(lines[1][0].text, "粗文字");
        assert_eq!(lines[1][0].style, 1);
    }

    #[test]
    fn no_line_exceeds_width() {
        let samples = [
            "终端里的 Markdown 渲染器 mdr 支持中文、English 混排，以及（括号）和「引号」。",
            "超长的英文单词 pneumonoultramicroscopicsilicovolcanoconiosis 也要能处理",
            "ｆｕｌｌｗｉｄｔｈ letters 和 emoji 😀 混在一起 😀😀😀",
        ];
        for s in samples {
            for width in 2..40 {
                for l in wrap_str(s, width) {
                    assert!(
                        display_width(&l) <= width,
                        "width={width} line={l:?} width {}",
                        display_width(&l)
                    );
                }
            }
        }
    }

    #[test]
    fn min_width_is_longest_word() {
        assert_eq!(min_width(&[text("hello wonderful world")]), 9);
        assert_eq!(min_width(&[text("全是中文")]), 2);
        assert_eq!(min_width(&[text("中文，")]), 4);
        assert_eq!(min_width::<()>(&[]), 0);
    }

    #[test]
    fn hard_wrap_splits_code() {
        let pieces = vec![Piece {
            text: "let 变量 = 1;".to_string(),
            style: (),
        }];
        let lines = plain(&hard_wrap(&pieces, 6));
        assert_eq!(lines, vec!["let 变", "量 = 1", ";"]);
        assert_eq!(plain(&hard_wrap::<()>(&[], 6)), vec![""]);
    }

    #[test]
    fn hard_wrap_hanging_uses_narrower_rest() {
        let pieces = vec![Piece {
            text: "abcdefghijklmn".to_string(),
            style: (),
        }];
        assert_eq!(
            plain(&hard_wrap_hanging(&pieces, 6, 3)),
            vec!["abcdef", "ghi", "jkl", "mn"]
        );
    }
}
