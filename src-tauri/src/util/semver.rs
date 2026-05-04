//! Lightweight three-part version comparison.
//!
//! We do not need the full `semver` crate: driver versions are strict
//! `MAJOR.MINOR.PATCH` triples written by us in `driver/Cargo.toml` / the
//! bundle `Info.plist`. Anything else returns `None` from [`parse`] and is
//! treated as "unknown" by callers.
#![allow(dead_code)] // Wired up by the driver-version-check change; keep API stable.

use std::cmp::Ordering;

/// Parse a `MAJOR.MINOR.PATCH` string into a tuple. Returns `None` if the
/// string is not exactly three dot-separated unsigned integers.
pub fn parse(s: &str) -> Option<(u32, u32, u32)> {
    let mut parts = s.split('.');
    let a = parts.next()?.parse::<u32>().ok()?;
    let b = parts.next()?.parse::<u32>().ok()?;
    let c = parts.next()?.parse::<u32>().ok()?;
    if parts.next().is_some() {
        return None;
    }
    Some((a, b, c))
}

/// Compare two `MAJOR.MINOR.PATCH` strings. Returns `None` if either side
/// fails to parse.
pub fn compare(a: &str, b: &str) -> Option<Ordering> {
    Some(parse(a)?.cmp(&parse(b)?))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_valid_triple() {
        assert_eq!(parse("1.2.3"), Some((1, 2, 3)));
        assert_eq!(parse("0.0.0"), Some((0, 0, 0)));
        assert_eq!(parse("10.20.30"), Some((10, 20, 30)));
    }

    #[test]
    fn rejects_malformed() {
        assert_eq!(parse(""), None);
        assert_eq!(parse("1"), None);
        assert_eq!(parse("1.2"), None);
        assert_eq!(parse("1.2.3.4"), None);
        assert_eq!(parse("1.2.x"), None);
        assert_eq!(parse("v1.2.3"), None);
        assert_eq!(parse("1.2.-3"), None);
        assert_eq!(parse("1.2.3-alpha"), None);
    }

    #[test]
    fn compare_equal() {
        assert_eq!(compare("1.0.0", "1.0.0"), Some(Ordering::Equal));
    }

    #[test]
    fn compare_older() {
        assert_eq!(compare("0.9.9", "1.0.0"), Some(Ordering::Less));
        assert_eq!(compare("1.0.0", "1.0.1"), Some(Ordering::Less));
        assert_eq!(compare("1.0.9", "1.1.0"), Some(Ordering::Less));
    }

    #[test]
    fn compare_newer() {
        assert_eq!(compare("1.0.1", "1.0.0"), Some(Ordering::Greater));
        assert_eq!(compare("2.0.0", "1.999.999"), Some(Ordering::Greater));
    }

    #[test]
    fn compare_malformed_returns_none() {
        assert_eq!(compare("bad", "1.0.0"), None);
        assert_eq!(compare("1.0.0", "bad"), None);
    }
}
