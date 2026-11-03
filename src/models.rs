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

    /// Build a social item. The parameters mirror the WIT `item` record 1:1.
    #[allow(
        clippy::too_many_arguments,
        reason = "mirrors the eight fields of the WIT item record"
    )]
    #[cfg_attr(
        not(test),
        allow(dead_code, reason = "produced by the plugin host in Phase 3")
    )]
    pub fn social(
        id: String,
        url: String,
        title: Option<String>,
        text: String,
        author: Option<String>,
        published_ms: Option<u64>,
        platform: String,
        raw: Option<String>,
    ) -> Self {
        // RED stub: published and raw are not converted yet.
        let _ = (published_ms, raw);
        Self {
            source_type: SourceType::Social,
            platform,
            id,
            url,
            title,
            text,
            author,
            published: None,
            raw: None,
            html: None,
        }
    }
}

/// Convert WIT epoch milliseconds to RFC 3339 UTC with millisecond precision.
pub fn epoch_ms_to_rfc3339(ms: u64) -> Option<String> {
    // RED stub.
    let _ = ms;
    None
}

/// Best-effort parse of a news date string to RFC 3339 UTC.
pub fn parse_news_published(raw: Option<&str>) -> Option<String> {
    // RED stub.
    let _ = raw;
    None
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

    fn rich_social() -> SourceItem {
        SourceItem::social(
            "t3_abc".to_string(),
            "https://www.reddit.com/r/rust/comments/abc".to_string(),
            Some("Zürich 東京 🦀".to_string()),
            "lol cat 🦀".to_string(),
            Some("u1".to_string()),
            Some(1_730_000_000_123),
            "reddit".to_string(),
            Some(r#"{"score":3}"#.to_string()),
        )
    }

    fn minimal_social() -> SourceItem {
        SourceItem::social(
            "x".to_string(),
            "https://r/x".to_string(),
            None,
            String::new(),
            None,
            None,
            "reddit".to_string(),
            None,
        )
    }

    fn round_trip(item: &SourceItem) -> SourceItem {
        serde_json::from_str(&serde_json::to_string(item).unwrap()).unwrap()
    }

    #[test]
    fn source_item_round_trip() {
        let news = SourceItem::news(
            "https://example.com/news".to_string(),
            "<p>Zürich 東京 🦀</p>".to_string(),
        );
        assert_eq!(round_trip(&news), news);

        let social = rich_social();
        let back = round_trip(&social);
        assert_eq!(back, social);
        // Text is carried byte-identical, with no normalisation or trimming.
        assert_eq!(back.title.as_deref(), Some("Zürich 東京 🦀"));
        assert_eq!(back.text, "lol cat 🦀");
        assert_eq!(back.raw, Some(serde_json::json!({"score": 3})));
        assert_eq!(back.published.as_deref(), Some("2024-10-27T03:33:20.123Z"));

        let minimal = minimal_social();
        assert_eq!(round_trip(&minimal), minimal);
    }

    /// Field names and option-ness of the WIT `item` record, in order.
    fn wit_item_fields() -> Vec<(String, bool)> {
        let wit = include_str!("../wit/types.wit");
        let open = "record item {";
        let start = wit.find(open).expect("record item in wit/types.wit") + open.len();
        let body = &wit[start..];
        let body = &body[..body.find('}').expect("end of record item")];
        body.lines()
            .map(str::trim)
            .filter(|line| !line.is_empty() && !line.starts_with("///"))
            .filter_map(|line| {
                let (name, ty) = line.split_once(':')?;
                Some((name.trim().replace('-', "_"), ty.trim().starts_with("option<")))
            })
            .collect()
    }

    fn keys(item: &SourceItem) -> std::collections::BTreeSet<String> {
        serde_json::to_value(item)
            .unwrap()
            .as_object()
            .unwrap()
            .keys()
            .cloned()
            .collect()
    }

    #[test]
    fn source_item_matches_wit_item() {
        let fields = wit_item_fields();
        let names: Vec<&str> = fields.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(
            names,
            ["id", "url", "title", "text", "author", "published", "platform", "raw"]
        );

        let mut expected: std::collections::BTreeSet<String> =
            names.iter().map(|n| (*n).to_string()).collect();
        expected.insert("source_type".to_string());
        assert_eq!(keys(&rich_social()), expected);

        let mut news_expected = expected.clone();
        news_expected.insert("html".to_string());
        let news = SourceItem::news("https://example.com/a".to_string(), String::new());
        assert_eq!(keys(&news), news_expected);

        // The WIT option fields are exactly the fields that may be null.
        let wit_optional: std::collections::BTreeSet<String> = fields
            .iter()
            .filter(|(_, optional)| *optional)
            .map(|(n, _)| n.clone())
            .collect();
        let value = serde_json::to_value(minimal_social()).unwrap();
        let null_fields: std::collections::BTreeSet<String> = value
            .as_object()
            .unwrap()
            .iter()
            .filter(|(_, v)| v.is_null())
            .map(|(k, _)| k.clone())
            .collect();
        assert_eq!(null_fields, wit_optional);
        assert_eq!(
            wit_optional,
            ["author", "published", "raw", "title"]
                .iter()
                .map(|s| (*s).to_string())
                .collect()
        );
    }

    #[test]
    fn minimal_social_json_defaults() {
        let item: SourceItem = serde_json::from_str(
            r#"{"source_type":"social","platform":"reddit","id":"x","url":"https://r/x"}"#,
        )
        .unwrap();
        assert_eq!(item.source_type, SourceType::Social);
        assert_eq!(item.text, "");
        assert!(item.title.is_none());
        assert!(item.author.is_none());
        assert!(item.published.is_none());
        assert!(item.raw.is_none());
        assert!(item.html.is_none());
    }

    #[test]
    fn epoch_ms_to_rfc3339_converts() {
        assert_eq!(
            epoch_ms_to_rfc3339(1_730_000_000_123).as_deref(),
            Some("2024-10-27T03:33:20.123Z")
        );
        assert_eq!(
            epoch_ms_to_rfc3339(0).as_deref(),
            Some("1970-01-01T00:00:00.000Z")
        );
        assert_eq!(epoch_ms_to_rfc3339(u64::MAX), None);
    }

    #[test]
    fn parse_news_published_best_effort() {
        let cases = [
            ("2024-05-01T10:00:00+01:00", "2024-05-01T09:00:00Z"),
            ("Wed, 01 May 2024 10:00:00 +0100", "2024-05-01T09:00:00Z"),
            ("2024-05-01T10:00:00", "2024-05-01T10:00:00Z"),
            ("2024-05-01", "2024-05-01T00:00:00Z"),
            ("  2024-05-01T10:00:00+01:00 \n", "2024-05-01T09:00:00Z"),
        ];
        for (input, expected) in cases {
            assert_eq!(
                parse_news_published(Some(input)).as_deref(),
                Some(expected),
                "{input:?}"
            );
        }
        assert_eq!(parse_news_published(Some("yesterday")), None);
        assert_eq!(parse_news_published(Some("")), None);
        assert_eq!(parse_news_published(None), None);
    }

    #[test]
    fn platform_from_url_rules() {
        assert_eq!(platform_from_url("https://www.bbc.co.uk/news/x"), "bbc.co.uk");
        assert_eq!(platform_from_url("https://example.com:8080/a"), "example.com");
        assert_eq!(
            platform_from_url("https://news.ycombinator.com/item"),
            "news.ycombinator.com"
        );
        assert_eq!(platform_from_url("file:///tmp/a.html"), "local");
        assert_eq!(
            platform_from_url("file://examples/fixtures/news-article.html"),
            "local"
        );
        assert_eq!(platform_from_url("not a url"), "local");
    }

    fn article(published_date: Option<&str>, relevance_score: f64) -> ExtractedArticle {
        ExtractedArticle {
            url: "https://www.bbc.co.uk/news/x".to_string(),
            title: "Policy report".to_string(),
            author: Some("Jane Reporter".to_string()),
            published_date: published_date.map(str::to_string),
            body_preview: "The parliament passed the bill.".to_string(),
            word_count: 5,
            relevance_score,
            top_keywords: vec!["parliament".to_string(), "policy".to_string()],
            named_entities: vec![],
            inferred_category: "Politics".to_string(),
        }
    }

    #[test]
    fn from_article_maps_fields() {
        let record = OutputRecord::from_article(article(Some("2024-05-01T10:00:00+01:00"), 0.7));
        assert_eq!(record.source_type, SourceType::News);
        assert_eq!(record.platform, "bbc.co.uk");
        assert_eq!(record.id, record.url);
        assert_eq!(record.title.as_deref(), Some("Policy report"));
        assert_eq!(record.text, "The parliament passed the bill.");
        assert_eq!(record.author.as_deref(), Some("Jane Reporter"));
        assert_eq!(record.published.as_deref(), Some("2024-05-01T09:00:00Z"));
        let meta = record.article.as_ref().expect("article block");
        assert!((meta.relevance_score - 0.7).abs() < f64::EPSILON);
        assert_eq!(meta.top_keywords, ["parliament", "policy"]);
        assert_eq!(meta.inferred_category, "Politics");
        assert_eq!(meta.word_count, 5);
        assert_eq!(meta.published_raw.as_deref(), Some("2024-05-01T10:00:00+01:00"));

        let unparsed = OutputRecord::from_article(article(Some("last Tuesday"), 0.7));
        assert_eq!(unparsed.published, None);
        assert_eq!(
            unparsed.article.unwrap().published_raw.as_deref(),
            Some("last Tuesday")
        );
    }

    #[test]
    fn from_article_keeps_boundary_relevance() {
        let record = OutputRecord::from_article(article(None, 0.05));
        let meta = record.article.expect("article block");
        assert!((meta.relevance_score - 0.05).abs() < f64::EPSILON);
    }

    #[test]
    fn social_raw_parsing() {
        assert_eq!(rich_social().raw, Some(serde_json::json!({"score": 3})));
        assert_eq!(
            rich_social().published.as_deref(),
            Some("2024-10-27T03:33:20.123Z")
        );

        let not_json = SourceItem::social(
            "x".to_string(),
            "https://r/x".to_string(),
            None,
            String::new(),
            None,
            None,
            "reddit".to_string(),
            Some("not json".to_string()),
        );
        assert_eq!(
            not_json.raw,
            Some(serde_json::Value::String("not json".to_string()))
        );

        assert_eq!(minimal_social().raw, None);
    }
}
