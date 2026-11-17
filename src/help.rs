//! Localized `--help`: the help text follows the system language.
//!
//! The command is defined once (in English, via clap derive). For other languages, [`localize`]
//! rewrites the user-facing strings of that command; argument names, defaults and possible values
//! are read from the command itself, so they can't drift apart.

use std::env;

use clap::{Arg, ArgAction, Command};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lang {
    En,
    Zh,
}

impl Lang {
    /// The system language, following the usual locale conventions:
    ///
    /// 1. the first of `LC_ALL`, `LC_MESSAGES`, `LANG` that is set decides; `C` / `POSIX` mean English;
    /// 2. `LANGUAGE` (a GNU priority list like `zh_CN:en`) refines that choice unless the locale is `C`;
    /// 3. with no locale variables at all (common in macOS terminals), use the macOS language list.
    pub fn detect() -> Lang {
        let var = |k: &str| env::var(k).ok().filter(|v| !v.trim().is_empty());
        let locale = ["LC_ALL", "LC_MESSAGES", "LANG"]
            .iter()
            .find_map(|k| var(k));
        match locale {
            Some(locale) => {
                let base = locale.split(['.', '@']).next().unwrap_or_default();
                if base == "C" || base == "POSIX" {
                    return Lang::En;
                }
                let language = var("LANGUAGE").unwrap_or_default();
                first_supported(language.split(':').chain([locale.as_str()])).unwrap_or(Lang::En)
            }
            None => system_languages()
                .and_then(|langs| first_supported(langs.iter().map(String::as_str)))
                .unwrap_or(Lang::En),
        }
    }

    /// Parse a language tag such as `zh_CN.UTF-8`, `zh-Hans-US` or `en`. `None` if unsupported.
    pub fn from_tag(tag: &str) -> Option<Lang> {
        let primary = tag
            .trim()
            .split(['_', '-', '.', '@'])
            .next()
            .unwrap_or_default()
            .to_ascii_lowercase();
        match primary.as_str() {
            "en" => Some(Lang::En),
            "zh" => Some(Lang::Zh),
            _ => None,
        }
    }
}

fn first_supported<'a>(tags: impl IntoIterator<Item = &'a str>) -> Option<Lang> {
    tags.into_iter().find_map(Lang::from_tag)
}

/// The user's preferred languages from macOS system settings, most preferred first.
#[cfg(target_os = "macos")]
fn system_languages() -> Option<Vec<String>> {
    let out = std::process::Command::new("defaults")
        .args(["read", "-g", "AppleLanguages"])
        .stderr(std::process::Stdio::null())
        .output()
        .ok()
        .filter(|o| o.status.success())?;
    Some(parse_plist_array(&String::from_utf8_lossy(&out.stdout))).filter(|l| !l.is_empty())
}

/// Parse an old-style plist array as printed by `defaults read`: `( "en-US", "zh-Hans-US" )`.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn parse_plist_array(text: &str) -> Vec<String> {
    text.split([',', '(', ')', '\n'])
        .map(|s| s.trim().trim_matches('"').to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

#[cfg(not(target_os = "macos"))]
fn system_languages() -> Option<Vec<String>> {
    None
}

struct Text {
    about: &'static str,
    usage: &'static str,
    arguments: &'static str,
    options: &'static str,
    default: &'static str,
    possible_values: &'static str,
    possible_values_heading: &'static str,
    help_short: &'static str,
    help_long: &'static str,
    version: &'static str,
    after_help: &'static str,
    /// Help for each argument, by argument id.
    args: &'static [(&'static str, &'static str)],
    /// Help for possible values, by (argument id, value).
    values: &'static [(&'static str, &'static str, &'static str)],
}

const ZH: Text = Text {
    about: "在终端里渲染 Markdown，正确处理中日韩文字的折行与表格对齐",
    usage: "用法:",
    arguments: "参数",
    options: "选项",
    default: "默认",
    possible_values: "可选值",
    possible_values_heading: "可选值:",
    help_short: "显示帮助（用 '--help' 查看详细说明）",
    help_long: "显示帮助（用 '-h' 查看摘要）",
    version: "显示版本",
    after_help: "示例:\n  mdr README.md\n  cat notes.md | mdr\n  mdr --width 60 --no-pager doc.md",
    args: &[
        (
            "file",
            "要渲染的 Markdown 文件；省略或为 \"-\" 时从标准输入读取",
        ),
        ("width", "排版宽度（列数，默认取终端宽度）"),
        ("no_color", "不输出颜色和样式（等同于 --color never）"),
        (
            "color",
            "何时使用颜色。auto：输出到终端且未设置 NO_COLOR 时使用",
        ),
        ("hyperlinks", "何时把链接做成可点击的终端超链接（OSC 8）"),
        (
            "style",
            "显示风格。auto：在 Ghostty、kitty 和 WezTerm 中使用增强效果",
        ),
        (
            "images",
            "何时在终端里直接显示本地图片（kitty 图形协议，Ghostty 和 kitty 支持）",
        ),
        ("no_pager", "输出超过一屏时也不使用分页器"),
    ],
    values: &[
        (
            "style",
            "auto",
            "在 Ghostty、kitty 和 WezTerm 中用 rich，其他终端用 basic",
        ),
        (
            "style",
            "rich",
            "增强效果：代码块底色面板、行内代码底色、链接点线下划线",
        ),
        ("style", "basic", "只用文字颜色和边框线，任何终端都适用"),
    ],
};

/// Translate the help text of `cmd` into `lang`. English is the source language and is returned
/// unchanged.
pub fn localize(cmd: Command, lang: Lang) -> Command {
    let text = match lang {
        Lang::En => return cmd,
        Lang::Zh => &ZH,
    };

    let usage = *cmd.get_styles().get_usage();
    let mut cmd = cmd
        .about(text.about)
        .after_help(text.after_help)
        .help_template(format!(
            "{{before-help}}{{about-with-newline}}\n{usage}{}{usage:#} {{usage}}\n\n{{all-args}}{{after-help}}",
            text.usage
        ));

    let ids: Vec<String> = cmd
        .get_arguments()
        .map(|a| a.get_id().to_string())
        .collect();
    for id in ids {
        let Some(&(_, help)) = text.args.iter().find(|(k, _)| *k == id) else {
            continue;
        };
        cmd = cmd.mut_arg(&id, |arg| translate_arg(arg, &id, help, text));
    }

    // The built-in -h / -V only exist once the command is built, so replace them with equivalent
    // flags carrying translated help. Added last so the arguments heading is listed first.
    cmd.disable_help_flag(true)
        .disable_version_flag(true)
        .arg(
            Arg::new("help")
                .short('h')
                .long("help")
                .action(ArgAction::Help)
                .help(text.help_short)
                .long_help(text.help_long)
                .help_heading(text.options),
        )
        .arg(
            Arg::new("version")
                .short('V')
                .long("version")
                .action(ArgAction::Version)
                .help(text.version)
                .help_heading(text.options),
        )
}

fn translate_arg(arg: Arg, id: &str, help: &str, text: &Text) -> Arg {
    let heading = if arg.is_positional() {
        text.arguments
    } else {
        text.options
    };
    let arg = arg.help_heading(heading);
    if !arg.get_action().takes_values() {
        return arg.help(help.to_string());
    }

    // clap prints "[default: ...]" and "[possible values: ...]" in English, so hide those and
    // spell them out here instead, in the same layout clap uses.
    let defaults: Vec<String> = arg
        .get_default_values()
        .iter()
        .map(|v| v.to_string_lossy().into_owned())
        .collect();
    let values = arg.get_possible_values();
    let names: Vec<&str> = values
        .iter()
        .filter(|v| !v.is_hide_set())
        .map(|v| v.get_name())
        .collect();
    let described: Vec<(&str, &str)> = names
        .iter()
        .filter_map(|&name| {
            text.values
                .iter()
                .find(|(a, v, _)| *a == id && *v == name)
                .map(|&(_, _, h)| (name, h))
        })
        .collect();

    let mut short = help.to_string();
    let mut long = help.to_string();
    let mut long_tail = Vec::new();
    if !defaults.is_empty() {
        let d = format!("[{}: {}]", text.default, defaults.join(", "));
        short.push_str(&format!(" {d}"));
        long_tail.push(d);
    }
    if !names.is_empty() {
        let p = format!("[{}: {}]", text.possible_values, names.join(", "));
        short.push_str(&format!(" {p}"));
        if described.is_empty() {
            long_tail.push(p);
        } else {
            let width = names.iter().map(|n| n.len()).max().unwrap_or(0);
            long.push_str(&format!("\n\n{}", text.possible_values_heading));
            for (name, h) in &described {
                let pad = " ".repeat(width - name.len());
                long.push_str(&format!("\n- {name}: {pad}{h}"));
            }
        }
    }
    if !long_tail.is_empty() {
        long.push_str(&format!("\n\n{}", long_tail.join("\n")));
    }
    arg.help(short)
        .long_help(long)
        .hide_default_value(true)
        .hide_possible_values(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn language_tags() {
        assert_eq!(Lang::from_tag("zh_CN.UTF-8"), Some(Lang::Zh));
        assert_eq!(Lang::from_tag("zh-Hans-US"), Some(Lang::Zh));
        assert_eq!(Lang::from_tag("zh_TW"), Some(Lang::Zh));
        assert_eq!(Lang::from_tag("en_US.UTF-8"), Some(Lang::En));
        assert_eq!(Lang::from_tag("fr_FR.UTF-8"), None);
        assert_eq!(Lang::from_tag("C"), None);
        assert_eq!(first_supported(["fr", "zh_CN", "en"]), Some(Lang::Zh));
    }

    #[test]
    fn macos_language_list() {
        let langs = parse_plist_array("(\n    \"zh-Hans-CN\",\n    \"en-US\"\n)\n");
        assert_eq!(langs, ["zh-Hans-CN", "en-US"]);
        let langs = parse_plist_array("(\n    \"fr-FR\",\n    en\n)\n");
        assert_eq!(
            first_supported(langs.iter().map(String::as_str)),
            Some(Lang::En)
        );
    }
}
