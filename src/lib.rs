//! mdr：在终端里渲染 Markdown。
//!
//! 处理流程分三步，彼此独立、可以单独测试：
//!
//! 1. [`parse`]：pulldown-cmark 事件流 → 块级树；
//! 2. [`render`]：块级树 → 按宽度排好的带样式文字行（换行逻辑在 [`wrap`]）；
//! 3. [`output`]：文字行 → 带 ANSI / OSC 8 转义的字符串。

pub mod highlight;
pub mod output;
pub mod parse;
pub mod render;
pub mod theme;
pub mod wrap;

pub use render::{Line, RenderOptions, Span, render, to_plain};
