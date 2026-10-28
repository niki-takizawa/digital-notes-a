//! Quote selection and resolution for `html_text_v1` Asset text.
//!
//! Pure operations only: no repository I/O and no Card mutation. Persistence
//! and citation marker insertion belong to the next commit.

use crate::{
    model::{
        AssetId, Evidence, EvidenceId, TextNormalization, TextPositionSelector,
        TextPositionSelectorType, TextQuoteSelector, TextQuoteSelectorType,
    },
    text::{self, CONTEXT_LENGTH},
};
use std::{error::Error, fmt};

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum EvidenceError {
    EmptyQuote,
    NotFound,
    Ambiguous { matches: usize },
    InvalidStart { start: usize },
}

impl fmt::Display for EvidenceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyQuote => f.write_str("quoted text is empty"),
            Self::NotFound => f.write_str("quote not found in Asset text"),
            Self::Ambiguous { matches } => write!(
                f,
                "ambiguous quote: {matches} matches; specify a codepoint start"
            ),
            Self::InvalidStart { start } => write!(
                f,
                "quote does not match a complete grapheme range at codepoint start {start}"
            ),
        }
    }
}

impl Error for EvidenceError {}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Resolution {
    /// Stored position, quote and both contexts all agree.
    Verified,
    /// The exact quote and contexts occur uniquely, but the stored offset moved.
    Recovered(usize),
    /// No unique, context-matching range is available.
    Unresolved,
}

fn char_string(chars: &[char]) -> String {
    chars.iter().collect()
}

/// Find quote starts at full Unicode grapheme boundaries. Positions are
/// counted in Unicode scalar values, not bytes.
pub fn quote_matches(text: &str, quote: &str) -> Vec<usize> {
    let data: Vec<char> = text.chars().collect();
    let needle: Vec<char> = quote.chars().collect();
    if needle.is_empty() || needle.len() > data.len() {
        return Vec::new();
    }
    let boundaries = text::grapheme_boundaries(text);
    data.windows(needle.len())
        .enumerate()
        .filter(|(start, candidate)| {
            *candidate == needle.as_slice()
                && boundaries[*start]
                && boundaries[*start + needle.len()]
        })
        .map(|(start, _)| start)
        .collect()
}

/// Normalize clipboard text by the same whitespace rule used for DOM text.
pub fn normalize_quote(quote: &str) -> Result<String, EvidenceError> {
    let normalized = text::normalize_whitespace(quote);
    if normalized.is_empty() {
        Err(EvidenceError::EmptyQuote)
    } else {
        Ok(normalized)
    }
}

/// Build Evidence from already-normalized Asset text.
///
/// If the quote appears more than once, `requested_start` must disambiguate
/// it. `requested_start`, if supplied, must point to a complete grapheme span.
pub fn make_evidence(
    id: EvidenceId,
    asset_id: AssetId,
    text: &str,
    quote: &str,
    requested_start: Option<usize>,
) -> Result<Evidence, EvidenceError> {
    let quote = normalize_quote(quote)?;
    let matches = quote_matches(text, &quote);
    let start = match requested_start {
        Some(start) if matches.contains(&start) => start,
        Some(start) => return Err(EvidenceError::InvalidStart { start }),
        None if matches.len() == 1 => matches[0],
        None if matches.is_empty() => return Err(EvidenceError::NotFound),
        None => {
            return Err(EvidenceError::Ambiguous {
                matches: matches.len(),
            });
        }
    };
    let data: Vec<char> = text.chars().collect();
    let end = start + quote.chars().count();
    let prefix = char_string(&data[start.saturating_sub(CONTEXT_LENGTH)..start]);
    let suffix = char_string(&data[end..end.saturating_add(CONTEXT_LENGTH).min(data.len())]);
    Ok(Evidence {
        id,
        asset_id,
        text_normalization: TextNormalization::HtmlTextV1,
        selector: TextQuoteSelector {
            selector_type: TextQuoteSelectorType::TextQuoteSelector,
            exact: quote,
            prefix,
            suffix,
        },
        position_selector: TextPositionSelector {
            selector_type: TextPositionSelectorType::TextPositionSelector,
            start: start as u64,
            end: end as u64,
        },
    })
}

fn context_matches(text: &[char], evidence: &Evidence, start: usize) -> bool {
    let quote_len = evidence.selector.exact.chars().count();
    let Some(end) = start.checked_add(quote_len) else {
        return false;
    };
    if end > text.len() || char_string(&text[start..end]) != evidence.selector.exact {
        return false;
    }
    let prefix = char_string(&text[start.saturating_sub(CONTEXT_LENGTH)..start]);
    let suffix = char_string(&text[end..end.saturating_add(CONTEXT_LENGTH).min(text.len())]);
    prefix == evidence.selector.prefix && suffix == evidence.selector.suffix
}

/// Verify the saved position, or recover *only* when quote + contexts select
/// one unique grapheme-aligned match. Does not silently change stored Evidence.
pub fn resolve(text: &str, evidence: &Evidence) -> Resolution {
    if evidence.selector.exact.is_empty() {
        return Resolution::Unresolved;
    }
    let data: Vec<char> = text.chars().collect();
    let start = usize::try_from(evidence.position_selector.start).ok();
    let end = usize::try_from(evidence.position_selector.end).ok();
    if let (Some(start), Some(end)) = (start, end)
        && end == start.saturating_add(evidence.selector.exact.chars().count())
        && text::grapheme_boundary(text, start)
        && text::grapheme_boundary(text, end)
        && start <= data.len()
        && context_matches(&data, evidence, start)
    {
        return Resolution::Verified;
    }
    let matching: Vec<_> = quote_matches(text, &evidence.selector.exact)
        .into_iter()
        .filter(|start| context_matches(&data, evidence, *start))
        .collect();
    if matching.len() == 1 {
        Resolution::Recovered(matching[0])
    } else {
        Resolution::Unresolved
    }
}
