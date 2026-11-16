//! 对示例文档做快照测试。更新快照：`INSTA_UPDATE=always cargo test`
//! （或安装 cargo-insta 后运行 `cargo insta review`）。

use mdr::{RenderOptions, render, to_plain};
use unicode_width::UnicodeWidthStr;

const SAMPLE: &str = include_str!("../examples/sample.md");

#[test]
fn sample_at_80_columns() {
    insta::assert_snapshot!(to_plain(&render(SAMPLE, &RenderOptions::plain(80))));
}

#[test]
fn sample_at_40_columns() {
    insta::assert_snapshot!(to_plain(&render(SAMPLE, &RenderOptions::plain(40))));
}

#[test]
/// 4 列表格每格至少要放下一个全角字符，最少需要 21 列，所以从 24 开始测。
fn sample_never_exceeds_width() {
    for width in [24, 33, 40, 57, 80, 120] {
        for line in render(SAMPLE, &RenderOptions::plain(width)) {
            let text = line.plain();
            assert!(
                text.width() <= width,
                "width={width}: {text:?} 宽 {}",
                text.width()
            );
        }
    }
}
