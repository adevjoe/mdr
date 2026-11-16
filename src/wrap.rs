//! 按「显示宽度」换行。
//!
//! 中文、日文等全角字符在终端里占 2 列，`unicode-width` 负责算宽度；
//! 这里负责决定在哪里断行：
//!
//! - 拉丁文按空格断词，单词本身不拆开（除非比整行还宽）；
//! - 全角字符之间任意位置都可以断行；
//! - 简单的避头尾：`，。）」` 这类标点不放在行首，`（「《` 这类不放在行尾；
//! - 源文件里中文之间的软换行不产生多余空格。
//!
//! 换行器对样式类型 `S` 是泛型的，所以不依赖终端输出，方便单元测试。

use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

/// 字符串在终端中占用的列数。
pub fn display_width(s: &str) -> usize {
    UnicodeWidthStr::width(s)
}

/// 一段带样式的文本。
#[derive(Clone, Debug, PartialEq)]
pub struct Piece<S> {
    pub text: String,
    pub style: S,
}

/// 换行器的输入。
#[derive(Clone, Debug, PartialEq)]
pub enum Item<S> {
    Text(String, S),
    /// Markdown 的软换行：通常相当于一个空格，中文之间则什么也不是。
    SoftBreak,
    /// 强制换行。
    HardBreak,
}

/// 换行后的一行。
pub type WrappedLine<S> = Vec<Piece<S>>;

/// 一行的显示宽度。
pub fn line_width<S>(line: &[Piece<S>]) -> usize {
    line.iter().map(|p| display_width(&p.text)).sum()
}

/// 不能出现在行首的标点。
const CLOSING: &[&str] = &[
    "，", "。", "、", "；", "：", "？", "！", "）", "」", "』", "》", "〉", "】", "〕", "］", "｝",
    "”", "’", "…", "～", ",", ".", ";", ":", "!", "?", ")", "]", "}", "%",
];

/// 不能出现在行尾的标点。
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
    // 不换行空格（NBSP）不作为断点。
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
    /// 不可再分的一段（拉丁单词、单个汉字连同粘着的标点……）及其宽度。
    /// 相邻的两个 Word 之间可以断行。
    Word(Vec<Piece<S>>, usize),
    /// 空格：可以断行，断行时丢弃。
    Space(S),
    Hard,
}

struct Tokenizer<S> {
    toks: Vec<Tok<S>>,
    cur: Vec<Piece<S>>,
    cur_w: usize,
    /// 刚遇到一个开括号类标点，下一个字符要粘在一起。
    glue: bool,
    /// 尚未决定是否变成空格的软换行。
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

        // 软换行：两边都是全角字符时直接相连，否则当成空格。
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

/// 把带样式的行内内容按 `width` 列换行。
///
/// 返回的每一行宽度都不超过 `width`（`width` 至少按 2 算，保证能放下一个全角字符）。
/// 内容为空时返回空列表。
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
                    // 单个「词」比整行还宽：按字形簇硬拆。
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

/// 最长的不可断开片段（英文单词等）的宽度，即不硬拆单词时能缩到的最小宽度。
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

/// 不考虑单词边界，严格按宽度切分（用于代码块）。至少返回一行。
pub fn hard_wrap<S: Clone + PartialEq>(pieces: &[Piece<S>], width: usize) -> Vec<WrappedLine<S>> {
    let width = width.max(2);
    let mut lines = Vec::new();
    let mut line: WrappedLine<S> = Vec::new();
    let mut w = 0usize;
    for p in pieces {
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
            assert!(display_width(l) <= 12, "{l:?} 超宽");
        }
        assert_eq!(
            lines.concat().replace(' ', ""),
            "Rust是一门注重安全和性能的系统编程语言"
        );
        assert_eq!(lines[0], "Rust 是一门");
    }

    #[test]
    fn closing_punctuation_never_starts_a_line() {
        // 宽度 6 时「三」后的「，」本来会落到行首，应当和「三」一起换行。
        let lines = wrap_str("一二三，四五六。七八九", 6);
        assert_eq!(lines, vec!["一二", "三，四", "五六。", "七八九"]);
        for l in &lines {
            assert!(!l.starts_with('，') && !l.starts_with('。'));
        }
    }

    #[test]
    fn opening_punctuation_never_ends_a_line() {
        // 不处理的话会得到「一二（」+「三四」。
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
                        "width={width} line={l:?} 宽度 {}",
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
}
