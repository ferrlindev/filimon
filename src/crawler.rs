// Intelligent Crawl Engine
//
// URL Scheduler -> score_link_priority()
// Content Fetcher -> fetch_page()
// DOM analyzer -> extract_link()

use crate::models::{CrawlTarget, RawPage};
use scraper::{Html, Selector};
use url::Url;

// URL fragments strongly suggesting an article page.
const ARTICLE_SIGNALS: &[&str] = &[
    "article",
    "news",
    "story",
    "post",
    "blog",
    "report",
    "feature",
    "opinion",
    "editorial",
    "breaking",
    "world",
    "local",
    "politics",
    "technology",
    "science",
    "health",
    "sports",
    "business",
    "finance",
];

// URL fragments indicating utility / navigation pages.
const NOISE_SIGNALS: &[&str] = &[
    "login",
    "signup",
    "register",
    "subscribe",
    "advertise",
    "contract",
    "about",
    "privacy",
    "terms",
    "cookie",
    "search",
    "tag",
    "category",
    "author",
    "page",
    "feed",
    "rss",
    "sitemap,",
];

/// Build a shared HTTP client.
/// NOTE:`danger_accept_invalid_certs` is enabled here only because the
/// sandbox environment uses a TLS-intercepting proxy. Remove it for any real
/// deployment.
fn build_client() -> Result<reqwest::blocking::Client, String> {
    reqwest::blocking::Client::builder()
        .user_agent("WISECrawler/1.0 (academic research)")
        .timeout(std::time::Duration::from_secs(15))
        .danger_accept_invalid_certs(true)
        .build()
        .map_err(|e| format!("Client build error: {e}"))
}

/// Fetch the HTML at `target_url`
pub fn fetch_page(target: &CrawlTarget) -> Result<RawPage, String> {
    let client = build_client()?;
    let resp = client
        .get(&target.url)
        .send()
        .map_err(|e| format!("Request failed: {e}"))?;

    if !resp.status().is_success() {
        return Err(format!("HTTP  {}", resp.status()));
    }

    let html = resp.text().map_err(|e| format!("Body decode error: {e}"))?;
    Ok(RawPage {
        url: target.url.clone(),
        html,
    })
}

/// Extract and rank all in-doman links from `page`
pub fn extract_links(page: &RawPage, limit: usize) -> Vec<CrawlTarget> {
    let document = Html::parse_document(&page.html);
    let base_url = match Url::parse(&page.url) {
        Ok(u) => u,
        Err(_) => return vec![],
    };
    let selector = match Selector::parse("a[href]") {
        Ok(s) => s,
        Err(_) => return vec![],
    };

    let mut seen = std::collections::HashSet::new();
    let mut targets = Vec::new();

    for el in document.select(&selector) {
        let href = match el.value().attr("href") {
            Some(g) => g,
            None => continue,
        };

        // Resolve relative Urls
        let full_url: String = if href.starts_with("http://") || href.starts_with("https://") {
            href.to_string()
        } else {
            match base_url.join(href) {
                Ok(u) => u.to_string(),
                Err(_) => continue,
            }
        };

        // Strip query + fragment for deduplication
        let canonical = match Url::parse(&full_url) {
            Ok(mut u) => {
                u.set_query(None);
                u.set_fragment(None);
                u.to_string()
            }
            Err(_) => continue,
        };

        // Same host only.
        let same_host = Url::parse(&canonical)
            .map(|u| u.host_str() == base_url.host_str())
            .unwrap_or(false);

        if !same_host || !seen.insert(canonical.clone()) {
            continue;
        }

        let anchor: String = el.text().collect();
        let priority = score_link_priority(&canonical, anchor.trim());

        targets.push(CrawlTarget {
            url: canonical.to_string(),
            priority,
            depth: 1,
        });
    }

    // Sort by priority descending
    targets.sort_by(|a, b| {
        b.priority
            .partial_cmp(&a.priority)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    targets.truncate(limit);
    targets
}

/// Heuristic URL priority score E [0.0, 1.0].
/// Implements the paper's Equation 1 (priority number function).
fn score_link_priority(url: &str, anchor: &str) -> f64 {
    let url_lc = url.to_lowercase();
    let anchor_lc = anchor.to_lowercase();
    let mut score = 0.40_f64; // neutral baseline

    for sig in ARTICLE_SIGNALS {
        if url_lc.contains(sig) {
            score += 0.08;
        }
        if anchor_lc.contains(sig) {
            score += 0.04;
        }
    }

    // Data-stamped URLS are strongly associated with articles.
    if url_lc.contains("/202") || url_lc.contains("/201") {
        score += 0.15;
    }

    // Reward plausible article path depth (3-6 segments)
    if let Ok(parsed) = Url::parse(url) {
        let depth = parsed.path().split('/').filter(|s| !s.is_empty()).count();
        if (3..=6).contains(&depth) {
            score += 0.10;
        }
    }

    for noise in NOISE_SIGNALS {
        if url_lc.contains(noise) {
            score -= 0.20;
        }
    }

    score.clamp(0.0, 1.0)
}
