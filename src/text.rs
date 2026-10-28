//! `html_text_v1`: versioned, deterministic HTML-to-text normalization.
//!
//! Selector offsets count Unicode scalar values (codepoints), **not** UTF-8
//! bytes. This is intentionally compatible with the original PRI contract.
//! Changes to the rules below require a new normalization version rather than
//! silently reinterpreting Evidence persisted with `html_text_v1`.

use scraper::{ElementRef, Html, Selector};
use std::{error::Error, fmt};
use unicode_segmentation::UnicodeSegmentation;

pub const NORMALIZATION_NAME: &str = "html_text_v1";
pub const CONTEXT_LENGTH: usize = 60;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TextError {
    InvalidUtf8,
    MissingBody,
}

impl fmt::Display for TextError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidUtf8 => f.write_str("Asset HTML is not UTF-8"),
            Self::MissingBody => f.write_str("HTML document has no body"),
        }
    }
}

impl Error for TextError {}

fn excluded(name: &str) -> bool {
    matches!(
        name,
        "head"
            | "script"
            | "style"
            | "noscript"
            | "template"
            | "svg"
            | "math"
            | "iframe"
            | "canvas"
    )
}

fn block(name: &str) -> bool {
    matches!(
        name,
        "address"
            | "article"
            | "aside"
            | "blockquote"
            | "dd"
            | "div"
            | "dl"
            | "dt"
            | "fieldset"
            | "figcaption"
            | "figure"
            | "footer"
            | "form"
            | "h1"
            | "h2"
            | "h3"
            | "h4"
            | "h5"
            | "h6"
            | "header"
            | "hr"
            | "li"
            | "main"
            | "nav"
            | "ol"
            | "p"
            | "pre"
            | "section"
            | "table"
            | "td"
            | "th"
            | "tr"
            | "ul"
    )
}

/// The explicitly versioned whitespace set. Do not replace this with
/// `char::is_whitespace()`: its Unicode membership can change over time.
pub fn is_separator(c: char) -> bool {
    matches!(
        c,
        '\u{0009}'..='\u{000D}'
            | '\u{0020}'
            | '\u{0085}'
            | '\u{00A0}'
            | '\u{1680}'
            | '\u{2000}'..='\u{200A}'
            | '\u{2028}'
            | '\u{2029}'
            | '\u{202F}'
            | '\u{205F}'
            | '\u{3000}'
    )
}

/// Collapse runs of selected whitespace to one ASCII space and trim them;
/// retain original Unicode case and normalization form.
pub fn normalize_whitespace(source: &str) -> String {
    let mut output = String::new();
    let mut pending = false;
    for c in source.chars() {
        if is_separator(c) {
            if !output.is_empty() {
                pending = true;
            }
        } else {
            if pending {
                output.push(' ');
                pending = false;
            }
            output.push(c);
        }
    }
    output
}

fn walk(element: ElementRef<'_>, output: &mut String) {
    let name = element.value().name();
    if excluded(name) || element.value().attr("hidden").is_some() {
        return;
    }
    if block(name) || name == "br" {
        output.push(' ');
    }
    if name != "br" && name != "hr" {
        for child in element.children() {
            if let Some(element) = ElementRef::wrap(child) {
                walk(element, output);
            } else if let Some(text) = child.value().as_text() {
                output.push_str(text);
            }
        }
    }
    if block(name) {
        output.push(' ');
    }
}

/// Parse HTML5 and normalize the body in document order, without executing
/// scripts or requesting external resources. HTML must be UTF-8.
///
/// CSS presentation (e.g. `display:none`) is deliberately **not** evaluated:
/// this is a DOM text extraction contract, not a rendered-browser text API.
pub fn html_text_v1(html: &[u8]) -> Result<String, TextError> {
    let source = std::str::from_utf8(html).map_err(|_| TextError::InvalidUtf8)?;
    let document = Html::parse_document(source);
    let selector = Selector::parse("body").expect("static CSS selector is valid");
    let body = document
        .select(&selector)
        .next()
        .ok_or(TextError::MissingBody)?;
    let mut raw = String::new();
    walk(body, &mut raw);
    Ok(normalize_whitespace(&raw))
}

/// Return a slice indexed by codepoints, rejecting reversed/out-of-range spans.
pub fn slice_chars(text: &str, start: usize, end: usize) -> Option<String> {
    if end < start {
        return None;
    }
    let chars: Vec<char> = text.chars().collect();
    (end <= chars.len()).then(|| chars[start..end].iter().collect())
}

/// `true` only where the codepoint offset is between grapheme clusters.
pub fn grapheme_boundaries(text: &str) -> Vec<bool> {
    let mut result = vec![false; text.chars().count() + 1];
    result[0] = true;
    let mut offset = 0;
    for grapheme in text.graphemes(true) {
        offset += grapheme.chars().count();
        result[offset] = true;
    }
    result
}

pub fn grapheme_boundary(text: &str, offset: usize) -> bool {
    grapheme_boundaries(text)
        .get(offset)
        .copied()
        .unwrap_or(false)
}

/// Convert a Unicode codepoint offset into a UTF-8 byte offset.
pub fn cp_to_byte(text: &str, offset: usize) -> Option<usize> {
    if offset == text.chars().count() {
        Some(text.len())
    } else {
        text.char_indices().nth(offset).map(|(index, _)| index)
    }
}
