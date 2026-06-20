// Shared data structures for the WISE pipeline
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// A URL queued for crawling, with a computed priority score (0.0 - 1.0)
#[derive(Debug, Clone)]
pub struct CrawlTarget {
    pub url: String,
    pub priority: f64,
    pub depth: u32, // hops from the seed URL
}

/// Raw HTML fetched from a single URL.
#[derive(Debug)]
pub struct RawPage {
    pub url: String,
    pub html: String,
}

/// Result of Stage 2: cleaned tokens + per-token frequencies.
#[derive(Debug, Clone)]
pub struct TokenizedContent {
    pub url: String,
    pub tokens: Vec<String>,
    pub frequencies: HashMap<String, usize>,
}

/// A single named entity found in the article today
#[derive(Debug, Serialize, Deserialize)]
pub struct NamedEntity {
    pub text: String,
    pub kind: String, // "DATE", "PERSON_OR_ORG", etc.
}

/// Final structured output produced by Stage 4.
#[derive(Debug, Serialize, Deserialize)]
pub struct ExtractedArticle {
    pub url: String,
    pub title: String,
    pub author: Option<String>,
    pub published_date: Option<String>,
    pub body_preview: String, // first 400 characters of extracted body text
    pub word_count: usize,
    pub relevance_score: f64,
    pub top_keywords: Vec<String>,
    pub named_entities: Vec<NamedEntity>,
    pub inferred_category: String,
}
