# mdr 示例文档

这份文档覆盖了 mdr 支持的所有 Markdown 元素，可以用 `mdr examples/sample.zh-CN.md` 查看效果。
中文段落在源文件里换行时，渲染结果不会多出空格；中英文混排（例如 Rust、Markdown 和 CommonMark）也能按终端宽度正确折行，标点符号不会跑到行首。

## 行内样式

普通文字、**粗体**、*斜体*、***粗斜体***、~~删除线~~，以及行内代码 `cargo build --release`。
样式可以嵌套：**粗体里有 *斜体* 和 `代码`**，也可以 ~~删除 **加粗的** 内容~~。

这一行末尾有两个空格，表示强制换行；  
所以这句话会从新的一行开始。

## 标题层级

### 三级标题

#### 四级标题

##### 五级标题

###### 六级标题

## 链接与图片

- 普通链接：[Rust 官网](https://www.rust-lang.org/)
- 自动链接：<https://github.com/pulldown-cmark/pulldown-cmark>
- 邮件链接：<mailto:hello@example.com>
- 图片：![mdr 的终端截图](https://example.com/images/mdr-screenshot.png)

单独成段的本地图片在 Ghostty 和 kitty 里会直接显示出来：

![一张渐变示意图](demo.png)

## 列表

无序列表与嵌套：

- 水果
  - 苹果
  - 香蕉
    - 小米蕉
    - 皇帝蕉
- 蔬菜
- 这是一个特别长的列表项，用来演示列表内容超过一行时，续行会和第一行的文字对齐，而不是顶到最左边。

有序列表：

1. 安装 Rust 工具链
2. 克隆仓库并进入目录
3. 运行构建命令
   1. 调试版：`cargo build`
   2. 发布版：`cargo build --release`
4. 运行测试

任务列表：

- [x] 解析 Markdown
- [x] 宽字符换行
- [ ] 支持脚注
- [ ] 支持主题配置

## 引用块

> 引用块里的文字会带有左侧竖线。引用可以包含多个段落，长文字同样按宽度自动换行。
>
> > 引用也可以嵌套，嵌套后每层都有自己的竖线。
>
> - 引用里也能放列表
> - 以及 **样式**

> [!NOTE]
> 这是 GitHub 风格的提示块。

> [!WARNING]
> 注意：不要在没有备份的情况下删除数据。

## 代码块

```rust
use std::collections::HashMap;

/// 统计每个词出现的次数
fn word_count(text: &str) -> HashMap<&str, usize> {
    let mut map = HashMap::new();
    for word in text.split_whitespace() {
        *map.entry(word).or_insert(0) += 1;
    }
    map
}
```

```python
def greet(name: str) -> str:
    # 中文注释也没问题
    return f"你好，{name}！"
```

```bash
cargo install --path .
cat README.md | mdr --width 60
```

```
没有指定语言的代码块不做高亮，原样显示。
```

## 表格

| 元素 | 语法 | 是否支持 | 备注 |
|:-----|:----:|:--------:|-----:|
| 标题 | `#` | ✓ | 六级 |
| 粗体 | `**text**` | ✓ | — |
| 表格 | GFM | ✓ | 列宽自动计算 |
| 脚注 | `[^1]` | ✗ | 计划中 |
| Mixed 中英 | text | ✓ | 1234 |

单元格内容过长时会在格子里自动换行：

| 命令 | 说明 |
|------|------|
| `mdr 文件.md` | 渲染指定的 Markdown 文件，内容超过一屏时自动交给分页器 less 显示 |
| `cat x.md \| mdr` | 从标准输入读取 |
| `mdr --no-color` | 关闭颜色，适合复制纯文本 |

## 分隔线

上面是一段内容。

---

下面是另一段内容。

<details>
<summary>HTML 块会原样显示</summary>
</details>
