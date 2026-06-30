//! Path resolution that mirrors `node:path.resolve`.
//!
//! Resolution joins the base directory and the candidate, then normalizes the
//! result to an absolute path. An absolute candidate ignores the base. A
//! relative base is resolved against the process working directory.

use std::env;
use std::ffi::OsString;
use std::path::{Component, Path, PathBuf};

use crate::Cwd;

/// Turn a [`Cwd`] into an absolute base directory.
///
/// [`Cwd::Process`] reads the process working directory. An absolute
/// [`Cwd::Path`] is used as is. A relative [`Cwd::Path`] is joined onto the
/// process working directory. The result is normalized so `.` and `..` segments
/// are collapsed.
///
/// Returns `None` when the base needs the process working directory and that
/// directory cannot be read. An absolute [`Cwd::Path`] never reads it, so it
/// always returns `Some`.
pub(crate) fn base_dir(cwd: &Cwd) -> Option<PathBuf> {
    match cwd {
        Cwd::Process => env::current_dir().ok(),
        Cwd::Path(path) if path.is_absolute() => Some(normalize(path)),
        Cwd::Path(path) => {
            let current = env::current_dir().ok()?;
            Some(normalize(&current.join(path)))
        }
    }
}

/// Resolve `candidate` against `base`.
///
/// An absolute candidate is returned normalized and `base` is ignored, so it
/// resolves even when `base` is `None`. A relative candidate is joined onto
/// `base`. A relative candidate with no `base` returns `None`, because it has
/// nothing to resolve against.
pub(crate) fn resolve(base: Option<&Path>, candidate: &Path) -> Option<PathBuf> {
    if candidate.is_absolute() {
        Some(normalize(candidate))
    } else {
        Some(normalize(&base?.join(candidate)))
    }
}

/// Collapse `.` and `..` segments lexically, without touching the filesystem.
///
/// This matches how `path.resolve` simplifies a path. It does not resolve
/// symlinks, which is left to the stat call. A `..` at the root stays at the
/// root.
///
/// The pass collects any prefix and root once, then a stack of plain segments.
/// `..` pops a segment in constant time, or is kept when the stack is empty and
/// there is no root. The `PathBuf` is assembled once at the end, so the whole
/// pass is linear with no rescans.
///
/// A Windows absolute path emits both a `Prefix` (the drive) and a `RootDir`,
/// so they need separate slots to keep `C:\foo` from collapsing to `\foo`.
fn normalize(path: &Path) -> PathBuf {
    let mut prefix: Option<OsString> = None;
    let mut root: Option<OsString> = None;
    let mut rooted = false;
    let mut stack: Vec<OsString> = Vec::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                let popped_name = matches!(stack.last(), Some(name) if name != "..");
                if popped_name {
                    stack.pop();
                } else if !rooted {
                    stack.push(OsString::from(".."));
                }
            }
            Component::Prefix(_) => {
                prefix = Some(component.as_os_str().to_os_string());
                rooted = true;
                stack.clear();
            }
            Component::RootDir => {
                root = Some(component.as_os_str().to_os_string());
                rooted = true;
                stack.clear();
            }
            other => stack.push(other.as_os_str().to_os_string()),
        }
    }

    let mut out = PathBuf::new();
    if let Some(prefix) = prefix {
        out.push(prefix);
    }
    if let Some(root) = root {
        out.push(root);
    }
    for segment in stack {
        out.push(segment);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn absolute_candidate_ignores_base() {
        let base = Path::new("/home/user");
        let candidate = Path::new("/etc/hosts");
        assert_eq!(
            resolve(Some(base), candidate),
            Some(PathBuf::from("/etc/hosts"))
        );
    }

    #[test]
    fn absolute_candidate_resolves_without_a_base() {
        let candidate = Path::new("/etc/hosts");
        assert_eq!(resolve(None, candidate), Some(PathBuf::from("/etc/hosts")));
    }

    #[test]
    fn relative_candidate_joins_base() {
        let base = Path::new("/home/user");
        let candidate = Path::new("file");
        assert_eq!(
            resolve(Some(base), candidate),
            Some(PathBuf::from("/home/user/file"))
        );
    }

    #[test]
    fn relative_candidate_without_a_base_is_none() {
        let candidate = Path::new("file");
        assert_eq!(resolve(None, candidate), None);
    }

    #[test]
    fn normalize_collapses_dot_and_dotdot() {
        assert_eq!(normalize(Path::new("/a/b/../c")), PathBuf::from("/a/c"));
        assert_eq!(normalize(Path::new("/a/./b")), PathBuf::from("/a/b"));
    }

    #[test]
    fn normalize_collapses_repeated_parent_segments() {
        assert_eq!(
            normalize(Path::new("/a/b/c/../../d")),
            PathBuf::from("/a/d")
        );
        assert_eq!(normalize(Path::new("a/b/../../c")), PathBuf::from("c"));
        assert_eq!(normalize(Path::new("a/../../b")), PathBuf::from("../b"));
    }

    #[test]
    fn dotdot_at_root_stays_at_root() {
        assert_eq!(normalize(Path::new("/../a")), PathBuf::from("/a"));
    }

    #[cfg(windows)]
    #[test]
    fn normalize_keeps_drive_prefix() {
        assert_eq!(
            normalize(Path::new(r"C:\foo\..\bar")),
            PathBuf::from(r"C:\bar")
        );
    }

    #[test]
    fn directory_link_to_self_resolves_to_dir() {
        let base = Path::new("/tmp/fixture");
        let candidate = Path::new(".");
        assert_eq!(
            resolve(Some(base), candidate),
            Some(PathBuf::from("/tmp/fixture"))
        );
    }
}
