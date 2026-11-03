// ─────────────────────────────────────────────────────────────────────────────
// map-extract SmartModule
//
// This SmartModule performs Stage 3 of the WISE pipeline:
// - Extracts title, author, date, body from HTML
// - Computes relevance score
// - Extracts top keywords
// - Infers article category
//
// Note: NER is not performed in the SmartModule due to model size constraints.
// NER should be applied by the consuming application if needed.
//
// Input: PreprocessedBundle { raw_page, tokens }
// Output: ExtractedArticle
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

// ── Topic Terms for Relevance Scoring ─────────────────────────────────────────

const TOPIC_TERMS: &[&str] = &[
    // Politics / governance
    "election", "government", "parliament", "policy", "president",
    "minister", "vote", "democrat", "republican", "legislat",
    "law", "court", "bill",
    // Economics / business
    "economy", "market", "gdp", "inflation", "stock", "trade",
    "bank", "invest", "merger", "acquisit", "revenue", "profit", "startup",
    // Technology
    "technolog", "software", "artifici", "intellig", "cybersecur",
    "digital", "platform", "app", "cloud", "chip", "robot", "data",
    // Health / science
    "health", "vaccine", "virus", "cancer", "research", "scientist",
    "climate", "environment", "energy", "nuclear", "space", "discoveri",
    // General news signals
    "report", "survey", "announc", "conflict", "war", "crisis",
    "disaster", "accid", "murder", "arrest", "trial", "sentenc",
];

// ── Extraction Functions ──────────────────────────────────────────────────────

/// Extract title from HTML using multiple strategies.
fn extract_title(html: &str) -> String {
    // Try og:title
    if let Some(title) = extract_meta_content(html, "og:title") {
        if !title.is_empty() {
            return title;
        }
    }

    // Try first h1
    if let Some(h1) = extract_tag_content(html, "h1") {
        if h1.len() > 4 {
            return h1;
        }
    }

    // Try title tag
    if let Some(title) = extract_tag_content(html, "title") {
        if !title.is_empty() {
            return title;
        }
    }

    "(no title found)".to_string()
}

/// Extract author from HTML.
fn extract_author(html: &str) -> Option<String> {
    // Try meta author
    if let Some(author) = extract_meta_content(html, "author") {
        if !author.is_empty() && author.len() < 120 {
            return Some(author);
        }
    }

    // Try article:author
    if let Some(author) = extract_meta_content(html, "article:author") {
        if !author.is_empty() && author.len() < 120 {
            return Some(author);
        }
    }

    None
}

/// Extract publication date from HTML.
fn extract_date(html: &str) -> Option<String> {
    // Try article:published_time
    if let Some(date) = extract_meta_content(html, "article:published_time") {
        if !date.is_empty() {
            return Some(date);
        }
    }

    // Try time tag datetime attribute
    if let Some(datetime) = extract_attribute(html, "time", "datetime") {
        if !datetime.is_empty() {
            return Some(datetime);
        }
    }

    None
}

/// Extract body preview from HTML.
fn extract_body(html: &str) -> String {
    // Try article tag
    if let Some(body) = extract_tag_content(html, "article") {
        if body.len() >= 100 {
            return clean_text(&body);
        }
    }

    // Try main tag
    if let Some(body) = extract_tag_content(html, "main") {
        if body.len() >= 100 {
            return clean_text(&body);
        }
    }

    // Fallback: concatenate p tags
    let mut paragraphs = Vec::new();
    let html_lower = html.to_lowercase();
    let mut pos = 0;

    while let Some(start) = html_lower[pos..].find("<p") {
        let start_pos = pos + start;
        if let Some(end) = html_lower[start_pos..].find("</p>") {
            let end_pos = start_pos + end + 4;
            let content = clean_html(&html[start_pos..end_pos]);
            if content.len() > 20 {
                paragraphs.push(content);
            }
            pos = end_pos;
        } else {
            break;
        }
    }

    if !paragraphs.is_empty() {
        return paragraphs.join("\n");
    }

    "(body not extractable)".to_string()
}

// ── Helper Functions ──────────────────────────────────────────────────────────

/// Extract content attribute from a meta tag.
fn extract_meta_content(html: &str, name: &str) -> Option<String> {
    let html_lower = html.to_lowercase();
    
    // Search for meta tag with property or name attribute
    let patterns = [
        format!(r#"property="{}""#, name),
        format!(r#"name="{}""#, name),
        format!(r#"property='{}'"#, name),
        format!(r#"name='{}'"#, name),
    ];

    for pattern in &patterns {
        if let Some(pos) = html_lower.find(&pattern.to_lowercase()) {
            // Find the content attribute
            let meta_start = html_lower[..pos].rfind('<').unwrap_or(0);
            let meta_end = html_lower[pos..].find('>').map(|p| pos + p + 1).unwrap_or(html.len());
            let meta_tag = &html[meta_start..meta_end];
            
            if let Some(content) = extract_attribute_from_tag(meta_tag, "content") {
                return Some(content);
            }
        }
    }

    None
}

/// Extract content from a specific HTML tag.
fn extract_tag_content(html: &str, tag: &str) -> Option<String> {
    let html_lower = html.to_lowercase();
    let open_tag = format!("<{}", tag);
    let close_tag = format!("</{}>", tag);

    if let Some(start) = html_lower.find(&open_tag) {
        let content_start = html[start..].find('>').map(|p| start + p + 1)?;
        if let Some(end) = html_lower[content_start..].find(&close_tag) {
            let content = &html[content_start..content_start + end];
            return Some(clean_html(content));
        }
    }

    None
}

/// Extract an attribute value from an HTML tag.
fn extract_attribute(html: &str, tag: &str, attr: &str) -> Option<String> {
    let html_lower = html.to_lowercase();
    let open_tag = format!("<{}", tag);

    if let Some(start) = html_lower.find(&open_tag) {
        let tag_end = html[start..].find('>').map(|p| start + p)?;
        let tag_content = &html[start..tag_end];
        return extract_attribute_from_tag(tag_content, attr);
    }

    None
}

/// Extract an attribute value from a tag string.
fn extract_attribute_from_tag(tag: &str, attr: &str) -> Option<String> {
    let tag_lower = tag.to_lowercase();
    let patterns = [
        format!(r#"{}=""#, attr),
        format!(r#"{}='"#, attr),
    ];

    for pattern in &patterns {
        if let Some(pos) = tag_lower.find(pattern.as_str()) {
            let value_start = pos + pattern.len();
            let quote_char = if pattern.ends_with('"') { '"' } else { '\'' };
            if let Some(value_end) = tag[value_start..].find(quote_char) {
                return Some(tag[value_start..value_start + value_end].to_string());
            }
        }
    }

    None
}

/// Clean HTML tags from text.
fn clean_html(html: &str) -> String {
    let mut result = String::new();
    let mut in_tag = false;

    for c in html.chars() {
        if c == '<' {
            in_tag = true;
        } else if c == '>' {
            in_tag = false;
            result.push(' ');
        } else if !in_tag {
            result.push(c);
        }
    }

    clean_text(&result)
}

/// Clean and normalize text.
fn clean_text(text: &str) -> String {
    text.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .trim()
        .to_string()
}

// ── Scoring Functions ─────────────────────────────────────────────────────────

/// Compute relevance score based on topic term frequency.
fn compute_relevance_score(tokens: &TokenizedContent) -> f64 {
    let n = tokens.tokens.len() as f64;
    if n == 0.0 {
        return 0.0;
    }

    // Component 1: topic-term TF ratio (60% weight)
    let topic_hits: usize = TOPIC_TERMS
        .iter()
        .map(|term| {
            tokens
                .frequencies
                .iter()
                .filter(|(k, _)| k.starts_with(term))
                .map(|(_, v)| v)
                .sum::<usize>()
        })
        .sum();

    let tf_ratio = (topic_hits as f64 / n).min(1.0);
    let component1 = tf_ratio * 0.60;

    // Component 2: content richness (25% weight)
    let richness = (n / 300.0).min(1.0).sqrt();
    let component2 = richness * 0.25;

    // Component 3: vocabulary diversity (15% weight)
    let diversity = (tokens.frequencies.len() as f64 / n).min(1.0);
    let component3 = diversity * 0.15;

    (component1 + component2 + component3).min(1.0)
}

/// Extract top N keywords by frequency.
fn top_keywords(tokens: &TokenizedContent, n: usize) -> Vec<String> {
    let mut pairs: Vec<(&String, &usize)> = tokens.frequencies.iter().collect();
    pairs.sort_by(|a, b| b.1.cmp(a.1).then(a.0.cmp(b.0)));
    pairs.into_iter().take(n).map(|(k, _)| k.clone()).collect()
}

/// Infer article category from keywords.
fn infer_category(keywords: &[String]) -> String {
    let joined = keywords.join(" ").to_lowercase();

    let rules: &[(&[&str], &str)] = &[
        (
            &["elect", "parliament", "vote", "democrat", "republican", "legislat", "polic"],
            "Politics",
        ),
        (
            &["market", "gdp", "inflat", "stock", "trade", "invest", "bank", "revenue", "startup"],
            "Business",
        ),
        (
            &["technolog", "softwar", "virus", "cancer", "hospital", "diseas", "medic"],
            "Health & Science",
        ),
        (
            &["health", "environment", "energi", "emiss", "renewabl"],
            "Environment",
        ),
        (
            &["match", "goal", "tournament", "footbal", "sport", "athlete"],
            "Sports",
        ),
    ];

    for (signals, label) in rules {
        if signals.iter().any(|s| joined.contains(s)) {
            return label.to_string();
        }
    }

    "General News".to_string()
}

// ── SmartModule Map Function ──────────────────────────────────────────────────

#[smartmodule(map)]
pub fn map(record: &SmartModuleRecord) -> Result<(Option<RecordData>, RecordData)> {
    // Parse the preprocessed bundle
    let bundle: PreprocessedBundle = serde_json::from_slice(record.value.as_ref())
        .map_err(|e| eyre!("Failed to parse PreprocessedBundle: {}", e))?;

    // Extract article components
    let title = extract_title(&bundle.raw_page.html);
    let author = extract_author(&bundle.raw_page.html);
    let published_date = extract_date(&bundle.raw_page.html);
    let body = extract_body(&bundle.raw_page.html);
    let body_preview: String = body.chars().take(500).collect();

    // Compute scores and keywords
    let relevance_score = compute_relevance_score(&bundle.tokens);
    let keywords = top_keywords(&bundle.tokens, 12);
    let inferred_category = infer_category(&keywords);

    // Create the extracted article
    let article = ExtractedArticle {
        url: bundle.raw_page.url,
        title,
        author,
        published_date,
        body_preview,
        word_count: bundle.tokens.tokens.len(),
        relevance_score,
        top_keywords: keywords,
        named_entities: vec![], // NER not performed in SmartModule
        inferred_category,
    };

    // Serialize and return
    let output = serde_json::to_vec(&article)
        .map_err(|e| eyre!("Failed to serialize article: {}", e))?;

    Ok((record.key.clone(), RecordData::from(output)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_title() {
        let html = r#"<html><head><meta property="og:title" content="Test Title"></head></html>"#;
        assert_eq!(extract_title(html), "Test Title");
    }

    #[test]
    fn test_extract_title_h1_fallback() {
        let html = r#"<html><body><h1>Article Title</h1></body></html>"#;
        assert_eq!(extract_title(html), "Article Title");
    }

    #[test]
    fn test_infer_category() {
        let keywords = vec!["election".to_string(), "vote".to_string(), "democrat".to_string()];
        assert_eq!(infer_category(&keywords), "Politics");

        let keywords = vec!["market".to_string(), "stock".to_string(), "trade".to_string()];
        assert_eq!(infer_category(&keywords), "Business");
    }
}
