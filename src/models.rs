// ─────────────────────────────────────────────────────────────────────────────
// models.rs - Shared data structures for the WISE pipeline
//
// All structures are serializable for Fluvio streaming. Records flow through
// topics as JSON-encoded messages.
// ─────────────────────────────────────────────────────────────────────────────

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// A URL queued for crawling, with a computed priority score (0.0 - 1.0)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CrawlTarget {
    pub url: String,
    pub priority: f64,
    pub depth: u32, // hops from the seed URL
}

/// Raw HTML fetched from a single URL.
///
/// This is the output of Stage 1 (Crawler) and input to Stage 2 (Preprocessor).
/// Streamed to the `filimon-raw-pages` Fluvio topic.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RawPage {
    pub url: String,
    pub html: String,
}

/// Result of Stage 2: cleaned tokens + per-token frequencies.
///
/// Streamed to the `filimon-preprocessed` Fluvio topic (bundled with RawPage).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenizedContent {
    pub url: String,
    pub tokens: Vec<String>,
    pub frequencies: HashMap<String, usize>,
}

/// Bundle containing raw page and its tokenized content.
///
/// Used for streaming between Stage 2 and Stage 3 to preserve context.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PreprocessedBundle {
    pub raw_page: RawPage,
    pub tokens: TokenizedContent,
}

/// A single named entity found in the article.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NamedEntity {
    pub text: String,
    pub kind: String, // "DATE", "PERSON", "ORGANIZATION", etc.
}

/// Final structured output produced by Stage 3/4.
///
/// Streamed to the `filimon-extracted` and `filimon-articles` Fluvio topics.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtractedArticle {
    pub url: String,
    pub title: String,
    pub author: Option<String>,
    pub published_date: Option<String>,
    pub body_preview: String, // first 400-500 characters of extracted body text
    pub word_count: usize,
    pub relevance_score: f64,
    pub top_keywords: Vec<String>,
    pub named_entities: Vec<NamedEntity>,
    pub inferred_category: String,
}

/// Pipeline statistics for monitoring and observability.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PipelineStats {
    pub pages_crawled: usize,
    pub pages_preprocessed: usize,
    pub articles_extracted: usize,
    pub articles_accepted: usize,
    pub articles_filtered: usize,
}

impl PipelineStats {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn acceptance_rate(&self) -> f64 {
        if self.articles_extracted == 0 {
            0.0
        } else {
            self.articles_accepted as f64 / self.articles_extracted as f64
        }
    }
}
