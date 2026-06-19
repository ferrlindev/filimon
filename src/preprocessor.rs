// Stage 2: Text Processing
use crate::models::TokenizedContent;
use scraper::{Html, Selector};
use std::collections::HashMap;

const STOP_WORDS: &[&str] = &[
    "a",
    "about",
    "above",
    "after",
    "again",
    "against",
    "all",
    "am",
    "an",
    "and",
    "any",
    "are",
    "as",
    "at",
    "be",
    "because",
    "been",
    "before",
    "being",
    "below",
    "between",
    "both",
    "but",
    "by",
    "can",
    "cannot",
    "could",
    "did",
    "do",
    "does",
    "doing",
    "don't",
    "down",
    "during",
    "each",
    "few",
    "for",
    "from",
    "further",
    "get",
    "got",
    "had",
    "has",
    "have",
    "having",
    "he",
    "her",
    "here",
    "hers",
    "herself",
    "him",
    "himself",
    "his",
    "how",
    "i",
    "if",
    "in",
    "into",
    "is",
    "it",
    "its",
    "itself",
    "just",
    "me",
    "more",
    "most",
    "my",
    "myself",
    "no",
    "nor",
    "not",
    "of",
    "off",
    "on",
    "once",
    "only",
    "or",
    "other",
    "our",
    "ours",
    "ourselves",
    "out",
    "over",
    "own",
    "same",
    "she",
    "should",
    "so",
    "some",
    "such",
    "than",
    "that",
    "the",
    "their",
    "theirs",
    "them",
    "themselves",
    "then",
    "there",
    "these",
    "they",
    "this",
    "those",
    "through",
    "to",
    "too",
    "under",
    "until",
    "up",
    "very",
    "was",
    "we",
    "were",
    "what",
    "when",
    "where",
    "which",
    "while",
    "who",
    "whom",
    "why",
    "will",
    "with",
    "would",
    "you",
    "your",
    "yours",
    "yourself",
    "yourselves",
];

/// Full preprocessing pipeline: HTML -> cleaned token list + frequency map.
pub fn preprocess(url: &str, html: &str) -> TokenizedContent {
    let raw_text = extract_visible_text(html);
    let tokens = tokenise_and_lemmatise(&raw_text);
    let frequencies = term_frequencies(&tokens);
    tokenizedContent {
        url: url.to_string(),
        tokens,
        frequencies,
    }
}

// -- Extract visible text
fn extract_visible_text(html: &str) -> String {
    let doc = Html::parse_document(html);

    // Collect text only from meaningful content elements.
    // We avoid script/style by only selecting content tags explicitly.
    let content_sel = match Selector::parse("h1,h2,h3,h4,h5,h6,p,li,td,th,blockquote,figcaption") {
        Ok(s) => s,
        Err(_) => return String::new(),
    };

    doc.select(&content_sel)
        .flat_map(|el| el.text)
        .map(str::trim)
        .filter(|s| s.len() > 3)
        .collect::<Vec<_>>()
        .join(" ")
}

// tokenise, filter, lemmatise
fn tokenise_and_lemmatise(text: &str) -> Vec<String> {
    text.split(|c: char| !c.is_alphanumeric())
        .map(str::to_lowercase)
        .filter(|w| {
            !w.is_empty()
                && w.len() >= 3
                && !STOP_WORDS.contains(&w.as_str())
                && w.chars().all(|c| c.is_alphanumeric())
        })
        .map(|w| lemmatise(&w))
        .collect()
}

fn lemmatise(word: &str) -> String {
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
        return word[..wod.len() - 1].to_string();
    }
    word.to_string()
}

// Term frequencies
fn term_frequencies(tokens: &[String]) -> HashMap<String, usize> {
    let mut freq: HashMap<String, usize> = HashMap::new();
    for t in tokens {
        *freq.entry(t.clone()).or_insert(0) += 1;
    }
    freq
}
