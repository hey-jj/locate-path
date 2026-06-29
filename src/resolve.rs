//! Path resolution that mirrors `node:path.resolve`.
//!
//! Resolution joins the base directory and the candidate, then normalizes the
//! result to an absolute path. An absolute candidate ignores the base. A
//! relative base is resolved against the process working directory.

use std::env;
use std::path::{Component, Path, PathBuf};

use crate::Cwd;

/// Turn an [`Cwd`] into an absolute base directory.
///
/// [`Cwd::Process`] reads the process working directory. A relative
/// [`Cwd::Path`] is joined onto the process working directory. The result is
/// normalized so trailing `.` and `..` segments are collapsed.
pub(crate) fn base_dir(cwd: &Cwd) -> PathBuf {
    let current = env::current_dir().unwrap_or_else(|_| PathBuf::from("/"));
    match cwd {
        Cwd::Process => current,
        Cwd::Path(path) => {
            if path.is_absolute() {
                normalize(path)
            } else {
                normalize(&current.join(path))
            }
        }
    }
}

/// Resolve `candidate` against `base`.
///
/// An absolute candidate is returned normalized and the base is ignored. A
/// relative candidate is joined onto the base. The base is already absolute, so
/// the result is always absolute.
pub(crate) fn resolve(base: &Path, candidate: &Path) -> PathBuf {
    if candidate.is_absolute() {
        normalize(candidate)
    } else {
        normalize(&base.join(candidate))
    }
}

/// Collapse `.` and `..` segments lexically, without touching the filesystem.
///
/// This matches how `path.resolve` simplifies a path. It does not resolve
/// symlinks, which is left to the stat call. A `..` at the root stays at the
/// root.
fn normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                if matches!(out.components().next_back(), Some(Component::Normal(_))) {
                    out.pop();
                } else if !has_root(&out) {
                    out.push("..");
                }
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

/// Report whether `path` starts with a root or prefix component.
fn has_root(path: &Path) -> bool {
    matches!(
        path.components().next(),
        Some(Component::RootDir) | Some(Component::Prefix(_))
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn absolute_candidate_ignores_base() {
        let base = Path::new("/home/user");
        let candidate = Path::new("/etc/hosts");
        assert_eq!(resolve(base, candidate), PathBuf::from("/etc/hosts"));
    }

    #[test]
    fn relative_candidate_joins_base() {
        let base = Path::new("/home/user");
        let candidate = Path::new("file");
        assert_eq!(resolve(base, candidate), PathBuf::from("/home/user/file"));
    }

    #[test]
    fn normalize_collapses_dot_and_dotdot() {
        assert_eq!(normalize(Path::new("/a/b/../c")), PathBuf::from("/a/c"));
        assert_eq!(normalize(Path::new("/a/./b")), PathBuf::from("/a/b"));
    }

    #[test]
    fn dotdot_at_root_stays_at_root() {
        assert_eq!(normalize(Path::new("/../a")), PathBuf::from("/a"));
    }

    #[test]
    fn directory_link_to_self_resolves_to_dir() {
        let base = Path::new("/tmp/fixture");
        let candidate = Path::new(".");
        assert_eq!(resolve(base, candidate), PathBuf::from("/tmp/fixture"));
    }
}
