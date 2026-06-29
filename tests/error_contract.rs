//! Error messages for invalid type strings and invalid concurrency.
//!
//! Three type values must be rejected: `rainbows`, `toString`, and the
//! numeric-like `1`. The type filter is an enum, so those values cannot reach
//! the match logic. The string parser carries the rule instead, and these tests
//! lock the message byte for byte.

use locate_path::{AsyncOptions, ConcurrencyError, Options, PathType};

#[test]
fn rejects_unknown_type_string() {
    let err = PathType::try_from("rainbows").unwrap_err();
    assert_eq!(err.to_string(), "Invalid type specified: rainbows");
}

#[test]
fn rejects_prototype_key_to_string() {
    // `toString` is a method on the JavaScript Object prototype, so a naive
    // membership test could wrongly accept it. An enum has no prototype chain,
    // so the value is rejected like any other unknown string. The assertion
    // makes that explicit.
    let err = PathType::try_from("toString").unwrap_err();
    assert_eq!(err.to_string(), "Invalid type specified: toString");
}

#[test]
fn rejects_numeric_like_string() {
    let err = PathType::try_from("1").unwrap_err();
    assert_eq!(err.to_string(), "Invalid type specified: 1");
}

#[test]
fn accepts_valid_type_strings() {
    assert_eq!("file".parse::<PathType>().unwrap(), PathType::File);
    assert_eq!(
        "directory".parse::<PathType>().unwrap(),
        PathType::Directory
    );
    assert_eq!("both".parse::<PathType>().unwrap(), PathType::Both);
}

#[test]
fn invalid_type_keeps_value() {
    let err = PathType::try_from("rainbows").unwrap_err();
    assert_eq!(err.value(), "rainbows");
}

#[test]
fn concurrency_zero_is_rejected() {
    let options = Options::default().concurrency(Some(0));
    assert_eq!(options.concurrency_or_error(), Err(ConcurrencyError));
    assert_eq!(
        ConcurrencyError.to_string(),
        "Expected `concurrency` to be a number from 1 and up"
    );
}

#[test]
fn concurrency_positive_and_none_accepted() {
    let bounded: AsyncOptions = Options::default().concurrency(Some(4));
    assert_eq!(bounded.concurrency_or_error(), Ok(Some(4)));

    let unbounded = Options::default().concurrency(None);
    assert_eq!(unbounded.concurrency_or_error(), Ok(None));
}
