# locate-path

> Find the first path that exists on disk from an ordered list of candidates.

Give it candidate paths in priority order. It returns the first one that exists
and matches a type filter, resolved against a working directory. If none match
it returns `None`. The return value is the candidate you passed, not the
resolved absolute path.

## Installation

```toml
[dependencies]
locate-path = "0.1"
```

## Usage

```rust
use locate_path::{locate_path, Options};

let files = ["unicorn.png", "Cargo.toml", "pony.png"];
let found = locate_path(files, &Options::default());
// `Cargo.toml` is the first one on disk
assert_eq!(found.as_deref(), Some(std::path::Path::new("Cargo.toml")));
```

Filter by type and pick a working directory:

```rust
use locate_path::{locate_path_sync, Options, PathType};

let opts = Options::default()
    .cwd("src")
    .r#type(PathType::Directory);
let found = locate_path_sync(["lib.rs", "."], &opts);
```

## API

### `locate_path(paths, options) -> Option<PathBuf>`

Returns the first candidate that exists and matches the filter. `paths` is any
`IntoIterator` of items that implement `AsRef<Path>`. The result is the matched
candidate, owned, exactly as supplied.

### `locate_path_sync(paths, options) -> Option<PathBuf>`

The synchronous core. Same contract and result as `locate_path`. With default
options both functions return the same value for the same input.

### `Options`

| Field | Type | Default | Meaning |
|-------|------|---------|---------|
| `cwd` | `Cwd` | process directory | Directory that candidates resolve against. |
| `type` | `PathType` | `File` | What counts as a match. |
| `allow_symlinks` | `bool` | `true` | Follow symlinks when set. Report on the link itself when clear. |
| `concurrency` | `Option<usize>` | `None` | Kept for parity. Scanning is serial, so it does not change the result. |
| `preserve_order` | `bool` | `true` | Kept for parity. Scanning is in order, so the result is always deterministic. |

Build options with the setter methods, which take `self` and return `self`:

```rust
use locate_path::{Options, PathType};

let opts = Options::default()
    .cwd("fixtures")
    .r#type(PathType::Both)
    .allow_symlinks(false);
```

### `PathType`

`File`, `Directory`, or `Both`. A path that exists but is neither a regular file
nor a directory, such as a socket or a FIFO, never matches.

Parse from a string with `parse` or `TryFrom`. An unknown string returns an
`InvalidType` whose message is `Invalid type specified: <value>`.

```rust
use locate_path::PathType;

assert_eq!("directory".parse::<PathType>().unwrap(), PathType::Directory);
assert!("rainbows".parse::<PathType>().is_err());
```

### `Cwd`

The working directory source. Defaults to the process directory. Accepts a
string, a `Path`, or a `PathBuf` through `Into`. A `file://` URL goes through
`Cwd::from_file_url`, which percent-decodes the path.

## Behavior notes

- The returned value is the input candidate, never the resolved path.
- A relative `cwd` resolves against the process working directory, so resolution
  always yields an absolute path. An absolute candidate ignores `cwd`.
- Any stat error, including a missing path or a permission failure, marks that
  candidate as a non-match. No filesystem error propagates.
- With `allow_symlinks` set, a symlink matches its target's type. Without it, a
  symlink matches neither file nor directory nor both.
- Symlink tests in the suite run on Unix. Other platforms skip them.

## License

Licensed under the [MIT license](LICENSE).
