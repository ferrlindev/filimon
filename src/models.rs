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

/// Final structured output of the news chain.
///
/// The `map-extract` module produces it, `filter-relevance` keeps it or drops
/// it, and the chain delivers it to `fili consume` from the `filimon-raw-pages`
/// stream.
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

// ── Unified Envelope ──────────────────────────────────────────────────────────

/// Which kind of source produced an item.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SourceType {
    News,
    Social,
}

/// The unified envelope for every item that enters the pipeline.
///
/// Its fields line up with the WIT `item` record in `wit/types.wit`, plus
/// `source_type` and the news-only `html`. News items travel on the
/// `filimon-raw-pages` Fluvio topic, where the chain modules read them as
/// `RawPage { url, html }` and ignores the other fields.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SourceItem {
    pub source_type: SourceType,
    pub platform: String,
    pub id: String,
    pub url: String,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub text: String,
    #[serde(default)]
    pub author: Option<String>,
    /// Publication time as RFC 3339 (UTC).
    #[serde(default)]
    pub published: Option<String>,
    /// Platform metadata as a JSON document.
    #[serde(default)]
    pub raw: Option<serde_json::Value>,
    /// Page HTML, news only. It is skipped when None, because a null html
    /// halts map-preprocess: its `RawPage.html` is a `String`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub html: Option<String>,
}

impl SourceItem {
    /// Build a news item from a fetched page. `html` is always set.
    pub fn news(url: String, html: String) -> Self {
        Self {
            source_type: SourceType::News,
            platform: platform_from_url(&url),
            id: url.clone(),
            url,
            title: None,
            text: String::new(),
            author: None,
            published: None,
            raw: None,
            html: Some(html),
        }
    }
}

/// Short platform name for a URL: the host without one leading `www.`.
///
/// File URLs, hostless URLs and unparsable URLs give `local`.
pub fn platform_from_url(url: &str) -> String {
    let Ok(parsed) = url::Url::parse(url) else {
        return "local".to_string();
    };
    if parsed.scheme() == "file" {
        return "local".to_string();
    }
    match parsed.host_str() {
        Some(host) => host.strip_prefix("www.").unwrap_or(host).to_string(),
        None => "local".to_string(),
    }
}

// ── Output Record ─────────────────────────────────────────────────────────────

/// News-only metadata from the `map-extract` module.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArticleMeta {
    pub relevance_score: f64,
    pub top_keywords: Vec<String>,
    pub inferred_category: String,
    pub word_count: usize,
    /// The date string exactly as map-extract found it.
    pub published_raw: Option<String>,
}

/// One record of the consumer's JSON output.
///
/// It deliberately has no html field, so page HTML cannot reach the output.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutputRecord {
    pub source_type: SourceType,
    pub platform: String,
    pub id: String,
    pub url: String,
    pub title: Option<String>,
    pub text: String,
    pub author: Option<String>,
    /// Publication time as RFC 3339 (UTC), or null.
    pub published: Option<String>,
    pub raw: Option<serde_json::Value>,
    pub entities: Vec<NamedEntity>,
    /// Some for news, None for social.
    pub article: Option<ArticleMeta>,
}

impl OutputRecord {
    /// Map an article from the news chain to an output record.
    pub fn from_article(a: ExtractedArticle) -> Self {
        Self {
            source_type: SourceType::News,
            platform: platform_from_url(&a.url),
            id: a.url.clone(),
            url: a.url,
            title: Some(a.title),
            text: a.body_preview,
            author: a.author,
            published: None,
            raw: None,
            entities: a.named_entities,
            article: Some(ArticleMeta {
                relevance_score: a.relevance_score,
                top_keywords: a.top_keywords,
                inferred_category: a.inferred_category,
                word_count: a.word_count,
                published_raw: a.published_date,
            }),
        }
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    /// Mirrors the `RawPage` struct the chain modules deserialize.
    #[derive(Debug, Deserialize)]
    struct SmRawPage {
        url: String,
        html: String,
    }

    #[test]
    fn news_item_is_rawpage_compatible() {
        let item = SourceItem::news(
            "https://example.com/news".to_string(),
            "<p>hello</p>".to_string(),
        );
        let json = serde_json::to_string(&item).unwrap();
        let page: SmRawPage = serde_json::from_str(&json).unwrap();
        assert_eq!(page.url, "https://example.com/news");
        assert_eq!(page.html, "<p>hello</p>");

        // An empty file still gives a string html, never null.
        let empty = SourceItem::news("file://a.html".to_string(), String::new());
        let json = serde_json::to_string(&empty).unwrap();
        assert!(json.contains(r#""html":"""#), "{json}");
        assert!(!json.contains(r#""html":null"#), "{json}");

        // An item without html has no html key at all.
        let mut no_html = item;
        no_html.html = None;
        let value = serde_json::to_value(&no_html).unwrap();
        assert!(value.as_object().unwrap().get("html").is_none(), "{value}");
    }
}
