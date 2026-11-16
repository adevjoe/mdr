//! mdr：在终端里渲染 Markdown。
//!
//! 处理流程分三步，彼此独立、可以单独测试：
//!
//! 1. [`parse`]：pulldown-cmark 事件流 → 块级树；
//! 2. [`render`]：块级树 → 按宽度排好的带样式文字行（换行逻辑在 [`wrap`]）；
//! 3. [`output`]：文字行 → 带 ANSI / OSC 8 / kitty 图形转义的字符串。
//!
//! [`term`] 识别 Ghostty 等现代终端，启用更丰富的样式（[`theme::Rich`]）和图片显示（[`graphics`]）。

pub mod graphics;
pub mod highlight;
pub mod output;
pub mod parse;
pub mod render;
pub mod term;
pub mod theme;
pub mod wrap;

pub use graphics::ImageOptions;
pub use render::{Line, RenderOptions, Span, render, to_plain};
pub use theme::Rich;
