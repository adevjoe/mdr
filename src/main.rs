use std::env;
use std::fs;
use std::io::{self, IsTerminal, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Stdio};

use clap::error::ErrorKind;
use clap::{CommandFactory, FromArgMatches, Parser, ValueEnum};

use mdr::{ImageOptions, RenderOptions, Rich, output, render, term};

mod help;

#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
enum When {
    Auto,
    Always,
    Never,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
enum StyleMode {
    /// Use rich in Ghostty, kitty and WezTerm, basic elsewhere
    Auto,
    /// Enhanced effects: shaded code block panels, inline code background, dotted link underlines
    Rich,
    /// Text colors and box lines only; works in any terminal
    Basic,
}

/// Render Markdown in the terminal.
#[derive(Parser, Debug)]
#[command(
    name = "mdr",
    version,
    about,
    after_help = "Examples:\n  mdr README.md\n  cat notes.md | mdr\n  mdr --width 60 --no-pager doc.md"
)]
struct Cli {
    /// Markdown file to render; reads stdin if omitted or "-"
    file: Option<PathBuf>,

    /// Layout width in columns (default: terminal width)
    #[arg(short, long, value_parser = clap::value_parser!(u16).range(10..))]
    width: Option<u16>,

    /// Disable colors and styles (same as --color never)
    #[arg(long)]
    no_color: bool,

    /// When to use colors. auto: when writing to a terminal and NO_COLOR is unset
    #[arg(long, value_enum, default_value_t = When::Auto, value_name = "WHEN")]
    color: When,

    /// When to make links clickable terminal hyperlinks (OSC 8)
    #[arg(long, value_enum, default_value_t = When::Auto, value_name = "WHEN")]
    hyperlinks: When,

    /// Display style. auto: enhanced effects in Ghostty, kitty and WezTerm
    #[arg(long, value_enum, default_value_t = StyleMode::Auto, value_name = "STYLE")]
    style: StyleMode,

    /// When to show local images inline (kitty graphics protocol; supported by Ghostty and kitty)
    #[arg(long, value_enum, default_value_t = When::Auto, value_name = "WHEN")]
    images: When,

    /// Don't use a pager even if output exceeds one screen
    #[arg(long)]
    no_pager: bool,
}

fn main() -> ExitCode {
    let cli = parse_args();
    match run(&cli) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("mdr: {e}");
            ExitCode::FAILURE
        }
    }
}

/// Parse the command line. `--help` is shown in the system language; the language is only looked
/// up when help is requested, so normal runs don't pay for it.
fn parse_args() -> Cli {
    let args: Vec<_> = env::args_os().collect();
    let matches = match Cli::command().try_get_matches_from(&args) {
        Ok(m) => m,
        Err(e) if e.kind() == ErrorKind::DisplayHelp => {
            let cmd = help::localize(Cli::command(), help::Lang::detect());
            match cmd.try_get_matches_from(&args) {
                Ok(m) => m,
                Err(e) => e.exit(),
            }
        }
        Err(e) => e.exit(),
    };
    Cli::from_arg_matches(&matches).unwrap_or_else(|e| e.exit())
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
        // These modern terminals all support truecolor, but COLORTERM isn't passed through over ssh.
        truecolor: color && (truecolor_supported() || kind.rich()),
        rich: rich.then(|| Rich {
            styled_underline: true,
            // Don't query when output is piped: a downstream pager may steal the terminal's reply.
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
        // Pagers can't show images. Only page if the output still exceeds one screen without
        // images; otherwise the images made it long, so print directly with images.
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
        // e.g. `mdr x.md | head`: the reader closing early isn't an error.
        Err(e) if e.kind() == io::ErrorKind::BrokenPipe => Ok(()),
        Err(e) => Err(format!("failed to write output: {e}")),
    }
}

fn read_input(cli: &Cli) -> Result<String, String> {
    let bytes = match &cli.file {
        Some(path) if path.as_os_str() != "-" => {
            fs::read(path).map_err(|e| format!("cannot read {}: {e}", path.display()))?
        }
        _ => {
            let mut stdin = io::stdin();
            if stdin.is_terminal() {
                return Err(
                    "no input. Usage: mdr <file.md>, or cat file.md | mdr (see mdr --help)".into(),
                );
            }
            let mut buf = Vec::new();
            stdin
                .read_to_end(&mut buf)
                .map_err(|e| format!("failed to read stdin: {e}"))?;
            buf
        }
    };
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

/// Image display options: relative paths resolve against the Markdown file's directory; an image
/// takes at most two thirds of the window height.
fn image_options(cli: &Cli, window: Option<term::WindowSize>) -> ImageOptions {
    let base_dir = match &cli.file {
        Some(p) if p.as_os_str() != "-" => p.parent().unwrap_or(Path::new(".")).to_path_buf(),
        _ => PathBuf::from("."),
    };
    let w = window.unwrap_or_default();
    // If the terminal doesn't report pixel sizes, assume a typical 1:2 cell.
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

/// Layout width: terminal width → $COLUMNS → 80.
///
/// When output is piped (e.g. `mdr x.md | less -R`) stdout isn't a terminal, so try stderr too.
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

/// Guess from environment variables whether the terminal supports OSC 8 hyperlinks. There is no
/// standard detection method, so only known terminals are accepted.
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

/// Show output in a pager: `less -R` by default, overridable with `MDR_PAGER`. Returns false if it
/// fails to start.
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
        // Quitting the pager early causes BrokenPipe; ignore it.
        let _ = stdin.write_all(text.as_bytes());
    }
    let _ = child.wait();
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chinese_help_is_fully_translated() {
        let mut english = Cli::command();
        english.build();
        let mut cmd = help::localize(Cli::command(), help::Lang::Zh);
        cmd.build();
        for arg in cmd.get_arguments() {
            let en = english
                .get_arguments()
                .find(|a| a.get_id() == arg.get_id())
                .unwrap();
            assert_ne!(
                arg.get_help().map(ToString::to_string),
                en.get_help().map(ToString::to_string),
                "--{} has no Chinese help",
                arg.get_id()
            );
        }
        for help in [cmd.render_help(), cmd.render_long_help()] {
            let help = help.to_string();
            for english in [
                "Usage:",
                "Arguments:",
                "Options:",
                "Examples:",
                "default:",
                "ossible values",
                "Print ",
            ] {
                assert!(
                    !help.contains(english),
                    "untranslated {english:?} in:\n{help}"
                );
            }
        }
    }

    #[test]
    fn english_help_is_unchanged() {
        let help = help::localize(Cli::command(), help::Lang::En)
            .render_help()
            .to_string();
        assert!(help.contains("Usage: mdr [OPTIONS] [FILE]"));
        assert!(help.contains("[default: auto]"));
    }
}
