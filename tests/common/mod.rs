//! Shared fixture builder for the conformance suite.
//!
//! Each test gets a fresh temporary tree. The layout mirrors the two layers the
//! behavior matrix needs: a root that stands in for a project directory, and a
//! `fixture/` subdirectory with a file and two symlinks.

// Each integration test binary compiles its own copy of this module. Not every
// binary uses every helper, so some items look unused per binary.
#![allow(dead_code)]

use std::fs;
use std::path::PathBuf;

use tempfile::TempDir;

/// True on platforms where the suite creates and tests symlinks.
///
/// Symlink cases run on Unix. Other platforms skip them, since creating
/// symlinks there is not always available.
#[cfg(unix)]
pub const SYMLINKS: bool = true;
/// True on platforms where the suite creates and tests symlinks.
#[cfg(not(unix))]
pub const SYMLINKS: bool = false;

/// A built fixture tree. Holds the temp directory so it lives for the test.
pub struct Fixture {
    _dir: TempDir,
    /// Root of the tree. Stands in for a project directory.
    pub root: PathBuf,
}

impl Fixture {
    /// The `fixture/` subdirectory inside the root.
    pub fn fixture_dir(&self) -> PathBuf {
        self.root.join("fixture")
    }
}

/// Build the tree.
///
/// ```text
/// root/
///   index.js         empty file
///   test.js          empty file
///   fixture/         directory
///     unicorn        empty file
///     file-link      -> unicorn        (Unix only)
///     directory-link -> .              (Unix only)
/// ```
pub fn build() -> Fixture {
    let dir = TempDir::new().expect("create temp dir");
    let root = dir.path().to_path_buf();

    fs::write(root.join("index.js"), b"").expect("write index.js");
    fs::write(root.join("test.js"), b"").expect("write test.js");

    let fixture = root.join("fixture");
    fs::create_dir(&fixture).expect("create fixture dir");
    fs::write(fixture.join("unicorn"), b"").expect("write unicorn");

    #[cfg(unix)]
    {
        use std::os::unix::fs::symlink;
        symlink("unicorn", fixture.join("file-link")).expect("link file-link");
        symlink(".", fixture.join("directory-link")).expect("link directory-link");
    }

    Fixture { _dir: dir, root }
}

/// Add a broken symlink `dangling -> nope` to the fixture directory.
///
/// Used to check that a missing symlink target is swallowed and produces no
/// match. Unix only.
#[cfg(unix)]
pub fn add_dangling(fixture: &Fixture) {
    use std::os::unix::fs::symlink;
    symlink("nope", fixture.fixture_dir().join("dangling")).expect("link dangling");
}

/// Create a named pipe (FIFO) at `name` inside the fixture directory.
///
/// A FIFO exists on disk but is neither a regular file nor a directory. It
/// stands in for the family of special files (sockets, devices) that the type
/// filter must never match. Returns `true` if the FIFO was created. Unix only,
/// and only on platforms where `mkfifo` is available.
#[cfg(unix)]
pub fn add_fifo(fixture: &Fixture, name: &str) -> bool {
    use std::ffi::CString;

    let path = fixture.fixture_dir().join(name);
    let c_path = match CString::new(path.as_os_str().as_encoded_bytes()) {
        Ok(value) => value,
        Err(_) => return false,
    };
    // SAFETY: `c_path` is a valid NUL-terminated C string for the duration of
    // the call. `mkfifo` only reads it and returns a status code.
    let rc = unsafe { libc_mkfifo(c_path.as_ptr(), 0o644) };
    rc == 0
}

#[cfg(unix)]
extern "C" {
    #[link_name = "mkfifo"]
    fn libc_mkfifo(path: *const std::os::raw::c_char, mode: u32) -> std::os::raw::c_int;
}
