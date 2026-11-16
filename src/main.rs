use std::env;
use std::fs;
use std::io::{self, IsTerminal, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Stdio};

use clap::{Parser, ValueEnum};

use mdr::{ImageOptions, RenderOptions, Rich, output, render, term};

#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
enum When {
    Auto,
    Always,
    Never,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
enum StyleMode {
    /// 在 Ghostty、kitty、WezTerm 里用 rich，其他终端用 basic
    Auto,
    /// 代码块底色面板、行内代码底色、点线链接等增强效果
    Rich,
    /// 只用文字颜色和边框线，适用于所有终端
    Basic,
}

/// 在终端里渲染 Markdown。
#[derive(Parser, Debug)]
#[command(
    name = "mdr",
    version,
    about,
    after_help = "示例:\n  mdr README.md\n  cat notes.md | mdr\n  mdr --width 60 --no-pager doc.md"
)]
struct Cli {
    /// 要渲染的 Markdown 文件；省略或为 "-" 时从标准输入读取
    file: Option<PathBuf>,

    /// 排版宽度（列数），默认取终端宽度
    #[arg(short, long, value_parser = clap::value_parser!(u16).range(10..))]
    width: Option<u16>,

    /// 不输出颜色和样式（等同于 --color never）
    #[arg(long)]
    no_color: bool,

    /// 何时输出颜色。auto：输出到终端且未设置 NO_COLOR 时
    #[arg(long, value_enum, default_value_t = When::Auto, value_name = "WHEN")]
    color: When,

    /// 何时把链接做成可点击的终端超链接（OSC 8）
    #[arg(long, value_enum, default_value_t = When::Auto, value_name = "WHEN")]
    hyperlinks: When,

    /// 显示风格。auto：在 Ghostty、kitty、WezTerm 里自动使用增强效果
    #[arg(long, value_enum, default_value_t = StyleMode::Auto, value_name = "STYLE")]
    style: StyleMode,

    /// 何时在终端里直接显示本地图片（kitty 图形协议，Ghostty 和 kitty 支持）
    #[arg(long, value_enum, default_value_t = When::Auto, value_name = "WHEN")]
    images: When,

    /// 内容超过一屏时也不使用分页器
    #[arg(long)]
    no_pager: bool,
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match run(&cli) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("mdr: {e}");
            ExitCode::FAILURE
        }
    }
}

fn run(cli: &Cli) -> Result<(), String> {
    let source = read_input(cli)?;
    let stdout_tty = io::stdout().is_terminal();

    let color = if cli.no_color {
        false
    } else {
        match cli.color {
            When::Always => true,
            When::Never => false,
            When::Auto => stdout_tty && !env_set("NO_COLOR"),
        }
    };
    let hyperlinks = match cli.hyperlinks {
        When::Always => true,
        When::Never => false,
        When::Auto => color && terminal_supports_hyperlinks(),
    };
    let width = cli
        .width
        .map(usize::from)
        .unwrap_or_else(|| terminal_width(stdout_tty));

    let kind = term::detect_env();
    let rich = color
        && match cli.style {
            StyleMode::Rich => true,
            StyleMode::Basic => false,
            StyleMode::Auto => kind.rich(),
        };
    let window = term::window_size();
    let images = match cli.images {
        When::Always => color,
        When::Never => false,
        When::Auto => color && stdout_tty && kind.graphics(),
    };

    let mut opts = RenderOptions {
        width,
        color,
        hyperlinks,
        // 这几种现代终端都支持真彩色，但通过 ssh 登录时 COLORTERM 不会传过去。
        truecolor: color && (truecolor_supported() || kind.rich()),
        rich: rich.then(|| Rich {
            styled_underline: true,
            // 输出被管道接走时不查询：下游的分页器可能会抢走终端的回复。
            background: if stdout_tty { term::background() } else { None },
        }),
        images: images.then(|| image_options(cli, window)),
    };
    let mut lines = render(&source, &opts);

    let screen_rows = window
        .map(|w| usize::from(w.rows))
        .or_else(|| terminal_size::terminal_size().map(|(_, h)| usize::from(h.0)))
        .filter(|_| stdout_tty && !cli.no_pager);
    if let Some(rows) = screen_rows
        && lines.len() >= rows
    {
        // 分页器显示不了图片。去掉图片后仍然超过一屏才分页；否则是图片把内容撑长了，
        // 直接带图输出。
        let paged = if opts.images.is_some() {
            opts.images = None;
            Some(render(&source, &opts)).filter(|l| l.len() >= rows)
        } else {
            Some(std::mem::take(&mut lines))
        };
        if let Some(paged) = paged {
            if page(&output::to_ansi(&paged, color)) {
                return Ok(());
            }
            lines = paged;
        }
    }

    let mut text = output::to_ansi(&lines, color);
    if stdout_tty && rich {
        text = format!("{}{text}{}", output::SYNC_BEGIN, output::SYNC_END);
    }
    let mut out = io::stdout().lock();
    match out.write_all(text.as_bytes()).and_then(|()| out.flush()) {
        Ok(()) => Ok(()),
        // 例如 `mdr x.md | head`：下游提前关闭不算错误。
        Err(e) if e.kind() == io::ErrorKind::BrokenPipe => Ok(()),
        Err(e) => Err(format!("写入输出失败: {e}")),
    }
}

fn read_input(cli: &Cli) -> Result<String, String> {
    let bytes = match &cli.file {
        Some(path) if path.as_os_str() != "-" => {
            fs::read(path).map_err(|e| format!("无法读取 {}: {e}", path.display()))?
        }
        _ => {
            let mut stdin = io::stdin();
            if stdin.is_terminal() {
                return Err(
                    "没有输入。用法: mdr <文件.md>，或 cat 文件.md | mdr（详见 mdr --help）".into(),
                );
            }
            let mut buf = Vec::new();
            stdin
                .read_to_end(&mut buf)
                .map_err(|e| format!("读取标准输入失败: {e}"))?;
            buf
        }
    };
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

/// 图片显示参数：相对路径以 Markdown 文件所在目录为准；一张图最多占窗口高度的三分之二。
fn image_options(cli: &Cli, window: Option<term::WindowSize>) -> ImageOptions {
    let base_dir = match &cli.file {
        Some(p) if p.as_os_str() != "-" => p.parent().unwrap_or(Path::new(".")).to_path_buf(),
        _ => PathBuf::from("."),
    };
    let w = window.unwrap_or_default();
    // 终端不报告像素尺寸时，按常见的 1:2 格子估算。
    let cell = if w.cell_width > 0 && w.cell_height > 0 {
        (w.cell_width, w.cell_height)
    } else {
        (10, 20)
    };
    let max_rows = if w.rows > 0 {
        (usize::from(w.rows) * 2 / 3).max(4)
    } else {
        24
    };
    ImageOptions {
        base_dir,
        cell,
        max_rows,
    }
}

fn env_set(name: &str) -> bool {
    env::var_os(name).is_some_and(|v| !v.is_empty())
}

/// 排版宽度：终端宽度 → $COLUMNS → 80。
///
/// 输出被管道接走时（如 `mdr x.md | less -R`）stdout 不是终端，再试试 stderr。
fn terminal_width(stdout_tty: bool) -> usize {
    let size = if stdout_tty {
        terminal_size::terminal_size()
    } else {
        terminal_size::terminal_size_of(io::stderr())
    };
    if let Some((terminal_size::Width(w), _)) = size {
        return usize::from(w);
    }
    env::var("COLUMNS")
        .ok()
        .and_then(|c| c.trim().parse().ok())
        .filter(|&w: &usize| w > 0)
        .unwrap_or(80)
}

fn truecolor_supported() -> bool {
    env::var("COLORTERM").is_ok_and(|v| v == "truecolor" || v == "24bit")
}

/// 根据环境变量猜测终端是否支持 OSC 8 超链接。没有标准的探测方法，只认已知支持的终端。
fn terminal_supports_hyperlinks() -> bool {
    let var = |k: &str| env::var(k).unwrap_or_default();
    if matches!(
        var("TERM_PROGRAM").as_str(),
        "iTerm.app" | "WezTerm" | "vscode" | "ghostty" | "Hyper" | "Tabby" | "rio" | "WarpTerminal"
    ) {
        return true;
    }
    let term = var("TERM");
    if [
        "kitty",
        "ghostty",
        "wezterm",
        "alacritty",
        "foot",
        "contour",
    ]
    .iter()
    .any(|t| term.contains(t))
    {
        return true;
    }
    if [
        "KITTY_WINDOW_ID",
        "WT_SESSION",
        "KONSOLE_VERSION",
        "GHOSTTY_RESOURCES_DIR",
    ]
    .iter()
    .any(|k| env_set(k))
    {
        return true;
    }
    var("VTE_VERSION").parse::<u32>().is_ok_and(|v| v >= 5000)
}

/// 交给分页器显示。默认 `less -R`，可用 `MDR_PAGER` 覆盖。启动失败返回 false。
fn page(text: &str) -> bool {
    let pager = env::var("MDR_PAGER")
        .ok()
        .filter(|p| !p.trim().is_empty())
        .unwrap_or_else(|| "less -R".to_string());
    let mut parts = pager.split_whitespace();
    let Some(program) = parts.next() else {
        return false;
    };
    let Ok(mut child) = Command::new(program)
        .args(parts)
        .stdin(Stdio::piped())
        .spawn()
    else {
        return false;
    };
    if let Some(mut stdin) = child.stdin.take() {
        // 用户在分页器里提前退出会导致 BrokenPipe，忽略即可。
        let _ = stdin.write_all(text.as_bytes());
    }
    let _ = child.wait();
    true
}
