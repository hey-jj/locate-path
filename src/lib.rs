//! Find the first path that exists on disk from an ordered list of candidates.
//!
//! Give the crate an ordered iterator of candidate paths. It returns the first
//! candidate that exists and matches a type filter (file, directory, or both),
//! resolved against a working directory. If nothing matches it returns `None`.
//!
//! The returned value is the original candidate, exactly as supplied, not the
//! resolved absolute path.
//!
//! # Examples
//!
//! ```
//! use locate_path::{locate_path, Options};
//!
//! let files = ["unicorn.png", "Cargo.toml", "pony.png"];
//! let found = locate_path(files, &Options::default());
//! assert_eq!(found.as_deref(), Some(std::path::Path::new("Cargo.toml")));
//! ```
//!
//! # Type filter and symlinks
//!
//! [`PathType`] selects what counts as a match. With [`Options::allow_symlinks`]
//! set to `true` (the default), symbolic links are followed and match the type
//! of their target. With it set to `false`, a symlink reports as a symlink and
//! matches neither file nor directory nor both.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use std::str::FromStr;

mod resolve;

/// The kind of path that counts as a match.
///
/// A path that exists but is neither a regular file nor a directory, such as a
/// socket or a FIFO, never matches any variant.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum PathType {
    /// Match regular files only. This is the default.
    #[default]
    File,
    /// Match directories only.
    Directory,
    /// Match both files and directories.
    Both,
}

impl PathType {
    /// Test whether `metadata` satisfies this filter.
    fn matches(self, metadata: &fs::Metadata) -> bool {
        match self {
            PathType::File => metadata.is_file(),
            PathType::Directory => metadata.is_dir(),
            PathType::Both => metadata.is_file() || metadata.is_dir(),
        }
    }
}

/// Error returned when a string does not name a valid [`PathType`].
///
/// The message is `Invalid type specified: <value>`, where `<value>` is the
/// rejected input.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InvalidType {
    value: String,
}

impl InvalidType {
    /// The rejected input.
    pub fn value(&self) -> &str {
        &self.value
    }
}

impl fmt::Display for InvalidType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Invalid type specified: {}", self.value)
    }
}

impl std::error::Error for InvalidType {}

impl FromStr for PathType {
    type Err = InvalidType;

    /// Parse `"file"`, `"directory"`, or `"both"`. Reject everything else.
    ///
    /// Rejection returns [`InvalidType`] carrying the input verbatim, so the
    /// `Display` message matches the input exactly.
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "file" => Ok(PathType::File),
            "directory" => Ok(PathType::Directory),
            "both" => Ok(PathType::Both),
            other => Err(InvalidType {
                value: other.to_owned(),
            }),
        }
    }
}

impl TryFrom<&str> for PathType {
    type Error = InvalidType;

    fn try_from(s: &str) -> Result<Self, Self::Error> {
        s.parse()
    }
}

/// Where the working directory comes from.
///
/// Candidate paths are resolved against this directory before the existence
/// check. A relative directory is itself resolved against the process working
/// directory, so resolution always produces an absolute path.
///
/// Build a `Cwd` through [`Default`] for the process directory, the `From`
/// impls for an explicit path (string, `Path`, or `PathBuf`), or
/// [`Cwd::from_file_url`] for a `file://` URL. The [`Cwd::Path`] variant is part
/// of the public API so callers can match on it to inspect a custom directory.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum Cwd {
    /// Use the process working directory. This is the default.
    #[default]
    Process,
    /// Use an explicit path. May be relative or absolute.
    Path(PathBuf),
}

impl From<PathBuf> for Cwd {
    fn from(path: PathBuf) -> Self {
        Cwd::Path(path)
    }
}

impl From<&Path> for Cwd {
    fn from(path: &Path) -> Self {
        Cwd::Path(path.to_path_buf())
    }
}

impl From<&str> for Cwd {
    fn from(path: &str) -> Self {
        Cwd::Path(PathBuf::from(path))
    }
}

impl From<String> for Cwd {
    fn from(path: String) -> Self {
        Cwd::Path(PathBuf::from(path))
    }
}

impl Cwd {
    /// Build a working directory from a `file://` URL.
    ///
    /// Supported subset:
    ///
    /// - An empty host (`file:///path`) or a `localhost` host. Both yield the
    ///   path after the host. A bare `file://` or `file:///` yields the root
    ///   `/`.
    /// - A POSIX absolute path. The path part is percent-decoded into a
    ///   filesystem path.
    /// - UTF-8 content. The decoded bytes must form valid UTF-8.
    ///
    /// Not supported:
    ///
    /// - Windows drive letters. `file:///C:/x` decodes to the path `/C:/x`, not
    ///   `C:\x`.
    /// - Non-UTF-8 paths. Decoded bytes that are not valid UTF-8 are rejected
    ///   with [`FileUrlError::Encoding`], even when they name a real path on a
    ///   Unix filesystem.
    ///
    /// Errors:
    ///
    /// - A scheme other than `file` returns [`FileUrlError::Scheme`].
    /// - A host other than empty or `localhost` returns
    ///   [`FileUrlError::Authority`].
    /// - A bad percent escape, or an encoded path separator (`%2F` or `%5C`),
    ///   returns [`FileUrlError::Encoding`]. An encoded separator is rejected
    ///   rather than split into two path segments.
    ///
    /// # Examples
    ///
    /// ```
    /// use locate_path::Cwd;
    /// use std::path::PathBuf;
    ///
    /// let cwd = Cwd::from_file_url("file:///tmp/my%20dir").unwrap();
    /// assert_eq!(cwd, Cwd::Path(PathBuf::from("/tmp/my dir")));
    /// ```
    pub fn from_file_url(url: &str) -> Result<Self, FileUrlError> {
        let scheme = url.as_bytes().get(..7).ok_or(FileUrlError::Scheme)?;
        if !scheme.eq_ignore_ascii_case(b"file://") {
            return Err(FileUrlError::Scheme);
        }
        let rest = &url[7..];
        // Split the authority from the path at the first `/`. With no `/`, the
        // whole remainder is the authority and the path is the bare root.
        let (authority, path_part) = match rest.find('/') {
            Some(slash) => (&rest[..slash], &rest[slash..]),
            None => (rest, "/"),
        };
        if !(authority.is_empty() || authority.eq_ignore_ascii_case("localhost")) {
            return Err(FileUrlError::Authority);
        }
        let path_part = match path_part
            .bytes()
            .position(|byte| byte == b'?' || byte == b'#')
        {
            Some(end) => &path_part[..end],
            None => path_part,
        };
        let decoded = percent_decode(path_part)?;
        Ok(Cwd::Path(PathBuf::from(decoded)))
    }
}

/// Error returned when a `file://` URL cannot become a path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FileUrlError {
    /// The URL does not start with `file://`.
    Scheme,
    /// The URL carries a host other than an empty host or `localhost`.
    Authority,
    /// The URL contains an invalid percent escape, an encoded path separator,
    /// or non-UTF-8 content.
    Encoding,
}

impl fmt::Display for FileUrlError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FileUrlError::Scheme => write!(f, "URL scheme must be \"file\""),
            FileUrlError::Authority => write!(f, "file URL host must be empty or \"localhost\""),
            FileUrlError::Encoding => write!(f, "invalid percent encoding in file URL"),
        }
    }
}

impl std::error::Error for FileUrlError {}

/// Percent-decode a URL path segment into raw bytes, then into a UTF-8 string.
///
/// An encoded path separator (`%2F` for `/`, `%5C` for `\`) is rejected. The
/// conversion does not turn an encoded separator into a real one, since that
/// would split one path segment into two and locate a different path.
fn percent_decode(input: &str) -> Result<String, FileUrlError> {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'%' => {
                let hi = bytes.get(i + 1).copied().ok_or(FileUrlError::Encoding)?;
                let lo = bytes.get(i + 2).copied().ok_or(FileUrlError::Encoding)?;
                let byte = (hex_value(hi)? << 4) | hex_value(lo)?;
                if byte == b'/' || byte == b'\\' {
                    return Err(FileUrlError::Encoding);
                }
                out.push(byte);
                i += 3;
            }
            other => {
                out.push(other);
                i += 1;
            }
        }
    }
    String::from_utf8(out).map_err(|_| FileUrlError::Encoding)
}

/// Convert one hex digit to its value.
fn hex_value(byte: u8) -> Result<u8, FileUrlError> {
    match byte {
        b'0'..=b'9' => Ok(byte - b'0'),
        b'a'..=b'f' => Ok(byte - b'a' + 10),
        b'A'..=b'F' => Ok(byte - b'A' + 10),
        _ => Err(FileUrlError::Encoding),
    }
}

/// Settings for a search.
///
/// `Options` is a builder. Start from [`Options::default`] and chain the
/// setters. Each setter takes `self` and returns `self`, so calls compose:
///
/// ```
/// use locate_path::{Options, PathType};
///
/// let opts = Options::default()
///     .cwd("src")
///     .kind(PathType::Directory)
///     .allow_symlinks(false);
/// ```
///
/// Defaults: the working directory is the process directory, the type filter is
/// [`PathType::File`], and symbolic links are followed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Options {
    cwd: Cwd,
    kind: PathType,
    allow_symlinks: bool,
}

impl Default for Options {
    fn default() -> Self {
        Options {
            cwd: Cwd::default(),
            kind: PathType::default(),
            allow_symlinks: true,
        }
    }
}

impl Options {
    /// Set the working directory. Accepts a string, a `Path`, a `PathBuf`, or a
    /// [`Cwd`].
    #[must_use]
    pub fn cwd(mut self, cwd: impl Into<Cwd>) -> Self {
        self.cwd = cwd.into();
        self
    }

    /// Set the type filter.
    #[must_use]
    pub fn kind(mut self, kind: PathType) -> Self {
        self.kind = kind;
        self
    }

    /// Set whether symbolic links are followed.
    #[must_use]
    pub fn allow_symlinks(mut self, allow: bool) -> Self {
        self.allow_symlinks = allow;
        self
    }
}

/// Fetch metadata for `path`, following symlinks when `allow_symlinks` is set.
///
/// Any error, including a missing path, is reported as `None`. The caller then
/// treats the candidate as a non-match and moves on.
fn stat(path: &Path, allow_symlinks: bool) -> Option<fs::Metadata> {
    if allow_symlinks {
        fs::metadata(path).ok()
    } else {
        fs::symlink_metadata(path).ok()
    }
}

/// Return the first candidate that exists and matches the type filter.
///
/// Candidates are resolved against the working directory in [`Options`] and
/// checked in iteration order. The return value is the first candidate, as
/// supplied, whose resolved path exists and satisfies the type filter. It is
/// `None` when nothing matches.
///
/// A stat error of any kind, including a missing path or a permission failure,
/// marks that candidate as a non-match. No filesystem error propagates.
///
/// A relative candidate or a relative working directory needs the process
/// working directory to resolve. If that directory cannot be read, those
/// candidates cannot resolve and the function returns `None`. An absolute
/// working directory and absolute candidates never need it.
///
/// # Examples
///
/// ```
/// use locate_path::{locate_path, Options, PathType};
///
/// let opts = Options::default().kind(PathType::Directory);
/// let found = locate_path(["Cargo.toml", "src"], &opts);
/// assert_eq!(found.as_deref(), Some(std::path::Path::new("src")));
/// ```
pub fn locate_path<I, P>(paths: I, options: &Options) -> Option<PathBuf>
where
    I: IntoIterator<Item = P>,
    P: AsRef<Path>,
{
    let base = resolve::base_dir(&options.cwd);

    for candidate in paths {
        let candidate = candidate.as_ref();
        let Some(resolved) = resolve::resolve(base.as_deref(), candidate) else {
            continue;
        };
        let Some(metadata) = stat(&resolved, options.allow_symlinks) else {
            continue;
        };
        if options.kind.matches(&metadata) {
            return Some(candidate.to_path_buf());
        }
    }

    None
}

/// Synonym for [`locate_path`].
///
/// The crate scans synchronously. This name exists for callers who reach for a
/// `_sync` suffix. It calls [`locate_path`] and returns the same value.
///
/// # Examples
///
/// ```
/// use locate_path::{locate_path_sync, Options};
///
/// let found = locate_path_sync(["does-not-exist", "Cargo.toml"], &Options::default());
/// assert_eq!(found.as_deref(), Some(std::path::Path::new("Cargo.toml")));
/// ```
pub fn locate_path_sync<I, P>(paths: I, options: &Options) -> Option<PathBuf>
where
    I: IntoIterator<Item = P>,
    P: AsRef<Path>,
{
    locate_path(paths, options)
}

#[cfg(test)]
mod tests {
    use super::{Cwd, PathBuf};

    #[test]
    fn from_file_url_accepts_case_insensitive_scheme() {
        assert_eq!(
            Cwd::from_file_url("FILE:///tmp/x").unwrap(),
            Cwd::Path(PathBuf::from("/tmp/x"))
        );
        assert_eq!(
            Cwd::from_file_url("File:///tmp/x").unwrap(),
            Cwd::Path(PathBuf::from("/tmp/x"))
        );
    }

    #[test]
    fn from_file_url_ignores_query_and_fragment() {
        assert_eq!(
            Cwd::from_file_url("file:///tmp/a?b").unwrap(),
            Cwd::Path(PathBuf::from("/tmp/a"))
        );
        assert_eq!(
            Cwd::from_file_url("file:///tmp/a#b").unwrap(),
            Cwd::Path(PathBuf::from("/tmp/a"))
        );
        assert_eq!(
            Cwd::from_file_url("file:///tmp/a%3Fb%23c").unwrap(),
            Cwd::Path(PathBuf::from("/tmp/a?b#c"))
        );
    }
}
