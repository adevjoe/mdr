//! 在终端里直接显示图片（kitty 图形协议，Ghostty 和 kitty 支持）。
//!
//! 只读取本地图片文件；网络图片不下载，仍按文字显示。图片先按终端格子的像素尺寸缩小，
//! 再转成 PNG 发给终端，避免传输原始大图。

use std::path::{Path, PathBuf};

use base64::Engine as _;
use image::GenericImageView as _;

/// 显示图片所需的环境信息。
#[derive(Clone, Debug, PartialEq)]
pub struct ImageOptions {
    /// 相对路径的基准目录（Markdown 文件所在目录）。
    pub base_dir: PathBuf,
    /// 每个格子的像素尺寸 (宽, 高)。
    pub cell: (u16, u16),
    /// 一张图片最多占多少行。
    pub max_rows: usize,
}

/// 准备好发给终端的图片。
#[derive(Debug, PartialEq)]
pub struct Graphic {
    pub png: Vec<u8>,
    pub cols: usize,
    pub rows: usize,
}

/// 把图片地址解析成本地路径。网络地址和 data URI 返回 `None`。
pub fn resolve(src: &str, base: &Path) -> Option<PathBuf> {
    let src = src.trim();
    let path = if let Some(p) = src.strip_prefix("file://") {
        p
    } else if src.is_empty() || src.contains("://") || src.starts_with("data:") {
        return None;
    } else {
        src
    };
    // 去掉 `?query` 和 `#fragment`，并解开 %20 之类的编码。
    let path = path.split(['?', '#']).next().unwrap_or(path);
    let path = PathBuf::from(percent_decode(path));
    Some(if path.is_absolute() {
        path
    } else {
        base.join(path)
    })
}

fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && let Some(v) = s
                .get(i + 1..i + 3)
                .and_then(|h| u8::from_str_radix(h, 16).ok())
        {
            out.push(v);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// 按格子尺寸算出图片占多少列、多少行：不放大到超过原始尺寸，宽度不超过
/// `max_cols`，高度不超过 `max_rows`。
pub fn fit(
    (px_w, px_h): (u32, u32),
    (cell_w, cell_h): (u16, u16),
    max_cols: usize,
    max_rows: usize,
) -> (usize, usize) {
    let (cw, ch) = (f64::from(cell_w.max(1)), f64::from(cell_h.max(1)));
    let (pw, ph) = (f64::from(px_w.max(1)), f64::from(px_h.max(1)));
    let max_cols = max_cols.max(1);
    let max_rows = max_rows.max(1);
    let rows_for = |cols: usize| ((ph * cols as f64 * cw / pw / ch).round() as usize).max(1);
    let mut cols = ((pw / cw).ceil() as usize).clamp(1, max_cols);
    let mut rows = rows_for(cols);
    if rows > max_rows {
        rows = max_rows;
        cols = ((pw * rows as f64 * ch / ph / cw).round() as usize).clamp(1, max_cols);
    }
    (cols, rows)
}

/// 读取并缩放图片。读不了或格式不支持时返回 `None`。
pub fn load(src: &str, opts: &ImageOptions, max_cols: usize) -> Option<Graphic> {
    let path = resolve(src, &opts.base_dir)?;
    let img = image::ImageReader::open(path)
        .ok()?
        .with_guessed_format()
        .ok()?
        .decode()
        .ok()?;
    let (cols, rows) = fit(img.dimensions(), opts.cell, max_cols, opts.max_rows);
    let box_w = (cols * usize::from(opts.cell.0.max(1))) as u32;
    let box_h = (rows * usize::from(opts.cell.1.max(1))) as u32;
    let img = if img.width() > box_w || img.height() > box_h {
        img.resize(box_w, box_h, image::imageops::FilterType::Triangle)
    } else {
        img
    };
    let mut png = Vec::new();
    img.write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
        .ok()?;
    Some(Graphic { png, cols, rows })
}

/// 生成 kitty 图形协议的转义序列：在光标处显示图片，缩放到 cols×rows 个格子，
/// 显示后光标不动（`C=1`），由调用方负责换行。
pub fn kitty_sequence(g: &Graphic) -> String {
    let data = base64::engine::general_purpose::STANDARD.encode(&g.png);
    let chunks: Vec<&str> = data
        .as_bytes()
        .chunks(4096)
        .map(|c| std::str::from_utf8(c).unwrap_or_default())
        .collect();
    let mut out = String::with_capacity(data.len() + chunks.len() * 16 + 64);
    for (i, chunk) in chunks.iter().enumerate() {
        let more = u8::from(i + 1 < chunks.len());
        if i == 0 {
            out.push_str(&format!(
                "\x1b_Ga=T,f=100,t=d,q=2,C=1,c={},r={},m={more};",
                g.cols, g.rows
            ));
        } else {
            out.push_str(&format!("\x1b_Gq=2,m={more};"));
        }
        out.push_str(chunk);
        out.push_str("\x1b\\");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_local_paths_only() {
        let base = Path::new("/docs");
        assert_eq!(
            resolve("img/a%20b.png?raw=1", base),
            Some(PathBuf::from("/docs/img/a b.png"))
        );
        assert_eq!(
            resolve("/abs/x.png", base),
            Some(PathBuf::from("/abs/x.png"))
        );
        assert_eq!(
            resolve("file:///abs/x.png", base),
            Some(PathBuf::from("/abs/x.png"))
        );
        assert_eq!(resolve("https://example.com/a.png", base), None);
        assert_eq!(resolve("data:image/png;base64,AAAA", base), None);
    }

    #[test]
    fn fits_images_into_cells() {
        // 800×400 的图，格子 10×20：自然宽度 80 列，高 20 行。
        assert_eq!(fit((800, 400), (10, 20), 100, 50), (80, 20));
        // 宽度受限时等比缩小。
        assert_eq!(fit((800, 400), (10, 20), 40, 50), (40, 10));
        // 高度受限时同样等比缩小。
        assert_eq!(fit((800, 400), (10, 20), 100, 10), (40, 10));
        // 小图不放大。
        assert_eq!(fit((16, 16), (10, 20), 100, 50), (2, 1));
    }

    #[test]
    fn kitty_sequence_is_chunked() {
        let g = Graphic {
            png: vec![0u8; 5000],
            cols: 3,
            rows: 2,
        };
        let s = kitty_sequence(&g);
        assert!(s.starts_with("\x1b_Ga=T,f=100,t=d,q=2,C=1,c=3,r=2,m=1;"));
        assert!(s.contains("\x1b_Gq=2,m=0;"));
        assert!(s.ends_with("\x1b\\"));
        assert_eq!(s.matches("\x1b_G").count(), 2);
    }

    #[test]
    fn loads_and_scales_png() {
        let dir = std::env::temp_dir().join(format!("mdr-graphics-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        image::RgbImage::from_pixel(400, 200, image::Rgb([200, 30, 30]))
            .save(dir.join("red.png"))
            .unwrap();
        let opts = ImageOptions {
            base_dir: dir.clone(),
            cell: (10, 20),
            max_rows: 30,
        };
        let g = load("red.png", &opts, 20).unwrap();
        assert_eq!((g.cols, g.rows), (20, 5));
        let img = image::load_from_memory(&g.png).unwrap();
        assert_eq!(img.dimensions(), (200, 100));
        assert!(load("missing.png", &opts, 20).is_none());
        std::fs::remove_dir_all(dir).unwrap();
    }
}
