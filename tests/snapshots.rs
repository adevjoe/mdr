//! Snapshot tests for the example documents. To update snapshots: `INSTA_UPDATE=always cargo test`
//! (or install cargo-insta and run `cargo insta review`).

use mdr::{RenderOptions, render, to_plain};
use unicode_width::UnicodeWidthStr;

const SAMPLE: &str = include_str!("../examples/sample.md");
/// The Chinese sample exercises full-width wrapping, kinsoku and CJK tables throughout.
const SAMPLE_ZH: &str = include_str!("../examples/sample.zh-CN.md");

#[test]
fn sample_at_80_columns() {
    insta::assert_snapshot!(to_plain(&render(SAMPLE, &RenderOptions::plain(80))));
}

#[test]
fn sample_at_40_columns() {
    insta::assert_snapshot!(to_plain(&render(SAMPLE, &RenderOptions::plain(40))));
}

#[test]
fn sample_zh_at_80_columns() {
    insta::assert_snapshot!(to_plain(&render(SAMPLE_ZH, &RenderOptions::plain(80))));
}

#[test]
fn sample_zh_at_40_columns() {
    insta::assert_snapshot!(to_plain(&render(SAMPLE_ZH, &RenderOptions::plain(40))));
}

#[test]
/// A 4-column table needs at least 21 columns so every cell fits one full-width character, so
/// testing starts at 24.
fn samples_never_exceed_width() {
    for sample in [SAMPLE, SAMPLE_ZH] {
        for width in [24, 33, 40, 57, 80, 120] {
            for line in render(sample, &RenderOptions::plain(width)) {
                let text = line.plain();
                assert!(
                    text.width() <= width,
                    "width={width}: {text:?} width {}",
                    text.width()
                );
            }
        }
    }
}
