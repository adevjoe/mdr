//! Detect the terminal type and capabilities: modern terminals like Ghostty support richer display.
//!
//! There is no standard detection method, so this relies mostly on environment variables; the
//! background color is queried with OSC 11.

/// Known modern terminals.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Ghostty,
    Kitty,
    WezTerm,
    Other,
}

impl Kind {
    /// Whether enhanced styling (code panels, styled underlines, etc.) is appropriate.
    pub fn rich(self) -> bool {
        self != Kind::Other
    }

    /// Whether the kitty graphics protocol (inline images) is supported.
    pub fn graphics(self) -> bool {
        matches!(self, Kind::Ghostty | Kind::Kitty)
    }
}

/// Detect the terminal from environment variables. `var` returns a variable's value, or an empty
/// string if unset.
///
/// Inside tmux / screen the outer terminal's variables are inherited, but these multiplexers
/// intercept or rewrite most escape sequences, so they are always treated as plain terminals.
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

/// Detect the current terminal from environment variables.
pub fn detect_env() -> Kind {
    detect(|k| std::env::var(k).unwrap_or_default())
}

/// Parse an OSC 11 reply (`ESC ] 11 ; rgb:RRRR/GGGG/BBBB` + BEL or ST) into 8-bit RGB.
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

/// Whether a color is light (perceived luminance above half).
pub fn is_light((r, g, b): (u8, u8, u8)) -> bool {
    0.2126 * f64::from(r) + 0.7152 * f64::from(g) + 0.0722 * f64::from(b) > 128.0
}

/// Terminal window size: (columns, rows, cell width px, cell height px). Pixels are 0 when unavailable.
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
        // SAFETY: winsize is plain data; ioctl only writes into it.
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

    /// Send an OSC 11 background color query followed by DA1 as a sentinel: every terminal
    /// answers DA1, so getting the DA1 reply without a color means the terminal doesn't support
    /// the query, and we don't have to wait for the timeout.
    pub fn background() -> Option<(u8, u8, u8)> {
        let mut tty = tty()?;
        let fd = tty.as_raw_fd();
        // SAFETY: termios is plain data, filled in by tcgetattr.
        let mut orig: libc::termios = unsafe { std::mem::zeroed() };
        if unsafe { libc::tcgetattr(fd, &mut orig) } != 0 {
            return None;
        }
        // Use VTIME as the read timeout (at most 0.1 s per read). poll() on macOS doesn't support tty devices.
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

    /// Whether the DA1 reply (`ESC [ ? … c`) has been received.
    fn da1_done(reply: &[u8]) -> bool {
        let Some(pos) = reply.windows(3).position(|w| w == b"\x1b[?") else {
            return false;
        };
        reply[pos + 3..].contains(&b'c')
    }
}

/// Read the terminal window size (including cell pixel size).
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

/// Query the terminal background color. Returns `None` if the terminal doesn't reply or we're not
/// running in a terminal.
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
        // Over ssh only TERM is passed through.
        assert_eq!(detect(env(&[("TERM", "xterm-ghostty")])), Kind::Ghostty);
        assert_eq!(detect(env(&[("TERM", "xterm-kitty")])), Kind::Kitty);
        assert_eq!(detect(env(&[("TERM_PROGRAM", "WezTerm")])), Kind::WezTerm);
        assert_eq!(detect(env(&[("TERM_PROGRAM", "iTerm.app")])), Kind::Other);
        assert_eq!(detect(env(&[])), Kind::Other);
        // Inside tmux, inherited Ghostty variables don't count as Ghostty.
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
