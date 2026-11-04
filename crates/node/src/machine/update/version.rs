//! A release's version.

use super::Error;
use std::{cmp::Ordering, fmt, str::FromStr};

/// A release's version: dot-separated numbers, compared as numbers, so
/// `0.1.10` is newer than `0.1.9` and `1.2` is `1.2.0`.
#[derive(Debug, Clone)]
pub struct Version {
    text: String,
    /// The numbers without trailing zeros.
    parts: Vec<u64>,
}

impl FromStr for Version {
    type Err = Error;

    fn from_str(text: &str) -> Result<Self, Error> {
        let bad = || Error::BadVersion(text.to_owned());
        let trimmed = text.trim();
        let trimmed = trimmed.strip_prefix('v').unwrap_or(trimmed);
        let mut parts = trimmed
            .split('.')
            .map(|part| {
                if part.is_empty() || !part.bytes().all(|b| b.is_ascii_digit()) {
                    return Err(bad());
                }
                part.parse::<u64>().map_err(|_| bad())
            })
            .collect::<Result<Vec<_>, _>>()?;
        while parts.last() == Some(&0) {
            parts.pop();
        }
        Ok(Self {
            text: trimmed.to_owned(),
            parts,
        })
    }
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.text)
    }
}

impl PartialEq for Version {
    fn eq(&self, other: &Self) -> bool {
        self.parts == other.parts
    }
}

impl Eq for Version {}

impl PartialOrd for Version {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Version {
    fn cmp(&self, other: &Self) -> Ordering {
        self.parts.cmp(&other.parts)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn version(text: &str) -> Version {
        text.parse().expect("a valid version")
    }

    #[test]
    fn versions_compare_as_numbers() {
        assert!(version("0.1.10") > version("0.1.9"));
        assert!(version("0.2.0") > version("0.1.14"));
        assert!(version("1.0.0") > version("0.99.99"));
        assert!(version("0.1.14") < version("0.1.15"));
        assert_eq!(version("0.1.14"), version("0.1.14"));
        assert_eq!(version("1.2"), version("1.2.0"));
        assert_eq!(version("v0.1.14\n"), version("0.1.14"));
        assert_eq!(version(" 0.1.14 ").to_string(), "0.1.14");
    }
}
