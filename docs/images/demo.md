# mdr: Markdown in your terminal

Plain text, **bold**, *italic*, ~~strikethrough~~, `inline code` and a [link](https://github.com/adevjoe/mdr).
CJK text such as 中文、日本語 counts as two columns, so wrapping and tables stay aligned.

- [x] Parse Markdown
- [x] Wrap wide characters
- [ ] Footnotes

> [!NOTE]
> GitHub-style alerts and nested quotes are supported.

```rust
/// Count how many times each word appears.
fn word_count(text: &str) -> HashMap<&str, usize> {
    let mut map = HashMap::new();
    for word in text.split_whitespace() {
        *map.entry(word).or_insert(0) += 1;
    }
    map
}
```

| Element | Syntax | Supported |
|:--------|:------:|:---------:|
| Heading | `#` | ✓ |
| Table | GFM | ✓ |
| Mixed 中文 | text | ✓ |

![a gradient demo image](demo.png)
