//! 用 pulldown-cmark 把 Markdown 解析成一棵简单的块级树。
//!
//! pulldown-cmark 输出的是事件流，直接边读边排版会让嵌套容器（列表套引用套代码块）
//! 很难处理宽度。先建树，再由 [`crate::render`] 递归排版，每一层只需关心自己的缩进。

use pulldown_cmark::{
    Alignment, BlockQuoteKind, CodeBlockKind, Event, HeadingLevel, Options, Parser, Tag, TagEnd,
};

/// 行内样式标记。渲染时由主题决定具体颜色。
#[derive(Clone, Debug, Default, PartialEq)]
pub struct InlineStyle {
    pub bold: bool,
    pub italic: bool,
    pub strike: bool,
    pub code: bool,
    /// 图片：文本是 alt，`link` 是图片地址。
    pub image: bool,
    pub link: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum InlineKind {
    Text(String),
    SoftBreak,
    HardBreak,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Inline {
    pub kind: InlineKind,
    pub style: InlineStyle,
}

pub type Inlines = Vec<Inline>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Align {
    None,
    Left,
    Center,
    Right,
}

/// GitHub 风格的提示块（`> [!NOTE]` 等）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AlertKind {
    Note,
    Tip,
    Important,
    Warning,
    Caution,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ListItem {
    /// `Some(true)` 已完成，`Some(false)` 未完成，`None` 不是任务项。
    pub task: Option<bool>,
    pub blocks: Vec<Block>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Block {
    Heading {
        level: u8,
        content: Inlines,
    },
    Paragraph(Inlines),
    CodeBlock {
        lang: Option<String>,
        code: String,
    },
    List {
        /// 有序列表的起始编号；无序列表为 `None`。
        start: Option<u64>,
        /// 紧凑列表（项之间没有空行）。
        tight: bool,
        items: Vec<ListItem>,
    },
    Quote {
        kind: Option<AlertKind>,
        blocks: Vec<Block>,
    },
    Table {
        aligns: Vec<Align>,
        head: Vec<Inlines>,
        rows: Vec<Vec<Inlines>>,
    },
    Rule,
    Html(String),
}

/// 解析 Markdown 文本。
pub fn parse(src: &str) -> Vec<Block> {
    let opts = Options::ENABLE_TABLES
        | Options::ENABLE_STRIKETHROUGH
        | Options::ENABLE_TASKLISTS
        | Options::ENABLE_GFM;
    let mut b = Builder::default();
    for ev in Parser::new_ext(src, opts) {
        b.event(ev);
    }
    b.finish()
}

enum Frame {
    Root(Vec<Block>),
    Quote {
        kind: Option<AlertKind>,
        blocks: Vec<Block>,
    },
    List {
        start: Option<u64>,
        tight: bool,
        items: Vec<ListItem>,
    },
    Item {
        task: Option<bool>,
        blocks: Vec<Block>,
        /// 紧凑列表项里没有 Paragraph 事件，行内内容先攒在这里。
        inline: Inlines,
    },
    Paragraph(Inlines),
    Heading(u8, Inlines),
    Code {
        lang: Option<String>,
        text: String,
    },
    Html(String),
    Table {
        aligns: Vec<Align>,
        head: Vec<Inlines>,
        rows: Vec<Vec<Inlines>>,
        row: Vec<Inlines>,
    },
    Cell(Inlines),
}

struct Builder {
    stack: Vec<Frame>,
    bold: u32,
    italic: u32,
    strike: u32,
    links: Vec<String>,
    /// 正在读取的图片：(地址, alt 文本)。
    image: Option<(String, String)>,
}

impl Default for Builder {
    fn default() -> Self {
        Builder {
            stack: vec![Frame::Root(Vec::new())],
            bold: 0,
            italic: 0,
            strike: 0,
            links: Vec::new(),
            image: None,
        }
    }
}

fn take_item_inline(blocks: &mut Vec<Block>, inline: &mut Inlines) {
    if !inline.is_empty() {
        blocks.push(Block::Paragraph(std::mem::take(inline)));
    }
}

impl Builder {
    fn style(&self) -> InlineStyle {
        InlineStyle {
            bold: self.bold > 0,
            italic: self.italic > 0,
            strike: self.strike > 0,
            code: false,
            image: false,
            link: self.links.last().cloned(),
        }
    }

    fn push_inline(&mut self, kind: InlineKind, style: InlineStyle) {
        let inline = Inline { kind, style };
        if let Some(
            Frame::Paragraph(v)
            | Frame::Heading(_, v)
            | Frame::Cell(v)
            | Frame::Item { inline: v, .. },
        ) = self.stack.last_mut()
        {
            v.push(inline);
        }
    }

    fn push_text(&mut self, text: &str, style: InlineStyle) {
        self.push_inline(InlineKind::Text(text.to_string()), style);
    }

    fn push_block(&mut self, block: Block) {
        match self.stack.last_mut() {
            Some(Frame::Root(v) | Frame::Quote { blocks: v, .. }) => v.push(block),
            Some(Frame::Item { blocks, inline, .. }) => {
                take_item_inline(blocks, inline);
                blocks.push(block);
            }
            _ => {}
        }
    }

    fn event(&mut self, ev: Event) {
        match ev {
            Event::Start(tag) => self.start(tag),
            Event::End(tag) => self.end(tag),
            Event::Text(t) => {
                if let Some((_, alt)) = &mut self.image {
                    alt.push_str(&t);
                    return;
                }
                match self.stack.last_mut() {
                    Some(Frame::Code { text, .. } | Frame::Html(text)) => text.push_str(&t),
                    _ => self.push_text(&t, self.style()),
                }
            }
            Event::Code(t) => {
                if let Some((_, alt)) = &mut self.image {
                    alt.push_str(&t);
                    return;
                }
                let mut style = self.style();
                style.code = true;
                self.push_text(&t, style);
            }
            Event::Html(t) => {
                if let Some(Frame::Html(text)) = self.stack.last_mut() {
                    text.push_str(&t);
                } else {
                    self.push_block(Block::Html(t.to_string()));
                }
            }
            Event::InlineHtml(t) => self.push_text(&t, self.style()),
            Event::SoftBreak => {
                if let Some((_, alt)) = &mut self.image {
                    alt.push(' ');
                } else {
                    self.push_inline(InlineKind::SoftBreak, self.style());
                }
            }
            Event::HardBreak => {
                if let Some((_, alt)) = &mut self.image {
                    alt.push(' ');
                } else {
                    self.push_inline(InlineKind::HardBreak, self.style());
                }
            }
            Event::Rule => self.push_block(Block::Rule),
            Event::TaskListMarker(done) => {
                for frame in self.stack.iter_mut().rev() {
                    if let Frame::Item { task, .. } = frame {
                        *task = Some(done);
                        break;
                    }
                }
            }
            Event::FootnoteReference(name) => self.push_text(&format!("[^{name}]"), self.style()),
            Event::InlineMath(t) | Event::DisplayMath(t) => {
                let mut style = self.style();
                style.code = true;
                self.push_text(&t, style);
            }
        }
    }

    fn start(&mut self, tag: Tag) {
        match tag {
            Tag::Paragraph => {
                // 列表项里出现 Paragraph 说明是松散列表。
                let n = self.stack.len();
                if n >= 2
                    && matches!(self.stack[n - 1], Frame::Item { .. })
                    && let Frame::List { tight, .. } = &mut self.stack[n - 2]
                {
                    *tight = false;
                }
                self.stack.push(Frame::Paragraph(Vec::new()));
            }
            Tag::Heading { level, .. } => {
                let level = match level {
                    HeadingLevel::H1 => 1,
                    HeadingLevel::H2 => 2,
                    HeadingLevel::H3 => 3,
                    HeadingLevel::H4 => 4,
                    HeadingLevel::H5 => 5,
                    HeadingLevel::H6 => 6,
                };
                self.stack.push(Frame::Heading(level, Vec::new()));
            }
            Tag::BlockQuote(kind) => {
                let kind = kind.map(|k| match k {
                    BlockQuoteKind::Note => AlertKind::Note,
                    BlockQuoteKind::Tip => AlertKind::Tip,
                    BlockQuoteKind::Important => AlertKind::Important,
                    BlockQuoteKind::Warning => AlertKind::Warning,
                    BlockQuoteKind::Caution => AlertKind::Caution,
                });
                self.flush_item_inline();
                self.stack.push(Frame::Quote {
                    kind,
                    blocks: Vec::new(),
                });
            }
            Tag::CodeBlock(kind) => {
                let lang = match kind {
                    CodeBlockKind::Fenced(info) => info
                        .split(|c: char| c.is_whitespace() || c == ',' || c == '{')
                        .next()
                        .filter(|s| !s.is_empty())
                        .map(str::to_string),
                    CodeBlockKind::Indented => None,
                };
                self.flush_item_inline();
                self.stack.push(Frame::Code {
                    lang,
                    text: String::new(),
                });
            }
            Tag::HtmlBlock => {
                self.flush_item_inline();
                self.stack.push(Frame::Html(String::new()));
            }
            Tag::List(start) => {
                self.flush_item_inline();
                self.stack.push(Frame::List {
                    start,
                    tight: true,
                    items: Vec::new(),
                });
            }
            Tag::Item => self.stack.push(Frame::Item {
                task: None,
                blocks: Vec::new(),
                inline: Vec::new(),
            }),
            Tag::Table(aligns) => {
                let aligns = aligns
                    .into_iter()
                    .map(|a| match a {
                        Alignment::None => Align::None,
                        Alignment::Left => Align::Left,
                        Alignment::Center => Align::Center,
                        Alignment::Right => Align::Right,
                    })
                    .collect();
                self.flush_item_inline();
                self.stack.push(Frame::Table {
                    aligns,
                    head: Vec::new(),
                    rows: Vec::new(),
                    row: Vec::new(),
                });
            }
            Tag::TableHead | Tag::TableRow => {}
            Tag::TableCell => self.stack.push(Frame::Cell(Vec::new())),
            Tag::Emphasis => self.italic += 1,
            Tag::Strong => self.bold += 1,
            Tag::Strikethrough => self.strike += 1,
            Tag::Link { dest_url, .. } => self.links.push(dest_url.to_string()),
            Tag::Image { dest_url, .. } => self.image = Some((dest_url.to_string(), String::new())),
            _ => {}
        }
    }

    fn flush_item_inline(&mut self) {
        if let Some(Frame::Item { blocks, inline, .. }) = self.stack.last_mut() {
            take_item_inline(blocks, inline);
        }
    }

    fn end(&mut self, tag: TagEnd) {
        match tag {
            TagEnd::Paragraph => {
                if let Some(Frame::Paragraph(v)) = self.stack.pop() {
                    self.push_block(Block::Paragraph(v));
                }
            }
            TagEnd::Heading(_) => {
                if let Some(Frame::Heading(level, content)) = self.stack.pop() {
                    self.push_block(Block::Heading { level, content });
                }
            }
            TagEnd::BlockQuote(_) => {
                if let Some(Frame::Quote { kind, blocks }) = self.stack.pop() {
                    self.push_block(Block::Quote { kind, blocks });
                }
            }
            TagEnd::CodeBlock => {
                if let Some(Frame::Code { lang, text }) = self.stack.pop() {
                    self.push_block(Block::CodeBlock { lang, code: text });
                }
            }
            TagEnd::HtmlBlock => {
                if let Some(Frame::Html(text)) = self.stack.pop() {
                    self.push_block(Block::Html(text));
                }
            }
            TagEnd::List(_) => {
                if let Some(Frame::List {
                    start,
                    tight,
                    items,
                }) = self.stack.pop()
                {
                    self.push_block(Block::List {
                        start,
                        tight,
                        items,
                    });
                }
            }
            TagEnd::Item => {
                if let Some(Frame::Item {
                    task,
                    mut blocks,
                    mut inline,
                }) = self.stack.pop()
                {
                    take_item_inline(&mut blocks, &mut inline);
                    if let Some(Frame::List { items, .. }) = self.stack.last_mut() {
                        items.push(ListItem { task, blocks });
                    }
                }
            }
            TagEnd::TableCell => {
                if let Some(Frame::Cell(v)) = self.stack.pop()
                    && let Some(Frame::Table { row, .. }) = self.stack.last_mut()
                {
                    row.push(v);
                }
            }
            TagEnd::TableHead => {
                if let Some(Frame::Table { head, row, .. }) = self.stack.last_mut() {
                    *head = std::mem::take(row);
                }
            }
            TagEnd::TableRow => {
                if let Some(Frame::Table { rows, row, .. }) = self.stack.last_mut() {
                    rows.push(std::mem::take(row));
                }
            }
            TagEnd::Table => {
                if let Some(Frame::Table {
                    aligns, head, rows, ..
                }) = self.stack.pop()
                {
                    self.push_block(Block::Table { aligns, head, rows });
                }
            }
            TagEnd::Emphasis => self.italic = self.italic.saturating_sub(1),
            TagEnd::Strong => self.bold = self.bold.saturating_sub(1),
            TagEnd::Strikethrough => self.strike = self.strike.saturating_sub(1),
            TagEnd::Link => {
                self.links.pop();
            }
            TagEnd::Image => {
                if let Some((url, alt)) = self.image.take() {
                    let mut style = self.style();
                    style.image = true;
                    style.link = Some(url);
                    self.push_text(&alt, style);
                }
            }
            _ => {}
        }
    }

    fn finish(mut self) -> Vec<Block> {
        match self.stack.drain(..).next() {
            Some(Frame::Root(v)) => v,
            _ => Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn texts(inl: &Inlines) -> String {
        inl.iter()
            .map(|i| match &i.kind {
                InlineKind::Text(t) => t.as_str(),
                _ => " ",
            })
            .collect()
    }

    #[test]
    fn tight_and_loose_lists() {
        let blocks = parse("- a\n- b\n");
        assert!(matches!(&blocks[0], Block::List { tight: true, items, .. } if items.len() == 2));
        let blocks = parse("- a\n\n- b\n");
        assert!(matches!(&blocks[0], Block::List { tight: false, .. }));
    }

    #[test]
    fn task_items_and_nesting() {
        let blocks = parse("- [x] 完成\n- [ ] 未完成\n  - 子项\n");
        let Block::List { items, .. } = &blocks[0] else {
            panic!()
        };
        assert_eq!(items[0].task, Some(true));
        assert_eq!(items[1].task, Some(false));
        assert!(matches!(&items[1].blocks[0], Block::Paragraph(p) if texts(p) == "未完成"));
        assert!(matches!(&items[1].blocks[1], Block::List { .. }));
    }

    #[test]
    fn table_structure() {
        let blocks = parse("| 名称 | 值 |\n|:--|--:|\n| 甲 | 1 |\n| 乙 | 2 |\n");
        let Block::Table { aligns, head, rows } = &blocks[0] else {
            panic!()
        };
        assert_eq!(aligns, &[Align::Left, Align::Right]);
        assert_eq!(texts(&head[0]), "名称");
        assert_eq!(rows.len(), 2);
        assert_eq!(texts(&rows[1][0]), "乙");
    }

    #[test]
    fn inline_styles_and_images() {
        let blocks = parse("**粗 _斜_** `code` [链接](https://a.b) ![图](x.png)");
        let Block::Paragraph(p) = &blocks[0] else {
            panic!()
        };
        assert!(p[0].style.bold && !p[0].style.italic);
        assert!(p[1].style.bold && p[1].style.italic);
        assert!(p.iter().any(|i| i.style.code));
        assert!(
            p.iter()
                .any(|i| i.style.link.as_deref() == Some("https://a.b") && !i.style.image)
        );
        let img = p.iter().find(|i| i.style.image).unwrap();
        assert_eq!(img.kind, InlineKind::Text("图".into()));
        assert_eq!(img.style.link.as_deref(), Some("x.png"));
    }

    #[test]
    fn code_block_language() {
        let blocks = parse("```rust title=x\nfn main() {}\n```\n");
        assert_eq!(
            blocks[0],
            Block::CodeBlock {
                lang: Some("rust".into()),
                code: "fn main() {}\n".into()
            }
        );
    }
}
