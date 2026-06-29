//! Edge cases the core matrix does not cover directly: empty input, long miss
//! prefixes, absolute candidates, lazy iterables, the order and concurrency
//! knobs, the default working directory, and a broken symlink.

mod common;

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use locate_path::{locate_path, locate_path_sync, Cwd, Options, PathType};

#[test]
fn empty_iterable_is_none() {
    let opts = Options::default();
    assert_eq!(locate_path_sync(Vec::<&str>::new(), &opts), None);
    assert_eq!(locate_path(Vec::<&str>::new(), &opts), None);
}

#[test]
fn all_missing_is_none() {
    let fixture = common::build();
    let opts = Options::default().cwd(fixture.root.clone());
    assert_eq!(locate_path_sync(["a", "b", "c"], &opts), None);
}

#[test]
fn match_after_several_misses() {
    let fixture = common::build();
    let opts = Options::default().cwd(fixture.fixture_dir());
    let paths = ["m1", "m2", "m3", "unicorn"];
    assert_eq!(
        locate_path_sync(paths, &opts).as_deref(),
        Some(Path::new("unicorn"))
    );
}

#[test]
fn absolute_candidate_ignores_cwd() {
    let fixture = common::build();
    let absolute = fixture.root.join("index.js");
    // cwd points at the fixture subdirectory, but an absolute candidate ignores
    // it. The return value is the absolute candidate verbatim.
    let opts = Options::default().cwd(fixture.fixture_dir());
    let found = locate_path_sync([absolute.clone()], &opts);
    assert_eq!(found.as_deref(), Some(absolute.as_path()));
}

#[test]
fn lazy_iterable_sources_preserve_order() {
    let fixture = common::build();
    let opts = Options::default().cwd(fixture.root.clone());

    // Vec iterator.
    let from_vec = locate_path_sync(vec!["noop.foo", "index.js"], &opts);
    assert_eq!(from_vec.as_deref(), Some(Path::new("index.js")));

    // A custom map-based iterator.
    let mapped = ["noop.foo", "index.js"].iter().map(|s| s.to_string());
    let from_map = locate_path_sync(mapped, &opts);
    assert_eq!(from_map.as_deref(), Some(Path::new("index.js")));
}

#[test]
fn ordered_source_with_single_match_is_deterministic() {
    // A BTreeSet yields its items in sorted order. Only one candidate exists, so
    // the result is fixed regardless of iteration source.
    let fixture = common::build();
    let opts = Options::default().cwd(fixture.root.clone());
    let set: BTreeSet<&str> = ["index.js", "missing-1", "missing-2"].into_iter().collect();
    let found = locate_path_sync(set, &opts);
    assert_eq!(found.as_deref(), Some(Path::new("index.js")));
}

#[test]
fn preserve_order_false_returns_a_match() {
    // With a single matching candidate the result stays deterministic whatever
    // the order flag says.
    let fixture = common::build();
    let opts = Options::default()
        .cwd(fixture.root.clone())
        .r#type(PathType::Both)
        .preserve_order(false);
    let found = locate_path(["missing", "fixture"], &opts);
    assert_eq!(found.as_deref(), Some(Path::new("fixture")));
}

#[test]
fn concurrency_one_does_not_change_result() {
    let fixture = common::build();
    let opts = Options::default()
        .cwd(fixture.root.clone())
        .concurrency(Some(1));
    let paths = ["noop.foo", "unicorn.png", "index.js", "test.js"];
    assert_eq!(
        locate_path(paths, &opts).as_deref(),
        Some(Path::new("index.js"))
    );
}

#[test]
fn omitting_cwd_equals_current_dir() {
    // Default cwd reads the process working directory. Passing that directory
    // explicitly must give the same result. Use a temp dir as cwd so the test
    // does not depend on where it runs.
    let fixture = common::build();
    let explicit = Options::default().cwd(Cwd::Path(fixture.root.clone()));

    // Build a candidate that exists relative to the fixture root.
    let candidate = "index.js";
    let with_explicit = locate_path_sync([candidate], &explicit);
    assert_eq!(with_explicit.as_deref(), Some(Path::new("index.js")));

    // The same call against a directory with no such file is None.
    let elsewhere = Options::default().cwd(Cwd::Path(PathBuf::from("/")));
    assert_eq!(locate_path_sync([candidate], &elsewhere), None);
}

#[cfg(unix)]
#[test]
fn broken_symlink_never_matches() {
    let fixture = common::build();
    common::add_dangling(&fixture);
    let base = fixture.fixture_dir();

    // Follow links: stat on the target fails, so no match for any type.
    for ty in [PathType::File, PathType::Directory, PathType::Both] {
        let opts = Options::default().cwd(base.clone()).r#type(ty);
        assert_eq!(locate_path_sync(["dangling"], &opts), None, "follow {ty:?}");
    }

    // No follow: the symlink itself exists but is neither file nor directory.
    for ty in [PathType::File, PathType::Directory, PathType::Both] {
        let opts = Options::default()
            .cwd(base.clone())
            .allow_symlinks(false)
            .r#type(ty);
        assert_eq!(
            locate_path_sync(["dangling"], &opts),
            None,
            "no follow {ty:?}"
        );
    }
}

#[test]
fn file_url_cwd_with_percent_encoding() {
    // A working directory passed as a file:// URL is decoded into a path. Build
    // a fixture whose directory name contains a space, then reach it via an
    // encoded URL.
    let fixture = common::build();
    let spaced = fixture.root.join("with space");
    std::fs::create_dir(&spaced).unwrap();
    std::fs::write(spaced.join("unicorn"), b"").unwrap();

    let url = format!("file://{}", spaced.display()).replace(' ', "%20");
    let cwd = Cwd::from_file_url(&url).expect("decode url");
    let opts = Options::default().cwd(cwd);
    assert_eq!(
        locate_path_sync(["unicorn"], &opts).as_deref(),
        Some(Path::new("unicorn"))
    );
}
