# mdr：在终端里读 Markdown

普通文字、**粗体**、*斜体*、~~删除线~~、`行内代码`和[链接](https://github.com/adevjoe/mdr)。
全角字符按 2 列宽计算，中英文混排（例如 Rust、Markdown）也能正确折行，标点不会跑到行首。

- [x] 解析 Markdown
- [x] 宽字符换行
- [ ] 支持脚注

> [!NOTE]
> 支持 GitHub 风格的提示块和嵌套引用。

```python
def greet(name: str) -> str:
    # 中文注释也没问题
    return f"你好，{name}！"
```

| 元素 | 语法 | 是否支持 | 备注 |
|:-----|:----:|:--------:|-----:|
| 标题 | `#` | ✓ | 六级 |
| 表格 | GFM | ✓ | 列宽自动计算 |
| Mixed 中英 | text | ✓ | 1234 |

![一张渐变示意图](demo.png)
