//! Behavior matrix. Each case asserts the same result through both entry
//! points, so [`locate_path`] and [`locate_path_sync`] stay in lockstep.

mod common;

use std::path::Path;

use locate_path::{locate_path, locate_path_sync, Cwd, Options, PathType};

/// Which working directory a case uses.
#[derive(Clone, Copy)]
enum Base {
    /// The root of the built tree.
    Root,
    /// The `fixture/` subdirectory, passed as a string path.
    Fixture,
    /// The `fixture/` subdirectory, passed as a `file://` URL.
    FixtureUrl,
}

struct Case {
    paths: &'static [&'static str],
    base: Base,
    kind: PathType,
    allow_symlinks: bool,
    expected: Option<&'static str>,
    needs_symlinks: bool,
}

const CASES: &[Case] = &[
    // 1: first existing path in array order, default type file.
    Case {
        paths: &["noop.foo", "unicorn.png", "index.js", "test.js"],
        base: Base::Root,
        kind: PathType::File,
        allow_symlinks: true,
        expected: Some("index.js"),
        needs_symlinks: false,
    },
    // 2: no candidate exists.
    Case {
        paths: &["nonexistent"],
        base: Base::Root,
        kind: PathType::File,
        allow_symlinks: true,
        expected: None,
        needs_symlinks: false,
    },
    // 3: string cwd resolution.
    Case {
        paths: &["noop", "unicorn"],
        base: Base::Fixture,
        kind: PathType::File,
        allow_symlinks: true,
        expected: Some("unicorn"),
        needs_symlinks: false,
    },
    // 4: file:// URL cwd resolution.
    Case {
        paths: &["noop", "unicorn"],
        base: Base::FixtureUrl,
        kind: PathType::File,
        allow_symlinks: true,
        expected: Some("unicorn"),
        needs_symlinks: false,
    },
    // 5: a file does not match type directory.
    Case {
        paths: &["index.js"],
        base: Base::Root,
        kind: PathType::Directory,
        allow_symlinks: true,
        expected: None,
        needs_symlinks: false,
    },
    // 6: a directory does not match type file.
    Case {
        paths: &["fixture"],
        base: Base::Root,
        kind: PathType::File,
        allow_symlinks: true,
        expected: None,
        needs_symlinks: false,
    },
    // 7: default type file rejects a directory.
    Case {
        paths: &["fixture"],
        base: Base::Root,
        kind: PathType::File,
        allow_symlinks: true,
        expected: None,
        needs_symlinks: false,
    },
    // 8: a directory matches type directory.
    Case {
        paths: &["fixture"],
        base: Base::Root,
        kind: PathType::Directory,
        allow_symlinks: true,
        expected: Some("fixture"),
        needs_symlinks: false,
    },
    // 9: both matches a file.
    Case {
        paths: &["index.js"],
        base: Base::Root,
        kind: PathType::Both,
        allow_symlinks: true,
        expected: Some("index.js"),
        needs_symlinks: false,
    },
    // 10: both matches a directory.
    Case {
        paths: &["fixture"],
        base: Base::Root,
        kind: PathType::Both,
        allow_symlinks: true,
        expected: Some("fixture"),
        needs_symlinks: false,
    },
    // 11: both returns first existing, directory first by order.
    Case {
        paths: &["fixture", "index.js"],
        base: Base::Root,
        kind: PathType::Both,
        allow_symlinks: true,
        expected: Some("fixture"),
        needs_symlinks: false,
    },
    // 12: both returns first existing, file first by order.
    Case {
        paths: &["index.js", "fixture"],
        base: Base::Root,
        kind: PathType::Both,
        allow_symlinks: true,
        expected: Some("index.js"),
        needs_symlinks: false,
    },
    // 16: follow file-link to a file, match type file.
    Case {
        paths: &["file-link", "unicorn"],
        base: Base::Fixture,
        kind: PathType::File,
        allow_symlinks: true,
        expected: Some("file-link"),
        needs_symlinks: true,
    },
    // 17: follow directory-link to a dir, skip for type file, fall to unicorn.
    Case {
        paths: &["directory-link", "unicorn"],
        base: Base::Fixture,
        kind: PathType::File,
        allow_symlinks: true,
        expected: Some("unicorn"),
        needs_symlinks: true,
    },
    // 18: follow directory-link to a dir, match type directory.
    Case {
        paths: &["directory-link", "unicorn"],
        base: Base::Fixture,
        kind: PathType::Directory,
        allow_symlinks: true,
        expected: Some("directory-link"),
        needs_symlinks: true,
    },
    // 19: both with symlinks, file-link first by order.
    Case {
        paths: &["file-link", "directory-link"],
        base: Base::Fixture,
        kind: PathType::Both,
        allow_symlinks: true,
        expected: Some("file-link"),
        needs_symlinks: true,
    },
    // 20: both with symlinks, directory-link first by order.
    Case {
        paths: &["directory-link", "file-link"],
        base: Base::Fixture,
        kind: PathType::Both,
        allow_symlinks: true,
        expected: Some("directory-link"),
        needs_symlinks: true,
    },
    // 21: no follow, file-link is a symlink not a file, fall to unicorn.
    Case {
        paths: &["file-link", "unicorn"],
        base: Base::Fixture,
        kind: PathType::File,
        allow_symlinks: false,
        expected: Some("unicorn"),
        needs_symlinks: true,
    },
    // 22: no follow, nothing matches type directory.
    Case {
        paths: &["directory-link", "unicorn"],
        base: Base::Fixture,
        kind: PathType::Directory,
        allow_symlinks: false,
        expected: None,
        needs_symlinks: true,
    },
];

fn options_for(fixture: &common::Fixture, case: &Case) -> Options {
    let cwd = match case.base {
        Base::Root => Cwd::from(fixture.root.clone()),
        Base::Fixture => Cwd::from(fixture.fixture_dir()),
        Base::FixtureUrl => {
            let url = format!("file://{}", fixture.fixture_dir().display());
            Cwd::from_file_url(&url).expect("build file URL cwd")
        }
    };
    Options::default()
        .cwd(cwd)
        .kind(case.kind)
        .allow_symlinks(case.allow_symlinks)
}

#[test]
fn matrix_through_both_entry_points() {
    let fixture = common::build();

    for (index, case) in CASES.iter().enumerate() {
        if case.needs_symlinks && !common::SYMLINKS {
            continue;
        }

        let options = options_for(&fixture, case);
        let expected = case.expected.map(Path::new);

        let sync_result = locate_path_sync(case.paths, &options);
        assert_eq!(
            sync_result.as_deref(),
            expected,
            "sync mismatch at case index {index}: {:?}",
            case.paths
        );

        let async_result = locate_path(case.paths, &options);
        assert_eq!(
            async_result.as_deref(),
            expected,
            "async mismatch at case index {index}: {:?}",
            case.paths
        );
    }
}

/// The returned value is the input candidate, not the resolved absolute path.
#[test]
fn returns_input_string_not_resolved_path() {
    let fixture = common::build();
    let options = Options::default().cwd(fixture.fixture_dir());

    let found = locate_path_sync(["noop", "unicorn"], &options);
    assert_eq!(found.as_deref(), Some(Path::new("unicorn")));
    // A resolved path would be absolute and include the fixture directory.
    assert!(!found.unwrap().is_absolute());
}
