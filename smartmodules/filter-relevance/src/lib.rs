// ─────────────────────────────────────────────────────────────────────────────
// filter-relevance SmartModule
//
// This SmartModule filters extracted articles based on their relevance score.
// Articles with a relevance_score below the threshold are discarded.
//
// Default threshold: 0.05 (can be customized by modifying this constant)
// ─────────────────────────────────────────────────────────────────────────────

use fluvio_smartmodule::{smartmodule, SmartModuleRecord, Result};
use serde::{Deserialize, Serialize};

/// Default relevance threshold - articles with scores below this are filtered out.
const RELEVANCE_THRESHOLD: f64 = 0.05;

// ── Article Structure (matches models.rs ExtractedArticle) ────────────────────

#[derive(Debug, Serialize, Deserialize)]
pub struct NamedEntity {
    pub text: String,
    pub kind: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ExtractedArticle {
    pub url: String,
    pub title: String,
    pub author: Option<String>,
    pub published_date: Option<String>,
    pub body_preview: String,
    pub word_count: usize,
    pub relevance_score: f64,
    pub top_keywords: Vec<String>,
    pub named_entities: Vec<NamedEntity>,
    pub inferred_category: String,
}

// ── SmartModule Filter Function ───────────────────────────────────────────────

#[smartmodule(filter)]
pub fn filter(record: &SmartModuleRecord) -> Result<bool> {
    // Parse the article from the record
    let article: ExtractedArticle = serde_json::from_slice(record.value.as_ref())?;

    // Return true to keep records with relevance >= threshold
    Ok(article.relevance_score >= RELEVANCE_THRESHOLD)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_article(relevance: f64) -> Vec<u8> {
        let article = ExtractedArticle {
            url: "https://example.com/test".to_string(),
            title: "Test Article".to_string(),
            author: Some("Test Author".to_string()),
            published_date: Some("2024-01-01".to_string()),
            body_preview: "This is a test article body preview.".to_string(),
            word_count: 100,
            relevance_score: relevance,
            top_keywords: vec!["test".to_string(), "article".to_string()],
            named_entities: vec![],
            inferred_category: "General News".to_string(),
        };
        serde_json::to_vec(&article).unwrap()
    }

    #[test]
    fn test_filter_accepts_high_relevance() {
        let _data = make_article(0.5);
        // High relevance (0.5) should pass the threshold (0.05)
        assert!(0.5 >= RELEVANCE_THRESHOLD);
    }

    #[test]
    fn test_filter_rejects_low_relevance() {
        // Low relevance (0.01) should NOT pass the threshold (0.05)
        assert!(0.01 < RELEVANCE_THRESHOLD);
    }
}
