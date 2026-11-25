//! Version ranges. The plan writes ranges npm-style (`">=0.4 <0.5"`); the `semver` crate
//! wants comma-separated comparators (`">=0.4, <0.5"`). Both spellings are accepted and
//! the range is stored, compared and written in the comma form.

use std::fmt;

use semver::{Version, VersionReq};

/// A semver range such as `^3` or `>=0.4, <0.5`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Range(VersionReq);

impl Range {
    /// Parses a range in either spelling. `||` alternatives and hyphen ranges are not
    /// supported.
    pub fn parse(text: &str) -> Result<Self, String> {
        let trimmed = text.trim();
        if trimmed.is_empty() {
            return Err("a range must not be empty (use `*` for any version)".into());
        }
        if trimmed.contains("||") {
            return Err("`||` alternatives are not supported".into());
        }
        let normal = if trimmed.contains(',') {
            trimmed.to_owned()
        } else {
            join_comparators(trimmed)
        };
        VersionReq::parse(&normal)
            .map(Range)
            .map_err(|e| e.to_string())
    }

    /// Whether `v` satisfies the range. As in Cargo, a pre-release only matches a
    /// comparator that names a pre-release of the same major.minor.patch.
    pub fn matches(&self, v: &Version) -> bool {
        self.0.matches(v)
    }

    /// The underlying `semver` requirement.
    pub fn req(&self) -> &VersionReq {
        &self.0
    }
}

/// `">= 0.4 <0.5"` to `">=0.4, <0.5"`: a token made only of operator characters is glued
/// to the next token.
fn join_comparators(s: &str) -> String {
    let mut parts: Vec<String> = Vec::new();
    let mut pending = String::new();
    for tok in s.split_whitespace() {
        if tok
            .chars()
            .all(|c| matches!(c, '=' | '>' | '<' | '~' | '^'))
        {
            pending.push_str(tok);
        } else {
            parts.push(format!("{pending}{tok}"));
            pending.clear();
        }
    }
    if !pending.is_empty() {
        parts.push(pending);
    }
    parts.join(", ")
}

impl fmt::Display for Range {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    fn v(s: &str) -> Version {
        Version::parse(s).unwrap()
    }

    #[test]
    fn both_spellings() {
        let a = Range::parse(">=0.4 <0.5").unwrap();
        let b = Range::parse(">=0.4, <0.5").unwrap();
        let c = Range::parse(">= 0.4 < 0.5").unwrap();
        assert_eq!(a, b);
        assert_eq!(a, c);
        assert_eq!(a.to_string(), ">=0.4, <0.5");
        assert!(a.matches(&v("0.4.9")));
        assert!(!a.matches(&v("0.5.0")));
        assert!(!a.matches(&v("0.3.9")));
    }

    #[test]
    fn caret_and_star() {
        assert!(Range::parse("^3").unwrap().matches(&v("3.2.1")));
        assert!(!Range::parse("^3").unwrap().matches(&v("4.0.0")));
        assert!(Range::parse("*").unwrap().matches(&v("9.9.9")));
    }

    #[test]
    fn rejects_garbage() {
        for bad in ["", "  ", "^3 || ^4", "1.0 - 2.0", "abc", ">=", "^x"] {
            assert!(Range::parse(bad).is_err(), "{bad}");
        }
    }
}
