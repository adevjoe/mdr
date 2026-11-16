# mdr

在终端里渲染 Markdown 的命令行工具，用 Rust 编写。对中文友好：全角字符按 2 列宽计算，换行和表格都不会错位。

```
mdr README.md
cat notes.md | mdr
```

## 安装

需要 Rust 工具链（1.88 或更新版本）。

```sh
git clone <仓库地址> mdr   # 或者直接进入已有的源码目录
cd mdr
cargo install --path .
```

`cargo install` 会把 `mdr` 装到 `~/.cargo/bin`，请确认这个目录在 `PATH` 里。

不想安装的话，也可以只构建、直接运行：

```sh
cargo build --release
./target/release/mdr examples/sample.md
```

卸载：`cargo uninstall mdr`。

## 使用

```sh
mdr 文件.md              # 渲染文件
cat 文件.md | mdr        # 从标准输入读取
mdr - < 文件.md          # 同上，"-" 表示标准输入
mdr --width 60 文件.md   # 指定排版宽度
mdr --no-color 文件.md   # 不要颜色
mdr --no-pager 文件.md   # 内容很长也直接输出，不进分页器
```

想看看所有元素的效果，可以运行：

```sh
mdr examples/sample.md
```

### 参数

| 参数 | 说明 |
|------|------|
| `FILE` | 要渲染的文件。省略或为 `-` 时从标准输入读取 |
| `-w, --width <N>` | 排版宽度（至少 10 列）。默认取终端宽度，取不到时用 `$COLUMNS`，再不行用 80 |
| `--no-color` | 不输出颜色和样式，等同于 `--color never` |
| `--color <auto\|always\|never>` | 何时输出颜色，默认 `auto` |
| `--hyperlinks <auto\|always\|never>` | 何时把链接做成可点击的终端超链接，默认 `auto` |
| `--no-pager` | 不使用分页器 |
| `-h, --help` / `-V, --version` | 帮助 / 版本 |

### 颜色

默认（`--color auto`）只在输出到终端时使用颜色。下面几种情况会自动去掉颜色，输出纯文本：

- 输出被管道或重定向接走，例如 `mdr a.md > a.txt`、`mdr a.md | grep 标题`；
- 设置了 `NO_COLOR` 环境变量（值非空），参见 [no-color.org](https://no-color.org/)；
- 使用了 `--no-color` 或 `--color never`。

命令行参数优先于环境变量：`--color always` 即使设置了 `NO_COLOR`、即使输出到管道也会带颜色，
适合 `mdr --color always a.md | less -R` 这样的用法。

没有颜色时，行内代码会用反引号 `` ` `` 标出，以便和正文区分。

### 链接

终端支持 [OSC 8 超链接](https://gist.github.com/egmontkob/eb114294efbcd5adb1944c9f3cb5feda) 时，链接文字可以直接点击（Ghostty、iTerm2、WezTerm、kitty、VS Code 终端、Windows Terminal 等）；
否则在链接文字后面用 `<网址>` 显示地址。终端是否支持靠环境变量判断，判断不准时可以用 `--hyperlinks always` 或 `--hyperlinks never` 指定。

图片在终端里显示为 `[图片: 替代文字]`，并附上图片地址（或做成可点击的链接）。

### 分页器

输出到终端且内容超过一屏时，mdr 会把结果交给 `less -R` 显示，按 `q` 退出。

- 用 `--no-pager` 关闭；
- 用 `MDR_PAGER` 环境变量换成别的分页器，例如 `MDR_PAGER="less -RS"`；
- 分页器启动失败时直接输出到终端。

## 支持的元素

- 标题（六级，样式各不相同；一、二级带下划线，三级以下显示 `###` 前缀）
- 粗体、斜体、删除线、行内代码，以及它们的嵌套
- 代码块：按语言语法高亮（基于 syntect，支持 Rust、Python、JavaScript/TypeScript、Go、Shell、JSON、YAML 等常见语言），过长的行会折行
- 有序、无序、嵌套列表，任务列表（`- [x]` / `- [ ]`）
- 引用块（可嵌套）和 GitHub 风格的提示块（`> [!NOTE]`、`> [!WARNING]` 等）
- 表格：按显示宽度对齐，支持左/中/右对齐；太宽时单元格内自动换行
- 分隔线、链接、图片、HTML 块（原样显示）

### 换行规则

- 按终端宽度自动换行，中文等全角字符算 2 列；
- 英文单词不会被拆开（除非一个单词比整行还长）；
- 简单的避头尾：`，。！？）」` 等不会出现在行首，`（「《` 等不会出现在行尾；
- 源文件里中文之间的换行不会变成多余的空格。

## 开发

```sh
cargo build            # 构建
cargo test             # 单元测试 + 快照测试
cargo clippy           # 静态检查
```

代码结构：解析、排版和终端输出相互独立，排版结果是「带样式的文字行」，不含转义序列，方便测试。

| 文件 | 作用 |
|------|------|
| `src/parse.rs` | 用 pulldown-cmark 把 Markdown 解析成块级树 |
| `src/wrap.rs` | 按显示宽度换行（宽字符、避头尾、软换行） |
| `src/render.rs` | 排版：标题、列表缩进、引用、代码块、表格列宽等 |
| `src/highlight.rs` | 代码块语法高亮 |
| `src/theme.rs` | 各元素的颜色与样式 |
| `src/output.rs` | 生成 ANSI 样式和 OSC 8 超链接 |
| `src/main.rs` | 命令行参数、颜色判断、终端宽度、分页器 |

快照测试在 `tests/snapshots.rs`，快照文件位于 `tests/snapshots/`。修改排版后如需更新快照：

```sh
INSTA_UPDATE=always cargo test    # 或安装 cargo-insta 后运行 cargo insta review
```

然后用 `git diff tests/snapshots` 检查变化是否符合预期。

## 尚未支持

- 脚注、定义列表、数学公式的专门排版（目前按普通文本或行内代码显示）
- 自定义配色主题；代码高亮固定使用 `base16-ocean.dark`，在浅色背景的终端里个别颜色可能偏淡
- 终端里直接显示图片
