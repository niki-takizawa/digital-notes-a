use serde::{Deserialize, Serialize};
use std::{fmt, str::FromStr};

fn valid_entity_id(raw: &str) -> bool {
    (8..=64).contains(&raw.len())
        && raw.as_bytes()[0].is_ascii_alphanumeric()
        && raw
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

// Distinct types prevent accidentally passing an AssetId as a WorkId.
// The schema deliberately does not prescribe a particular ID generator.
macro_rules! entity_id {
    ($name:ident) => {
        #[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
        #[serde(try_from = "String", into = "String")]
        pub struct $name(String);

        impl $name {
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl TryFrom<String> for $name {
            type Error = String;

            fn try_from(value: String) -> Result<Self, Self::Error> {
                if valid_entity_id(&value) {
                    Ok(Self(value))
                } else {
                    Err(format!(
                        "{}: expected a filename-safe ID of 8–64 ASCII characters",
                        stringify!($name)
                    ))
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

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(&self.0)
            }
        }
    };
}

entity_id!(WorkId);
entity_id!(AssetId);
entity_id!(CardId);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn id_matches_schema_boundary_and_character_constraints() {
        let long_valid = "z".repeat(64);
        for valid in ["a1234567", "A2345678", "a-b_0123", long_valid.as_str()] {
            assert!(valid.parse::<WorkId>().is_ok(), "{valid}");
        }
        let long_invalid = "z".repeat(65);
        for invalid in [
            "",
            "abc1234",
            long_invalid.as_str(),
            "-abcdefg",
            "_abcdefg",
            "hello world",
            "日本語abcdef",
        ] {
            assert!(invalid.parse::<WorkId>().is_err(), "{invalid}");
        }
    }

    #[test]
    fn id_deserialization_rejects_invalid_strings() {
        assert!(serde_json::from_str::<WorkId>("\"short\"").is_err());
        assert_eq!(
            serde_json::from_str::<AssetId>("\"asset123\"")
                .expect("valid asset ID")
                .to_string(),
            "asset123"
        );
    }
}
