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
    /// The path part is percent-decoded into a filesystem path. A scheme other
    /// than `file` is rejected with [`FileUrlError::Scheme`]. This lets a caller
    /// pass a working directory as a `file://` URL instead of a plain path.
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
        let rest = url.strip_prefix("file://").ok_or(FileUrlError::Scheme)?;
        // Drop an empty authority (`file:///path`) or a `localhost` authority.
        let path_part = match rest.find('/') {
            Some(slash) => {
                let authority = &rest[..slash];
                if authority.is_empty() || authority.eq_ignore_ascii_case("localhost") {
                    &rest[slash..]
                } else {
                    return Err(FileUrlError::Authority);
                }
            }
            None => return Err(FileUrlError::Path),
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
    /// The URL has no path part.
    Path,
    /// The URL contains an invalid percent escape.
    Encoding,
}

impl fmt::Display for FileUrlError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FileUrlError::Scheme => write!(f, "URL scheme must be \"file\""),
            FileUrlError::Authority => write!(f, "file URL host must be empty or \"localhost\""),
            FileUrlError::Path => write!(f, "file URL must have a path"),
            FileUrlError::Encoding => write!(f, "invalid percent encoding in file URL"),
        }
    }
}

impl std::error::Error for FileUrlError {}

/// Percent-decode a URL path segment into raw bytes, then into a UTF-8 string.
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

/// Settings shared by both entry points.
///
/// Build with [`Options::default`] and the field setters, or construct the
/// struct directly. Defaults: working directory is the process directory, type
/// is [`PathType::File`], and symbolic links are followed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Options {
    /// Directory that candidate paths resolve against.
    pub cwd: Cwd,
    /// The kind of path that counts as a match.
    pub r#type: PathType,
    /// Follow symbolic links when `true`. Report on the link itself when
    /// `false`.
    pub allow_symlinks: bool,
    /// Cap on how many existence checks run at once. `None` means no cap.
    ///
    /// This crate scans candidates serially, so the value does not change the
    /// result. It is kept for API parity. A value of `Some(0)` is rejected by
    /// [`AsyncOptions::concurrency_or_error`].
    pub concurrency: Option<usize>,
    /// Return the earliest matching candidate by input order when `true`.
    ///
    /// This crate always scans in order, so the result is deterministic and the
    /// flag does not change it. It is kept for API parity.
    pub preserve_order: bool,
}

impl Default for Options {
    fn default() -> Self {
        Options {
            cwd: Cwd::default(),
            r#type: PathType::default(),
            allow_symlinks: true,
            concurrency: None,
            preserve_order: true,
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
    pub fn r#type(mut self, r#type: PathType) -> Self {
        self.r#type = r#type;
        self
    }

    /// Set whether symbolic links are followed.
    #[must_use]
    pub fn allow_symlinks(mut self, allow: bool) -> Self {
        self.allow_symlinks = allow;
        self
    }

    /// Set the concurrency cap.
    #[must_use]
    pub fn concurrency(mut self, concurrency: Option<usize>) -> Self {
        self.concurrency = concurrency;
        self
    }

    /// Set whether input order is preserved.
    #[must_use]
    pub fn preserve_order(mut self, preserve_order: bool) -> Self {
        self.preserve_order = preserve_order;
        self
    }
}

/// Alias for the option set that carries the async-style knobs.
///
/// The crate has one [`Options`] struct carrying every field. This alias names
/// the set that includes [`concurrency`](Options::concurrency) and
/// [`preserve_order`](Options::preserve_order), which shape an asynchronous
/// scan and have no effect on the serial scan here.
pub type AsyncOptions = Options;

impl AsyncOptions {
    /// Validate [`concurrency`](Options::concurrency).
    ///
    /// A positive integer or `None` (unbounded) is accepted. `Some(0)` is
    /// rejected. The error names the rule the value broke.
    pub fn concurrency_or_error(&self) -> Result<Option<usize>, ConcurrencyError> {
        match self.concurrency {
            Some(0) => Err(ConcurrencyError),
            other => Ok(other),
        }
    }
}

/// Error returned when `concurrency` is not a number from 1 and up.
///
/// The message is `Expected `concurrency` to be a number from 1 and up`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConcurrencyError;

impl fmt::Display for ConcurrencyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Expected `concurrency` to be a number from 1 and up")
    }
}

impl std::error::Error for ConcurrencyError {}

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
/// Candidates are resolved against [`Options::cwd`] and checked in iteration
/// order. The return value is the first candidate, as supplied, whose resolved
/// path exists and satisfies [`Options::type`](Options::type). It is `None` when
/// nothing matches.
///
/// A stat error of any kind, including a missing path or a permission failure,
/// marks that candidate as a non-match. No filesystem error propagates.
///
/// This is the synchronous core. [`locate_path`] forwards to it.
///
/// # Examples
///
/// ```
/// use locate_path::{locate_path_sync, Options, PathType};
///
/// let opts = Options::default().r#type(PathType::Directory);
/// let found = locate_path_sync(["Cargo.toml", "src"], &opts);
/// assert_eq!(found.as_deref(), Some(std::path::Path::new("src")));
/// ```
pub fn locate_path_sync<I, P>(paths: I, options: &Options) -> Option<PathBuf>
where
    I: IntoIterator<Item = P>,
    P: AsRef<Path>,
{
    let base = resolve::base_dir(&options.cwd);

    for candidate in paths {
        let candidate = candidate.as_ref();
        let resolved = resolve::resolve(&base, candidate);
        let Some(metadata) = stat(&resolved, options.allow_symlinks) else {
            continue;
        };
        if options.r#type.matches(&metadata) {
            return Some(candidate.to_path_buf());
        }
    }

    None
}

/// Return the first candidate that exists and matches the type filter.
///
/// Same contract and result as [`locate_path_sync`]. This name is the entry
/// point for callers who want the order and concurrency knobs in scope. With
/// default options the two functions return the same value for the same input.
///
/// # Examples
///
/// ```
/// use locate_path::{locate_path, Options};
///
/// let found = locate_path(["does-not-exist", "Cargo.toml"], &Options::default());
/// assert_eq!(found.as_deref(), Some(std::path::Path::new("Cargo.toml")));
/// ```
pub fn locate_path<I, P>(paths: I, options: &Options) -> Option<PathBuf>
where
    I: IntoIterator<Item = P>,
    P: AsRef<Path>,
{
    locate_path_sync(paths, options)
}
