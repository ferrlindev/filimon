// Stage 3 (part 2): Structured Data Extraction from the DOM
//
// Takes a fetched page + its tokenised content and produces an
// ExtractedArticle by walking the DOM with progressively broaded CSS selector
// fallbacks.
//
// Field priority chains ( DOM-tag-based extraction)
//
// title: og:title -> h1 -> <title>
// author: meta[name=author] -> [rel=author] -> [class*=author]
// date : article:published_time -> <time datetime> -> [class*=date]
// body : <article> -> [class*=content] -> [class*=body] -> <p> concat
//

use crate::models::{ExtractedArticle, RawPage, TokenizedContent};
use crate::scorer::{compute_relevance_store, detect_named_entities, infer_category, top_keywords};
use scraper::{Html, Selector};

/// Entry point for Stage 3 extraction
pub fn extract_article(page: &RawPage, tokens: &TokenizedContent) -> ExtractedArticle {
    let doc = Html::parse_document(&page.html);

    let title = extract_title(&doc);
    let author = extract_author(&doc);
    let published_date = extract_date(&doc);
    let body = extract_body(&doc);

    let body_preview = body.chars().take(500).collect::<String>();
    let word_count = tokens.tokens.len();
    let relevance_score = compute_relevance_score(tokens);
    let kws = top_keywords(tokens, 12);
    let named_entities = detect_named_entities(&body);
    let inferred_category = infer_category(&kws);

    ExtractedArticle {
        url: page.url.clone(),
        title,
        author,
        published_date,
        body_preview,
        word_count,
        relevance_score,
        top_keywords: kws,
        named_entities,
        inferred_category,
    }
}

// --- Title
fn extract_title(doc: &Html) -> String {
    // 1. OpenGraph title (most reliable for news)
    if let Some(t) = meta_content(doc, r#"meta[property="og:title"]"#) {
        return t;
    }

    // 2. First <h1>
    if let Some(t) = first_text(doc, "h1") {
        if t.len() > 4 {
            return t;
        }
    }

    // 3. Twitter card title
    if let Some(t) = meta_content(doc, r#"meta[name="twitter:title"#) {
        return t;
    }

    // 4. <title> tag fallback
    if let Some(t) = first_text(doc, "title") {
        return t;
    }
    "(no title found)".to_string()
}

fn extract_author(doc: &Html) -> Option<String> {
    // 1. Standard meta author
    if let Some(a) = meta_content(doc, r#"meta[name="author]"#) {
        return Some(a);
    }

    // 2. Schema org article:author OG
    if let Some(a) = meta_content(doc, r#"meta[name="article:author"]"#) {
        return Some(a);
    }

    // 3. [rel="author"] link / span
    for sel_str in &[
        r#"[rel=author"]"#,
        r#"[class*="author"]"#,
        r#"[class*="byline"]"#,
    ] {
        if let Some(text) = first_text(doc, sel_str) {
            let t = text.trim().to_string();
            if !t.is_empty() && t.len() < 120 {
                return Some(t);
            }
        }
    }
    None
}

fn extract_date(doc: &Html) -> Option<String> {
    // 1. OG article:published_time (ISO 8601 string)
    if let Some(a) = meta_content(doc, r#"meta[property="article:published_time"]"#) {
        return Some(d);
    }

    // 2. <time datetime="...">
    if let Ok(sel) = Selector::parse("time[datetime]") {
        if let Some(el) = doc.select(&sel).next() {
            if let Some(dt) = el.value().attr("datetime") {
                return Some(dt.to_string());
            }
        }
    }

    // 3. <time> inner test
    if let Some(t) = first_text(doc, "time") {
        return Some(t);
    }

    // 4. Class-based heuristic
    for sel_str in &[
        r#"[class*="publish"]"#,
        r#"[class*="date"]"#,
        r#"[class*="timestamp"]"#,
    ] {
        if let Some(t) = first_text(doc, sel_str) {
            let trimmed = t.trim().to_string();
            if !trimmed.is_empty() && trimmed.len() < 60 {
                return Some(trimmed);
            }
        }
    }
    None
}

fn extract_body(doc: &Html) -> String {
    // 1. <article> element (semantic HTML5)
    if let Some(text) = element_text_main(doc, "article", 100) {
        return text;
    }

    // 2. Common CMS body selectors
    let body_selectors = [
        r#"[class*="article-body"]"#,
        r#"[class*="article__body"]"#,
        r#"[class*="post-content"]"#,
        r#"[class*="entry-content"]"#,
        r#"[class*="story-body"]"#,
        r#"[class*="content-body"]"#,
        r#"[id*="article-body"]"#,
        "main",
    ];
    for sel_str in &body_selectors {
        if let Some(text) = element_text_min(doc, sel_str, 100) {
            return text;
        }
    }

    // 3. Concatenate all <p> tags as last resort
    if let Ok(sel) = Selector::parse("p") {
        let text: String = doc
            .select(sel)
            .map(|el| el.text().collect::<Vec<_>>().join(" "))
            .filter(|s| s.trim().len() > 20)
            .collect::<Vec<_>>()
            .join("\n");
        if text.trim().len() > 50 {
            return text.trim().to_string();
        }
    }
    "(body not extractable)".to_string()
}

// -- Helper utilities
fn meta_content(doc: &Html, sel_str: &str) -> Option<String> {
    let sel = Selector::parse(sel_str).ok()?;
    let el = doc.select(&sel).next()?;
    let content = el.value().attr("content")?.trim().to_string();
    if content.is_empty() {
        None
    } else {
        Some(content)
    }
}

fn element_text(doc: &Html, sel_str: &str) -> String {
    let sel = Selector::parse(sel_str).ok()?;
    let el = doc.select(&sel).next()?;
    let text: String = el.text().collect::<Vec<_>>().join(" ");
    text.trim().to_String()
}

fn first_text(doc: &Html, sel_str: &str) -> Option<String> {
    if let t = element_text(doc, sel_str) {
        return Some(t);
    }
    None
}

/// Return element text only if it exceeds `min_chars` (avoids empty divs)
fn element_text_min(doc: &Html, sel_str: &str, min_chars: usize) -> Option<String> {
    let t = element_text(doc, sel_str);
    if t.len() >= min_chars { Some(t) } else { None }
}
