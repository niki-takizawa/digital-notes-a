use super::AssetId;
use serde::{Deserialize, Serialize};
use std::{fmt, str::FromStr};

#[derive(Clone, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct EvidenceId(String);

impl FromStr for EvidenceId {
    type Err = String;

    fn from_str(raw: &str) -> Result<Self, Self::Err> {
        let digits = raw
            .strip_prefix("ev")
            .ok_or_else(|| "expected ev followed by a positive integer".to_owned())?;
        if digits.is_empty()
            || digits.starts_with('0')
            || !digits.bytes().all(|byte| byte.is_ascii_digit())
        {
            return Err("expected ev followed by a positive integer".to_owned());
        }
        Ok(Self(raw.to_owned()))
    }
}

impl TryFrom<String> for EvidenceId {
    type Error = String;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        value.parse()
    }
}

impl From<EvidenceId> for String {
    fn from(value: EvidenceId) -> Self {
        value.0
    }
}

impl fmt::Display for EvidenceId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum TextNormalization {
    #[serde(rename = "html_text_v1")]
    HtmlTextV1,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum TextQuoteSelectorType {
    #[serde(rename = "TextQuoteSelector")]
    TextQuoteSelector,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum TextPositionSelectorType {
    #[serde(rename = "TextPositionSelector")]
    TextPositionSelector,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TextQuoteSelector {
    #[serde(rename = "type")]
    pub selector_type: TextQuoteSelectorType,
    pub exact: String,
    pub prefix: String,
    pub suffix: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TextPositionSelector {
    #[serde(rename = "type")]
    pub selector_type: TextPositionSelectorType,
    pub start: u64,
    pub end: u64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Evidence {
    pub id: EvidenceId,
    pub asset_id: AssetId,
    pub text_normalization: TextNormalization,
    pub selector: TextQuoteSelector,
    pub position_selector: TextPositionSelector,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn evidence_id_rejects_noncanonical_values() {
        for valid in ["ev1", "ev10", "ev12345"] {
            assert!(valid.parse::<EvidenceId>().is_ok());
        }
        for invalid in ["ev0", "ev01", "ev", "e1", "ev-1", "ev１２"] {
            assert!(invalid.parse::<EvidenceId>().is_err());
        }
    }
}
