use chrono::{DateTime, SecondsFormat, Utc};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::{fmt, str::FromStr};

/// On disk, the only supported schema version is the integer `1`.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct SchemaVersion;

impl Serialize for SchemaVersion {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_u32(1)
    }
}

impl<'de> Deserialize<'de> for SchemaVersion {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        match u32::deserialize(deserializer)? {
            1 => Ok(Self),
            _ => Err(serde::de::Error::custom("expected schema_version 1")),
        }
    }
}

macro_rules! checked_string {
    ($name:ident, $check:expr, $reason:literal) => {
        #[derive(Clone, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
        #[serde(try_from = "String", into = "String")]
        pub struct $name(String);

        impl TryFrom<String> for $name {
            type Error = String;

            fn try_from(value: String) -> Result<Self, Self::Error> {
                if ($check)(&value) {
                    Ok(Self(value))
                } else {
                    Err(concat!(stringify!($name), ": ", $reason).to_owned())
                }
            }
        }

        impl FromStr for $name {
            type Err = String;

            fn from_str(raw: &str) -> Result<Self, Self::Err> {
                Self::try_from(raw.to_owned())
            }
        }

        impl From<$name> for String {
            fn from(value: $name) -> Self {
                value.0
            }
        }

        impl $name {
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(&self.0)
            }
        }
    };
}

checked_string!(
    Title,
    |s: &str| s.chars().any(|c| !c.is_whitespace()),
    "title must contain non-whitespace text"
);
checked_string!(
    SnakeCase,
    |s: &str| {
        let mut bytes = s.bytes();
        matches!(bytes.next(), Some(b'a'..=b'z'))
            && bytes.all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
    },
    "expected snake_case"
);
checked_string!(
    ContentHash,
    |s: &str| s.len() == 64
        && s.bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)),
    "expected 64 lowercase hexadecimal characters"
);

// The schema's full URI-format validation will be covered in Commit 13.
checked_string!(
    HttpUrl,
    |s: &str| {
        s.strip_prefix("https://")
            .or_else(|| s.strip_prefix("http://"))
            .is_some_and(|rest| {
                !rest.is_empty() && !rest.starts_with('/') && !rest.chars().any(char::is_whitespace)
            })
    },
    "expected an http(s) URL"
);

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct UtcTimestamp(DateTime<Utc>);

impl UtcTimestamp {
    pub fn now() -> Self {
        Self(Utc::now())
    }

    pub fn into_inner(self) -> DateTime<Utc> {
        self.0
    }
}

fn schema_utc_format(raw: &str) -> bool {
    let bytes = raw.as_bytes();
    if bytes.len() < 20 || !bytes.is_ascii() || bytes.last() != Some(&b'Z') {
        return false;
    }
    for (index, &byte) in bytes.iter().enumerate().take(19) {
        match index {
            4 | 7 if byte == b'-' => {}
            10 if byte == b'T' => {}
            13 | 16 if byte == b':' => {}
            4 | 7 | 10 | 13 | 16 => return false,
            _ if byte.is_ascii_digit() => {}
            _ => return false,
        }
    }
    bytes.len() == 20
        || (bytes.len() > 21
            && bytes[19] == b'.'
            && bytes[20..bytes.len() - 1].iter().all(u8::is_ascii_digit))
}

impl FromStr for UtcTimestamp {
    type Err = String;

    fn from_str(raw: &str) -> Result<Self, Self::Err> {
        if !schema_utc_format(raw) {
            return Err("expected an RFC3339 UTC date-time ending in Z".to_owned());
        }
        DateTime::parse_from_rfc3339(raw)
            .map(|dt| Self(dt.with_timezone(&Utc)))
            .map_err(|err| err.to_string())
    }
}

impl fmt::Display for UtcTimestamp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0.to_rfc3339_opts(SecondsFormat::AutoSi, true))
    }
}

impl Serialize for UtcTimestamp {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for UtcTimestamp {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        String::deserialize(deserializer)?
            .parse()
            .map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn primitives_reject_invalid_values() {
        assert!("   ".parse::<Title>().is_err());
        assert!("source_collection".parse::<SnakeCase>().is_ok());
        assert!("SourceCollection".parse::<SnakeCase>().is_err());
        assert!("a".repeat(64).parse::<ContentHash>().is_ok());
        assert!("A".repeat(64).parse::<ContentHash>().is_err());
        assert!("file:///etc/passwd".parse::<HttpUrl>().is_err());
    }

    #[test]
    fn timestamps_are_valid_utc_datetimes() {
        for valid in ["2026-10-28T14:30:00Z", "2026-10-28T14:30:00.123Z"] {
            assert!(valid.parse::<UtcTimestamp>().is_ok());
        }
        for invalid in [
            "2026-10-28T14:30:00+00:00",
            "2026-02-30T14:30:00Z",
            "2026-10-28T14:30:00.Z",
            "2026-10-28T14:30:00z",
        ] {
            assert!(invalid.parse::<UtcTimestamp>().is_err());
        }
    }

    #[test]
    fn schema_version_only_accepts_one() {
        assert_eq!(serde_json::to_string(&SchemaVersion).unwrap(), "1");
        assert!(serde_json::from_str::<SchemaVersion>("1").is_ok());
        assert!(serde_json::from_str::<SchemaVersion>("2").is_err());
    }
}
