//! Classification: file type from extension, screenshot detection,
//! Indonesian month names, and EXIF-with-fallback photo dating.

use std::path::Path;

use chrono::{Local, TimeZone};

/// Broad file category used for "rapikan by type".
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FileCategory {
    Dokumen,
    Gambar,
    Video,
    Audio,
    Arsip,
    Installer,
    Lainnya,
}

impl FileCategory {
    /// Folder name segment for this category.
    pub fn folder_name(self) -> &'static str {
        match self {
            FileCategory::Dokumen => "Dokumen",
            FileCategory::Gambar => "Gambar",
            FileCategory::Video => "Video",
            FileCategory::Audio => "Audio",
            FileCategory::Arsip => "Arsip",
            FileCategory::Installer => "Installer",
            FileCategory::Lainnya => "Lainnya",
        }
    }
}

/// Classify by lowercase extension. The map lives here in one place so it
/// is easy to review and edit.
pub fn classify_by_extension(path: &Path) -> FileCategory {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();
    match ext.as_str() {
        "pdf" | "doc" | "docx" | "xls" | "xlsx" | "ppt" | "pptx" | "txt" | "rtf" | "odt"
        | "ods" | "odp" | "csv" | "md" | "epub" => FileCategory::Dokumen,
        "jpg" | "jpeg" | "png" | "gif" | "bmp" | "webp" | "heic" | "heif" | "svg" | "tif"
        | "tiff" | "raw" | "cr2" | "nef" => FileCategory::Gambar,
        "mp4" | "mkv" | "avi" | "mov" | "wmv" | "flv" | "webm" | "3gp" | "m4v" => {
            FileCategory::Video
        }
        "mp3" | "wav" | "flac" | "ogg" | "m4a" | "aac" | "opus" | "wma" => FileCategory::Audio,
        "zip" | "rar" | "7z" | "tar" | "gz" | "bz2" | "xz" | "cab" | "iso" => FileCategory::Arsip,
        "exe" | "msi" | "dmg" | "apk" | "deb" | "appimage" => FileCategory::Installer,
        _ => FileCategory::Lainnya,
    }
}

/// True for installer packages (used for the "installer lama" rule).
pub fn is_installer(path: &Path) -> bool {
    matches!(classify_by_extension(path), FileCategory::Installer)
}

/// Screenshot filename patterns (Indonesian + English + vendor prefixes).
fn screenshot_name_patterns() -> &'static [&'static str] {
    &[
        "screenshot",
        "screen shot",
        "tangkapan layar",
        "tangkapan",
        "screenshots",
        "citra layar",
    ]
}

/// Heuristic screenshot detection: filename patterns like `Screenshot*`,
/// `Screen Shot*`, `Tangkapan*`, plus PNG files with typical screen
/// dimensions. `dimensions` is (width, height) when known.
pub fn is_screenshot(file_name: &str, dimensions: Option<(u32, u32)>) -> bool {
    let lower = file_name.to_lowercase();
    if screenshot_name_patterns().iter().any(|p| lower.contains(p)) {
        return true;
    }
    // PNG with typical screen aspect/dimensions is likely a screenshot.
    if lower.ends_with(".png") {
        if let Some((w, h)) = dimensions {
            if w >= 800 && h >= 600 {
                return true;
            }
        }
    }
    false
}

/// Indonesian month names, index 1..=12.
pub const BULAN_INDONESIA: [&str; 12] = [
    "Januari",
    "Februari",
    "Maret",
    "April",
    "Mei",
    "Juni",
    "Juli",
    "Agustus",
    "September",
    "Oktober",
    "November",
    "Desember",
];

/// `Gambar/<YYYY>/<MM - NamaBulan>/` segment for the "rapikan by date" mode.
pub fn date_folder_name(year: i32, month: u32) -> String {
    let name = BULAN_INDONESIA
        .get(month.saturating_sub(1) as usize)
        .copied()
        .unwrap_or("Unknown");
    format!("{year:04}/{month:02} - {name}")
}

/// Photo date: EXIF DateTimeOriginal, falling back to file mtime.
/// Returns (year, month, day).
pub fn photo_date(path: &Path, mtime_secs: i64) -> (i32, u32, u32) {
    if let Some((y, m, d)) = exif_original_date(path) {
        return (y, m, d);
    }
    let dt = Local
        .timestamp_opt(mtime_secs, 0)
        .single()
        .unwrap_or_else(Local::now);
    (
        dt.format("%Y").to_string().parse().unwrap_or(1970),
        dt.format("%m").to_string().parse().unwrap_or(1),
        dt.format("%d").to_string().parse().unwrap_or(1),
    )
}

fn exif_original_date(path: &Path) -> Option<(i32, u32, u32)> {
    let file = std::fs::File::open(path).ok()?;
    let mut buf = std::io::BufReader::new(file);
    let exif = exif::Reader::new().read_from_container(&mut buf).ok()?;
    let field = exif.get_field(exif::Tag::DateTimeOriginal, exif::In::PRIMARY)?;
    let value = field.display_value().to_string();
    parse_exif_datetime(&value)
}

/// Parse `YYYY:MM:DD HH:MM:SS` as produced by EXIF DateTimeOriginal.
fn parse_exif_datetime(s: &str) -> Option<(i32, u32, u32)> {
    let date = s.split_whitespace().next()?;
    let mut parts = date.split(':');
    let y: i32 = parts.next()?.parse().ok()?;
    let m: u32 = parts.next()?.parse().ok()?;
    let d: u32 = parts.next()?.parse().ok()?;
    if (1..=12).contains(&m) && (1..=31).contains(&d) {
        Some((y, m, d))
    } else {
        None
    }
}

/// Format bytes for Indonesian UI strings, e.g. 2,1 GB.
pub fn format_bytes_id(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut v = bytes as f64;
    let mut u = 0;
    while v >= 1024.0 && u < UNITS.len() - 1 {
        v /= 1024.0;
        u += 1;
    }
    let s = if u == 0 {
        format!("{v:.0}")
    } else {
        format!("{v:.1}")
    };
    // Indonesian decimal separator.
    format!("{} {}", s.replace('.', ","), UNITS[u])
}

#[allow(unused_imports)]
use chrono::Timelike;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extension_map() {
        assert_eq!(
            classify_by_extension(Path::new("a.pdf")),
            FileCategory::Dokumen
        );
        assert_eq!(
            classify_by_extension(Path::new("a.JPG")),
            FileCategory::Gambar
        );
        assert_eq!(
            classify_by_extension(Path::new("setup.exe")),
            FileCategory::Installer
        );
        assert_eq!(
            classify_by_extension(Path::new("noext")),
            FileCategory::Lainnya
        );
    }

    #[test]
    fn screenshot_patterns() {
        assert!(is_screenshot("Screenshot 2024-01-01.png", None));
        assert!(is_screenshot("Screen Shot 2024.png", None));
        assert!(is_screenshot("Tangkapan Layar 1.png", None));
        assert!(!is_screenshot("liburan-bali.jpg", None));
        assert!(is_screenshot("random.png", Some((1920, 1080))));
        assert!(!is_screenshot("icon.png", Some((32, 32))));
    }

    #[test]
    fn indonesian_date_folder() {
        assert_eq!(date_folder_name(2024, 1), "2024/01 - Januari");
        assert_eq!(date_folder_name(2023, 8), "2023/08 - Agustus");
        assert_eq!(date_folder_name(2025, 12), "2025/12 - Desember");
    }

    #[test]
    fn exif_falls_back_to_mtime() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("photo.jpg");
        std::fs::write(&p, b"not a real jpeg").unwrap();
        let dt: chrono::DateTime<Local> = Local.with_ymd_and_hms(2022, 5, 6, 7, 8, 9).unwrap();
        let (y, m, d) = photo_date(&p, dt.timestamp());
        assert_eq!((y, m, d), (2022, 5, 6));
    }

    #[test]
    fn byte_formatting_indonesian() {
        assert_eq!(format_bytes_id(512), "512 B");
        assert_eq!(format_bytes_id(2_147_483_648), "2,0 GB");
    }
}
