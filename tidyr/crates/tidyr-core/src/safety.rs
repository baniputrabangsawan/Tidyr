//! Protected-path checks: refuse to scan-for-modification, move, or delete
//! anything inside OS/system locations or the app's own data folders.
//!
//! Paths are normalized and canonicalized first so `..` segments and
//! symlink tricks cannot bypass the checks.

use std::path::{Component, Path, PathBuf};

use crate::{Error, Result};

/// Normalize a path lexically: resolve `.` and `..` without touching the
/// filesystem (so it works for paths that do not exist yet).
pub fn normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for comp in path.components() {
        match comp {
            Component::CurDir => {}
            Component::ParentDir => {
                if !out.pop() {
                    // Leading `..` on a relative path: keep it.
                    out.push("..");
                }
            }
            other => out.push(other.as_os_str()),
        }
    }
    if out.as_os_str().is_empty() {
        PathBuf::from(".")
    } else {
        out
    }
}

/// Canonicalize if the path exists, otherwise fall back to an absolute
/// normalized form. Canonicalization resolves symlinks, which closes the
/// symlink-escape bypass.
pub fn canonicalize_best_effort(path: &Path) -> PathBuf {
    if let Ok(c) = path.canonicalize() {
        return c;
    }
    let abs = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join(path)
    };
    normalize(&abs)
}

/// OS/system locations that must never be modified. Checked against the
/// canonicalized candidate path (prefix match on whole components).
fn protected_prefixes() -> Vec<PathBuf> {
    let mut v = Vec::new();
    if cfg!(windows) {
        if let Ok(windir) = std::env::var("WINDIR") {
            v.push(PathBuf::from(windir));
        } else {
            v.push(PathBuf::from(r"C:\Windows"));
        }
        for var in ["ProgramFiles", "ProgramFiles(x86)", "ProgramData"] {
            if let Ok(p) = std::env::var(var) {
                v.push(PathBuf::from(p));
            }
        }
        v.push(PathBuf::from(r"C:\Program Files"));
        v.push(PathBuf::from(r"C:\Program Files (x86)"));
        v.push(PathBuf::from(r"C:\ProgramData"));
    } else {
        for p in [
            "/",
            "/usr",
            "/bin",
            "/sbin",
            "/etc",
            "/private/etc",
            "/System",
            "/Library",
            "/var",
            "/private/var",
            "/boot",
            "/proc",
            "/sys",
            "/dev",
        ] {
            v.push(PathBuf::from(p));
        }
        v.push(PathBuf::from("/home"));
        v.push(PathBuf::from("/root"));
        v.push(PathBuf::from("/private/var/root"));
    }
    v
}

/// Allowed user temporary workspaces (such as macOS /var/folders/... or /tmp/user...).
/// These live under system parent paths on some OSes (e.g. macOS tempdirs are symlinked
/// under /private/var/folders) but are legitimate scratch spaces, provided they are strictly
/// inside the temporary folder rather than the system root itself.
fn is_allowed_temp_workspace(path: &Path) -> bool {
    #[cfg(not(windows))]
    {
        let allowed = [
            "/tmp",
            "/private/tmp",
            "/var/tmp",
            "/private/var/tmp",
            "/var/folders",
            "/private/var/folders",
        ];
        for p in allowed {
            let prefix = Path::new(p);
            if path.starts_with(prefix) && path != prefix {
                return true;
            }
        }
    }
    false
}

/// Returns true if `candidate` equals `prefix` or lies inside it,
/// comparing whole path components case-sensitively (and on Windows,
/// case-insensitively — handled by callers passing canonicalized paths;
/// we additionally compare case-insensitively on Windows here).
fn is_within(candidate: &Path, prefix: &Path) -> bool {
    let mut c = candidate.components();
    let mut p = prefix.components();
    loop {
        match (c.next(), p.next()) {
            (Some(a), Some(b)) => {
                let eq = if cfg!(windows) {
                    a.as_os_str().to_string_lossy().to_lowercase()
                        == b.as_os_str().to_string_lossy().to_lowercase()
                } else {
                    a.as_os_str() == b.as_os_str()
                };
                if !eq {
                    return false;
                }
            }
            (Some(_), None) => return true, // candidate longer: inside
            (None, None) => return true,    // equal
            (None, Some(_)) => return false, // candidate shorter
        }
    }
}

/// True if `path` is a filesystem root (drive root on Windows, `/` elsewhere).
pub fn is_filesystem_root(path: &Path) -> bool {
    let mut comps = path.components();
    match comps.next() {
        Some(Component::RootDir) => comps.next().is_none(),
        Some(Component::Prefix(_)) => match comps.next() {
            Some(Component::RootDir) => comps.next().is_none(),
            None => true,
            _ => false,
        },
        _ => false,
    }
}

/// Check a user-selected folder before scanning for modification.
/// Returns an error if the folder is protected, a filesystem root, the user
/// profile root itself, or inside the app's data/quarantine dirs.
pub fn check_scan_root(path: &Path, app_data_dir: &Path) -> Result<PathBuf> {
    let canon = canonicalize_best_effort(path);
    if is_filesystem_root(&canon) {
        return Err(Error::ProtectedPath(canon));
    }
    // User profile root itself (parent of home but home itself): refuse.
    if let Some(home) = dirs_home() {
        if canon == home {
            return Err(Error::ProtectedPath(canon));
        }
    }
    if !is_allowed_temp_workspace(&canon) {
        for prefix in protected_prefixes() {
            if is_filesystem_root(&prefix) {
                continue;
            }
            if is_within(&canon, &prefix) {
                return Err(Error::ProtectedPath(canon));
            }
        }
    }
    let app_canon = canonicalize_best_effort(app_data_dir);
    if is_within(&canon, &app_canon) || is_within(&app_canon, &canon) {
        return Err(Error::ProtectedPath(canon));
    }
    Ok(canon)
}

/// Check a single path before a move/quarantine/delete action.
pub fn check_action_path(path: &Path, app_data_dir: &Path) -> Result<PathBuf> {
    // Same rules as scan root; actions target files inside, so being equal
    // to the app dir itself is also refused via is_within.
    check_scan_root(path, app_data_dir)
}

fn dirs_home() -> Option<PathBuf> {
    if let Some(home) = std::env::var_os("HOME").map(PathBuf::from) {
        return Some(home);
    }
    #[cfg(windows)]
    {
        std::env::var_os("USERPROFILE").map(PathBuf::from)
    }
    #[cfg(not(windows))]
    {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_resolves_dotdot() {
        assert_eq!(normalize(Path::new("/a/b/../c")), PathBuf::from("/a/c"));
        assert_eq!(normalize(Path::new("a/./b")), PathBuf::from("a/b"));
    }

    #[test]
    fn filesystem_root_detection() {
        assert!(is_filesystem_root(Path::new("/")));
        assert!(!is_filesystem_root(Path::new("/etc")));
        assert!(!is_filesystem_root(Path::new("/home/user")));
        #[cfg(windows)]
        {
            assert!(is_filesystem_root(Path::new(r"C:\")));
            assert!(is_filesystem_root(Path::new(r"C:")));
            assert!(!is_filesystem_root(Path::new(r"C:\Windows")));
            assert!(!is_filesystem_root(Path::new(r"C:\Users\User")));
        }
    }

    #[cfg(not(windows))]
    #[test]
    fn refuses_system_dirs() {
        let app = PathBuf::from("/tmp/tidyr-test-app-data-xyz");
        for p in ["/etc", "/usr", "/var", "/", "/home", "/System"] {
            assert!(
                check_scan_root(Path::new(p), &app).is_err(),
                "{p} should be refused"
            );
        }
    }

    #[cfg(not(windows))]
    #[test]
    fn refuses_dotdot_escape() {
        // /tmp/safe/../../etc must be refused even though the literal path
        // starts with /tmp/safe.
        let app = PathBuf::from("/tmp/tidyr-test-app-data-xyz");
        assert!(check_scan_root(Path::new("/tmp/safe/../../etc"), &app).is_err());
    }

    #[cfg(not(windows))]
    #[test]
    fn refuses_symlink_escape() {
        let dir = tempfile::tempdir().unwrap();
        let link = dir.path().join("link");
        #[cfg(unix)]
        std::os::unix::fs::symlink("/etc", &link).unwrap();
        let app = dir.path().join("app-data");
        assert!(check_scan_root(&link, &app).is_err());
    }

    #[cfg(not(windows))]
    #[test]
    fn allows_normal_folder() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("Downloads");
        std::fs::create_dir(&target).unwrap();
        let app = dir.path().join("app-data");
        assert!(check_scan_root(&target, &app).is_ok());
    }

    #[cfg(not(windows))]
    #[test]
    fn macos_temp_and_system_paths_behavior() {
        let app = PathBuf::from("/tmp/tidyr-test-app-data-xyz");
        let temp_dir = tempfile::tempdir().unwrap();
        assert!(check_scan_root(temp_dir.path(), &app).is_ok());
        assert!(check_scan_root(&temp_dir.path().join("subfolder"), &app).is_ok());

        for sys in ["/etc", "/usr", "/var", "/System", "/Library"] {
            assert!(
                check_scan_root(Path::new(sys), &app).is_err(),
                "expected {sys} to be refused"
            );
        }
    }
    #[cfg(not(windows))]
    #[test]
    fn refuses_app_data_dir_itself() {
        let dir = tempfile::tempdir().unwrap();
        let app = dir.path().join("app-data");
        std::fs::create_dir(&app).unwrap();
        assert!(check_scan_root(&app, &app).is_err());
    }
}
