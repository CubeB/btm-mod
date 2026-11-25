//! Syntax rules for ids and names. Everything is ASCII, so byte lengths equal character
//! counts.

/// Longest mod id, interface name, option name, event name, slot name or phase name.
pub const MAX_NAME: usize = 64;
/// Longest relative file path or glob.
pub const MAX_PATH: usize = 255;

/// A run of `[a-z0-9]` words joined by single hyphens, the first character a letter when
/// `letter_first`.
fn hyphen_words(s: &str, letter_first: bool) -> bool {
    let first_ok = s.chars().next().is_some_and(|c| {
        if letter_first {
            c.is_ascii_lowercase()
        } else {
            c.is_ascii_lowercase() || c.is_ascii_digit()
        }
    });
    first_ok
        && !s.ends_with('-')
        && !s.contains("--")
        && s.chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

/// A mod id: a registry name (`weather-storms`: lowercase letters, digits and single
/// hyphens, starting with a letter) or reverse-DNS (`io.github.alice.storms`: two or more
/// dot-separated labels of that shape; labels after the first may start with a digit).
pub fn is_mod_id(s: &str) -> bool {
    if s.is_empty() || s.len() > MAX_NAME {
        return false;
    }
    s.split('.')
        .enumerate()
        .all(|(i, label)| hyphen_words(label, i == 0))
}

/// An interface name, also the namespace of its events: `wind_meteors`, `economy`.
/// Lowercase letters, digits and underscores, starting with a letter.
pub fn is_interface_name(s: &str) -> bool {
    snake(s)
}

/// An option name or a built-in phase name: same rule as interface names.
pub fn is_snake_name(s: &str) -> bool {
    snake(s)
}

fn snake(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= MAX_NAME
        && s.chars().next().is_some_and(|c| c.is_ascii_lowercase())
        && s.chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
}

/// An event name: `StormStarted`. An ASCII capital, then letters and digits.
pub fn is_event_name(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= MAX_NAME
        && s.chars().next().is_some_and(|c| c.is_ascii_uppercase())
        && s.chars().all(|c| c.is_ascii_alphanumeric())
}

/// A slot (entry point) name: `UpdateWind`. A letter, then letters, digits and underscores.
pub fn is_slot_name(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= MAX_NAME
        && s.chars().next().is_some_and(|c| c.is_ascii_alphabetic())
        && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// An enum option value: 1 to 64 printable ASCII characters, no spaces.
pub fn is_enum_value(s: &str) -> bool {
    !s.is_empty() && s.len() <= MAX_NAME && s.chars().all(|c| c.is_ascii_graphic())
}

/// Why a relative path is invalid, or `None` when it is fine. `glob` allows glob syntax.
pub fn path_problem(s: &str, glob: bool) -> Option<&'static str> {
    if s.is_empty() {
        return Some("must not be empty");
    }
    if s.len() > MAX_PATH {
        return Some("is longer than 255 bytes");
    }
    if s.chars().any(char::is_control) {
        return Some("contains control characters");
    }
    if s.contains('\\') {
        return Some("uses `\\`; separate directories with `/`");
    }
    if s.starts_with('/') || s.contains(':') {
        return Some("must be relative to the mod directory");
    }
    if s.split('/').any(|c| c.is_empty() || c == "." || c == "..") {
        return Some("must not contain empty, `.` or `..` components");
    }
    if !glob && s.chars().any(|c| matches!(c, '*' | '?' | '[' | ']')) {
        return Some("must be a plain path, not a glob");
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mod_ids() {
        for ok in [
            "weather-storms",
            "a",
            "community-patch",
            "x1",
            "io.github.alice.storms",
            "com.example.7zip",
            "org.ta-universe.pack-2",
        ] {
            assert!(is_mod_id(ok), "{ok}");
        }
        for bad in [
            "",
            "Weather",
            "1abc",
            "a--b",
            "a-",
            "-a",
            "wind_meteors",
            "a..b",
            ".a",
            "a.",
            "a b",
            "1.example",
            &"a".repeat(65),
        ] {
            assert!(!is_mod_id(bad), "{bad}");
        }
    }

    #[test]
    fn other_names() {
        assert!(is_interface_name("wind_meteors"));
        assert!(!is_interface_name("Wind"));
        assert!(!is_interface_name("wind-meteors"));
        assert!(is_event_name("StormStarted"));
        assert!(!is_event_name("stormStarted"));
        assert!(!is_event_name("Storm_Started"));
        assert!(is_slot_name("UpdateWind"));
        assert!(is_slot_name("update_wind"));
        assert!(!is_slot_name("1x"));
        assert!(is_enum_value("high"));
        assert!(!is_enum_value("a b"));
    }

    #[test]
    fn paths() {
        assert_eq!(path_problem("data/a.hpi", false), None);
        assert_eq!(path_problem("data/*.hpi", true), None);
        assert!(path_problem("data/*.hpi", false).is_some());
        for bad in ["", "/a", "a\\b", "c:/a", "a//b", "./a", "a/../b", "a\n"] {
            assert!(path_problem(bad, true).is_some(), "{bad:?}");
        }
    }
}
