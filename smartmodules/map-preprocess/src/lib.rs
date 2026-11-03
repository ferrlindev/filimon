// ─────────────────────────────────────────────────────────────────────────────
// map-preprocess SmartModule
//
// This SmartModule performs Stage 2 of the WISE pipeline:
// - Extracts visible text from HTML
// - Tokenizes and lemmatizes the text
// - Computes term frequencies
//
// Input: RawPage { url, html }
// Output: PreprocessedBundle { raw_page, tokens }
// ─────────────────────────────────────────────────────────────────────────────

use fluvio_smartmodule::{smartmodule, SmartModuleRecord, RecordData, Result, eyre};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// ── Data Structures ───────────────────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize)]
pub struct RawPage {
    pub url: String,
    pub html: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenizedContent {
    pub url: String,
    pub tokens: Vec<String>,
    pub frequencies: HashMap<String, usize>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PreprocessedBundle {
    pub raw_page: RawPage,
    pub tokens: TokenizedContent,
}

// ── Stop Words ────────────────────────────────────────────────────────────────

const STOP_WORDS: &[&str] = &[
    "a", "about", "above", "after", "again", "against", "all", "am", "an", "and",
    "any", "are", "as", "at", "be", "because", "been", "before", "being", "below",
    "between", "both", "but", "by", "can", "cannot", "could", "did", "do", "does",
    "doing", "don't", "down", "during", "each", "few", "for", "from", "further",
    "get", "got", "had", "has", "have", "having", "he", "her", "here", "hers",
    "herself", "him", "himself", "his", "how", "i", "if", "in", "into", "is",
    "it", "its", "itself", "just", "me", "more", "most", "my", "myself", "no",
    "nor", "not", "of", "off", "on", "once", "only", "or", "other", "our", "ours",
    "ourselves", "out", "over", "own", "same", "she", "should", "so", "some",
    "such", "than", "that", "the", "their", "theirs", "them", "themselves", "then",
    "there", "these", "they", "this", "those", "through", "to", "too", "under",
    "until", "up", "very", "was", "we", "were", "what", "when", "where", "which",
    "while", "who", "whom", "why", "will", "with", "would", "you", "your", "yours",
    "yourself", "yourselves",
];

// ── Preprocessing Functions ───────────────────────────────────────────────────

/// Extract visible text from HTML by finding content between tags.
/// Note: This is a simplified extraction for SmartModule WASM compatibility.
/// For full HTML parsing, use the main application's scraper-based implementation.
fn extract_visible_text(html: &str) -> String {
    let mut result = String::new();
    let mut in_tag = false;
    let mut in_script = false;
    let mut in_style = false;

    let html_lower = html.to_lowercase();
    let chars: Vec<char> = html.chars().collect();
    let lower_chars: Vec<char> = html_lower.chars().collect();

    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];

        // Check for script/style tags
        if i + 7 < chars.len() {
            let slice: String = lower_chars[i..i + 7].iter().collect();
            if slice == "<script" {
                in_script = true;
            } else if slice == "</scrip" {
                in_script = false;
            }
        }
        if i + 6 < chars.len() {
            let slice: String = lower_chars[i..i + 6].iter().collect();
            if slice == "<style" {
                in_style = true;
            } else if slice == "</styl" {
                in_style = false;
            }
        }

        if c == '<' {
            in_tag = true;
        } else if c == '>' {
            in_tag = false;
            result.push(' ');
        } else if !in_tag && !in_script && !in_style {
            result.push(c);
        }

        i += 1;
    }

    // Clean up whitespace
    result
        .split_whitespace()
        .filter(|s| s.len() > 3)
        .collect::<Vec<_>>()
        .join(" ")
}

/// Tokenize and lemmatize text.
fn tokenize_and_lemmatize(text: &str) -> Vec<String> {
    text.split(|c: char| !c.is_alphanumeric())
        .map(str::to_lowercase)
        .filter(|w| {
            !w.is_empty()
                && w.len() >= 3
                && !STOP_WORDS.contains(&w.as_str())
                && w.chars().all(|c| c.is_alphanumeric())
        })
        .map(|w| lemmatize(&w))
        .collect()
}

/// Simple suffix-stripping lemmatization.
fn lemmatize(word: &str) -> String {
    if word.ends_with("ing") && word.len() > 6 {
        return word[..word.len() - 3].to_string();
    }
    if word.ends_with("ed") && word.len() > 5 {
        return word[..word.len() - 2].to_string();
    }
    if word.ends_with("ies") && word.len() > 5 {
        return format!("{}y", &word[..word.len() - 3]);
    }
    if word.ends_with('s') && !word.ends_with("ss") && word.len() > 4 {
        return word[..word.len() - 1].to_string();
    }
    word.to_string()
}

/// Compute term frequencies.
fn term_frequencies(tokens: &[String]) -> HashMap<String, usize> {
    let mut freq: HashMap<String, usize> = HashMap::new();
    for t in tokens {
        *freq.entry(t.clone()).or_insert(0) += 1;
    }
    freq
}

/// Main preprocessing pipeline.
fn preprocess(url: &str, html: &str) -> TokenizedContent {
    let raw_text = extract_visible_text(html);
    let tokens = tokenize_and_lemmatize(&raw_text);
    let frequencies = term_frequencies(&tokens);
    TokenizedContent {
        url: url.to_string(),
        tokens,
        frequencies,
    }
}

// ── SmartModule Map Function ──────────────────────────────────────────────────

#[smartmodule(map)]
pub fn map(record: &SmartModuleRecord) -> Result<(Option<RecordData>, RecordData)> {
    // Parse the raw page
    let raw_page: RawPage = serde_json::from_slice(record.value.as_ref())
        .map_err(|e| eyre!("Failed to parse RawPage: {}", e))?;

    // Preprocess the HTML
    let tokens = preprocess(&raw_page.url, &raw_page.html);

    // Create the bundle
    let bundle = PreprocessedBundle { raw_page, tokens };

    // Serialize and return
    let output = serde_json::to_vec(&bundle)
        .map_err(|e| eyre!("Failed to serialize bundle: {}", e))?;

    Ok((record.key.clone(), RecordData::from(output)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_visible_text() {
        let html = "<html><head><title>Test</title></head><body><p>Hello world</p></body></html>";
        let text = extract_visible_text(html);
        assert!(text.contains("Hello"));
        assert!(text.contains("world"));
    }

    #[test]
    fn test_tokenize() {
        let text = "The quick brown fox jumps over the lazy dog";
        let tokens = tokenize_and_lemmatize(text);
        assert!(tokens.contains(&"quick".to_string()));
        assert!(tokens.contains(&"brown".to_string()));
        // "the" should be filtered as stop word
        assert!(!tokens.iter().any(|t| t == "the"));
    }

    #[test]
    fn test_lemmatize() {
        assert_eq!(lemmatize("running"), "runn");
        assert_eq!(lemmatize("jumped"), "jump");
        assert_eq!(lemmatize("cities"), "city");
        assert_eq!(lemmatize("cars"), "car");
    }
}
