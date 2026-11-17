//! mdr: render Markdown in the terminal.
//!
//! The pipeline has three independent, separately testable stages:
//!
//! 1. [`parse`]: pulldown-cmark event stream → block tree;
//! 2. [`render`]: block tree → styled text lines laid out to a width (wrapping lives in [`wrap`]);
//! 3. [`output`]: text lines → a string with ANSI / OSC 8 / kitty graphics escapes.
//!
//! [`term`] detects modern terminals such as Ghostty and enables richer styling ([`theme::Rich`]) and inline images ([`graphics`]).

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
