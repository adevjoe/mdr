//! 识别终端类型和能力：Ghostty 等现代终端支持更丰富的显示效果。
//!
//! 没有标准的探测方法，主要靠环境变量判断；终端背景色通过 OSC 11 查询。

/// 已知的现代终端。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Ghostty,
    Kitty,
    WezTerm,
    Other,
}

impl Kind {
    /// 是否适合使用增强样式（代码面板、样式化下划线等）。
    pub fn rich(self) -> bool {
        self != Kind::Other
    }

    /// 是否支持 kitty 图形协议（在终端里直接显示图片）。
    pub fn graphics(self) -> bool {
        matches!(self, Kind::Ghostty | Kind::Kitty)
    }
}

/// 根据环境变量识别终端。`var` 返回变量值，未设置时返回空字符串。
///
/// 在 tmux / screen 里时，外层终端的变量会被继承下来，但这些复用器会拦截或改写
/// 大部分转义序列，所以一律按普通终端处理。
pub fn detect(var: impl Fn(&str) -> String) -> Kind {
    if !var("TMUX").is_empty() || !var("STY").is_empty() {
        return Kind::Other;
    }
    let term = var("TERM");
    match var("TERM_PROGRAM").as_str() {
        "ghostty" => return Kind::Ghostty,
        "WezTerm" => return Kind::WezTerm,
        "tmux" | "screen" => return Kind::Other,
        _ => {}
    }
    if term.contains("ghostty") || !var("GHOSTTY_RESOURCES_DIR").is_empty() {
        Kind::Ghostty
    } else if term.contains("kitty") || !var("KITTY_WINDOW_ID").is_empty() {
        Kind::Kitty
    } else if term.contains("wezterm") || !var("WEZTERM_EXECUTABLE").is_empty() {
        Kind::WezTerm
    } else {
        Kind::Other
    }
}

/// 从环境变量识别当前终端。
pub fn detect_env() -> Kind {
    detect(|k| std::env::var(k).unwrap_or_default())
}

/// 解析 OSC 11 回复（`ESC ] 11 ; rgb:RRRR/GGGG/BBBB` + BEL 或 ST），返回 8 位 RGB。
pub fn parse_osc11(reply: &[u8]) -> Option<(u8, u8, u8)> {
    let s = String::from_utf8_lossy(reply);
    let start = s.find("]11;")? + 4;
    let rest = &s[start..];
    let end = rest.find(['\x07', '\x1b']).unwrap_or(rest.len());
    let body = rest[..end].strip_prefix("rgb:")?;
    let mut parts = body.split('/').map(|p| {
        if p.is_empty() || p.len() > 4 {
            return None;
        }
        let v = u32::from_str_radix(p, 16).ok()?;
        let max = (1u32 << (4 * p.len())) - 1;
        Some(((v * 255 + max / 2) / max) as u8)
    });
    let rgb = (parts.next()??, parts.next()??, parts.next()??);
    parts.next().is_none().then_some(rgb)
}

/// 颜色是否偏亮（感知亮度过半）。
pub fn is_light((r, g, b): (u8, u8, u8)) -> bool {
    0.2126 * f64::from(r) + 0.7152 * f64::from(g) + 0.0722 * f64::from(b) > 128.0
}

/// 终端窗口的大小：(列, 行, 每格宽度像素, 每格高度像素)。像素取不到时为 0。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct WindowSize {
    pub cols: u16,
    pub rows: u16,
    pub cell_width: u16,
    pub cell_height: u16,
}

#[cfg(unix)]
mod sys {
    use std::fs::File;
    use std::io::{Read, Write};
    use std::os::fd::AsRawFd;
    use std::time::{Duration, Instant};

    use super::WindowSize;

    fn tty() -> Option<File> {
        File::options().read(true).write(true).open("/dev/tty").ok()
    }

    pub fn window_size() -> Option<WindowSize> {
        let tty = tty()?;
        // SAFETY: winsize 是纯数据结构，ioctl 只往里面写数据。
        let mut ws: libc::winsize = unsafe { std::mem::zeroed() };
        if unsafe { libc::ioctl(tty.as_raw_fd(), libc::TIOCGWINSZ, &mut ws) } != 0 || ws.ws_col == 0
        {
            return None;
        }
        let cell = |px: u16, n: u16| px.checked_div(n).unwrap_or(0);
        Some(WindowSize {
            cols: ws.ws_col,
            rows: ws.ws_row,
            cell_width: cell(ws.ws_xpixel, ws.ws_col),
            cell_height: cell(ws.ws_ypixel, ws.ws_row),
        })
    }

    /// 向终端发 OSC 11 查询背景色，再发 DA1 作为结束标记：所有终端都会回复 DA1，
    /// 所以收到 DA1 的回复还没有背景色，就说明终端不支持，不用干等到超时。
    pub fn background() -> Option<(u8, u8, u8)> {
        let mut tty = tty()?;
        let fd = tty.as_raw_fd();
        // SAFETY: termios 是纯数据结构，由 tcgetattr 填充。
        let mut orig: libc::termios = unsafe { std::mem::zeroed() };
        if unsafe { libc::tcgetattr(fd, &mut orig) } != 0 {
            return None;
        }
        // 用 VTIME 做读超时（每次最多等 0.1 秒）。macOS 上的 poll() 不支持终端设备。
        let mut raw = orig;
        raw.c_lflag &= !(libc::ICANON | libc::ECHO);
        raw.c_cc[libc::VMIN] = 0;
        raw.c_cc[libc::VTIME] = 1;
        if unsafe { libc::tcsetattr(fd, libc::TCSANOW, &raw) } != 0 {
            return None;
        }

        let mut reply = Vec::new();
        if tty.write_all(b"\x1b]11;?\x1b\\\x1b[c").is_ok() && tty.flush().is_ok() {
            let deadline = Instant::now() + Duration::from_millis(500);
            let mut buf = [0u8; 256];
            while !da1_done(&reply) && Instant::now() < deadline {
                match tty.read(&mut buf) {
                    Ok(n) => reply.extend_from_slice(&buf[..n]),
                    Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
                    Err(_) => break,
                }
            }
        }
        unsafe { libc::tcsetattr(fd, libc::TCSANOW, &orig) };
        super::parse_osc11(&reply)
    }

    /// 是否已收到 DA1 回复（`ESC [ ? … c`）。
    fn da1_done(reply: &[u8]) -> bool {
        let Some(pos) = reply.windows(3).position(|w| w == b"\x1b[?") else {
            return false;
        };
        reply[pos + 3..].contains(&b'c')
    }
}

/// 读取终端窗口大小（含每格像素尺寸）。
pub fn window_size() -> Option<WindowSize> {
    #[cfg(unix)]
    {
        sys::window_size()
    }
    #[cfg(not(unix))]
    {
        None
    }
}

/// 查询终端背景色。终端不回复或不在终端里运行时返回 `None`。
pub fn background() -> Option<(u8, u8, u8)> {
    #[cfg(unix)]
    {
        sys::background()
    }
    #[cfg(not(unix))]
    {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env(vars: &[(&str, &str)]) -> impl Fn(&str) -> String {
        let vars: Vec<(String, String)> = vars
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        move |k| {
            vars.iter()
                .find(|(n, _)| n == k)
                .map(|(_, v)| v.clone())
                .unwrap_or_default()
        }
    }

    #[test]
    fn detects_terminals() {
        assert_eq!(detect(env(&[("TERM_PROGRAM", "ghostty")])), Kind::Ghostty);
        // 通过 ssh 登录时只有 TERM 会传过去。
        assert_eq!(detect(env(&[("TERM", "xterm-ghostty")])), Kind::Ghostty);
        assert_eq!(detect(env(&[("TERM", "xterm-kitty")])), Kind::Kitty);
        assert_eq!(detect(env(&[("TERM_PROGRAM", "WezTerm")])), Kind::WezTerm);
        assert_eq!(detect(env(&[("TERM_PROGRAM", "iTerm.app")])), Kind::Other);
        assert_eq!(detect(env(&[])), Kind::Other);
        // tmux 里继承了 Ghostty 的变量，也不当作 Ghostty。
        assert_eq!(
            detect(env(&[
                ("TMUX", "/tmp/tmux-501/default,1,0"),
                ("GHOSTTY_RESOURCES_DIR", "/Applications/Ghostty.app"),
            ])),
            Kind::Other
        );
        assert!(Kind::Ghostty.graphics() && Kind::Ghostty.rich());
        assert!(!Kind::WezTerm.graphics() && Kind::WezTerm.rich());
    }

    #[test]
    fn parses_background_reply() {
        assert_eq!(
            parse_osc11(b"\x1b]11;rgb:2828/2c2c/3434\x1b\\\x1b[?62;22c"),
            Some((0x28, 0x2c, 0x34))
        );
        assert_eq!(
            parse_osc11(b"\x1b]11;rgb:ff/ff/ff\x07"),
            Some((255, 255, 255))
        );
        assert_eq!(parse_osc11(b"\x1b]11;rgb:f/0/8\x07"), Some((255, 0, 136)));
        assert_eq!(parse_osc11(b"\x1b[?62;22c"), None);
        assert_eq!(parse_osc11(b"\x1b]11;rgb:zz/00/00\x07"), None);
        assert!(is_light((255, 255, 255)));
        assert!(!is_light((0x28, 0x2c, 0x34)));
    }
}
