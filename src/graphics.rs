//! Inline image display in the terminal (kitty graphics protocol, supported by Ghostty and kitty).
//!
//! Only local image files are read; remote images are not downloaded and are still shown as text.
//! Images are first downscaled to fit the terminal's cell pixel size, then encoded as PNG and sent
//! to the terminal, so large originals are never transmitted.

use std::path::{Path, PathBuf};

use base64::Engine as _;
use image::GenericImageView as _;

/// Environment information needed to display images.
#[derive(Clone, Debug, PartialEq)]
pub struct ImageOptions {
    /// Base directory for relative paths (the directory containing the Markdown file).
    pub base_dir: PathBuf,
    /// Pixel size of a single cell (width, height).
    pub cell: (u16, u16),
    /// Maximum number of rows a single image may occupy.
    pub max_rows: usize,
}

/// An image ready to be sent to the terminal.
#[derive(Debug, PartialEq)]
pub struct Graphic {
    pub png: Vec<u8>,
    pub cols: usize,
    pub rows: usize,
}

/// Resolves an image URL to a local path. Returns `None` for remote URLs and data URIs.
pub fn resolve(src: &str, base: &Path) -> Option<PathBuf> {
    let src = src.trim();
    let path = if let Some(p) = src.strip_prefix("file://") {
        p
    } else if src.is_empty() || src.contains("://") || src.starts_with("data:") {
        return None;
    } else {
        src
    };
    // Strip `?query` and `#fragment`, and percent-decode sequences like %20.
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

/// Computes how many columns and rows an image occupies given the cell size: never upscaled
/// beyond its original size, at most `max_cols` wide and at most `max_rows` tall.
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

/// Loads and scales an image. Returns `None` if it cannot be read or the format is unsupported.
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

/// Builds the kitty graphics protocol escape sequence: displays the image at the cursor, scaled
/// to cols×rows cells, leaving the cursor in place (`C=1`); the caller is responsible for line
/// breaks.
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
        // An 800×400 image with 10×20 cells: natural size is 80 columns by 20 rows.
        assert_eq!(fit((800, 400), (10, 20), 100, 50), (80, 20));
        // Scaled down proportionally when width-constrained.
        assert_eq!(fit((800, 400), (10, 20), 40, 50), (40, 10));
        // Likewise scaled down proportionally when height-constrained.
        assert_eq!(fit((800, 400), (10, 20), 100, 10), (40, 10));
        // Small images are not upscaled.
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
