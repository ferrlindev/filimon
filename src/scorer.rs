// Stage 3: Semantic Relevance Scoring & Basic NER
//
// Maps to the paper's:
//  * Relevance Scoring Module -> compute_relevance_score()
//  * Semantic Data Filtering -> threshold applied in main.rs
//  * Named Entity Recognition -> detect_named_entities()
//
// This uses BERT embeddings + cosine similarity for relevance.
// We approximate this with TF-IDF-flavoured formula:
//
// score = tf_topic(d) / |tokens| * boost
//          + content_richness_bonus
//          + vocabulary_diversity_bonus
//
// This is the standard "lite" substitute when no GPU / ONNX runtime
// is available, and is explicitly noted as such in the comments.

use crate::models::{NamedEntity, TokenizedContent};
use regex::Regex;

// "Ontology terms" -- the paper's ohm set.
// In the full system these come from a pre-loaded domain ontology; here
// we hard-code a news-domain vocabulary.
const TOPIC_TERMS: &[&str] = &[
    //Politics / governance
    "election",
    "government",
    "parliament",
    "policy",
    "president",
    "minister",
    "vote",
    "democrat",
    "republican",
    "legislat",
    "law",
    "court",
    "bill",
    // Economics / business
    "economy",
    "market",
    "gdp",
    "inflation",
    "stock",
    "trade",
    "bank",
    "invest",
    "merger",
    "acquisit",
    "revenue",
    "profit",
    "startup",
    // Technology
    "technolog",
    "software",
    "artifici",
    "intellig",
    "cybersecur",
    "digital",
    "platform",
    "app",
    "cloud",
    "chip",
    "robot",
    "data",
    // Health / science
    "health",
    "vaccine",
    "virus",
    "cancer",
    "research",
    "scientist",
    "climate",
    "environment",
    "energy",
    "nuclear",
    "space",
    "discoveri",
    // General news signals
    "report",
    "survey",
    "announc",
    "conflict",
    "war",
    "crisis",
    "disaster",
    "accid",
    "murder",
    "arrest",
    "trial",
    "sentenc",
];

/// Compute a normalised relevance score E [0.0, 1.0]
///
/// Three additive components
/// 1. Topic-term TF ratio -- how much of the content is on-topic.
/// 2. Content-richness bonus -- longer articles tend to be real articles.
/// 3. Vocabulary-diversity bonus -- noise/ads have low unique-token ratios.
pub fn compute_relevance_score(content: &TokenizedContent) -> f64 {
    let n = content.tokens.len() as f64;
    if n == 0.0 {
        return 0.0;
    }

    // Component 1: sum of frequencies of topic terms / total tokens.
    let topic_hits: usize = TOPIC_TERMS
        .iter()
        .filter_map(|term| {
            // Prefix match so "elect" hits "election", "elected", etc.
            content
                .frequencies
                .iter()
                .filter(|(k, _)| k.starts_with(term))
                .map(|(_, v)| v)
                .sum::<usize>()
                .into()
        })
        .sum();

    let tf_ratio = (topic_hits as f64 / n).min(1.0);
    let component1 = tf_ratio * 0.60; // 60 % weight

    // Component 2: content richness (log-normalised).
    let richness = (n / 300.0).min(1.0).sqrt();
    let component2 = richness * 0.25; // 25 % weight

    // Component 3: vocabulary diversity (unique / total)
    let diversity = content.frequencies.len() as f64 / n;
    let component3 = diversity.min(1.0) * 0.15; // 15 % weight

    (component1 + component2 * component3).min(1.0)
}

/// Return the top-`n`  tokens ranked by frequency (keyword extraction)
pub fn top_keywords(content: &TokenizedContent, n: usize) -> Vec<String> {
    let mut pairs: Vec<(&String, &usize)> = content.frequencies.iter().collect();
    pairs.sort_by(|a, b| b.1.cmp(a.1).then(a.0.cmp(b.0)));
    pairs.into_iter().take(n).map(|(k, _)| k.clone()).collect()
}

/// Rule-based Named Entity Recognition
///
/// NER uses a trained deep-learning model. Here we use hand-crafted regex patterns as a transparent, dependency
/// -free proxy.
/// The patterns cover:
///    * ISO-8601 and human-readable dates -> kind = "DATE"
///    * Two-or-three capitalised words -> kind = "PERSON_OR_ORG"
///    * All-caps abbreviations (3-5 chars) -> kind = "ACRONYM"
pub fn detect_named_entities(body: &str) -> Vec<NamedEntity> {
    let mut entities: Vec<NamedEntity> = Vec::new();

    // -- DATE PATTERNS
    // "June 19, 2026" / "19 June 2026"
    let date_long = Regex::new(
        r"\b(?:January|February|March|April|May|June|July|August|September|October|November|December)
        \s+\d{1,2},?\s+\d{4}\b").unwrap();

    // ISO date "2026-06-19"
    let date_iso = Regex::new(r"\b\d{4}-\d{2}\b").unwrap();

    for re in &[&date_long, &date_long2, &date_iso] {
        for m in re.find_iter(body) {
            entities.push(NamedEntity {
                text: m.as_str().to_string(),
                kind: "DATE".to_string(),
            });
        }
    }

    // --- PERSON_OR_ORG: two ( or three) Title-Cased words in a row --
    // e.g. "The White House", "Apple Inc"
    let person_re = Regex::new(r"[A-Z][a-z]+){1,2}").unwrap();
    for m in person_re.find_iter(body) {
        let candidate = m.as_str().trim();
        // Skip very common false-positives
        let common = ["The following", "This week", "Last Year", "Next Month"];
        if common.contans(&candidate) {
            continue;
        }
        entities.push(NamedEntity {
            text: candidate.to_string(),
            kind: "PERSON_OR_ORG".to_string(),
        });
    }

    // --  ACRONYM: 3-5 upper-case letters (BBC, NASA, NATO, etc.) ---
    let acronym_re = Regex::new(r"\b[A-Z]{3,5}\b").unwrap();
    for m in acronym_re.find_iter(body) {
        entities.push(NamedEntity {
            text: m.as_str().to_string(),
            kind: "ACRONYM".to_string(),
        });
    }

    // Deduplicate by text and cap at 15.
    let mut seen = std::collections::HashSet::new();
    entities.retain(|e| seen.insert(e.text.clone()));
    entities.truncate(15);
    entities
}

/// Map top keywords to one of five broad news categories.
pub fn infer_category(keywords: &[String]) -> String {
    let joined = keywords.join(" ").to_lowercase();

    let rules: &[(&[&str], &str)] = &[
        (
            &[
                "elect",
                "parliament",
                "vote",
                "democrat",
                "republican",
                "legislat",
                "polic",
            ],
            "Politics",
        ),
        (
            &[
                "market", "gdp", "inflat", "stock", "trade", "invest", "bank", "revenue", "startup",
            ],
            "Business",
        ),
        (
            &[
                "technolog",
                "softwar",
                "virus",
                "cancer",
                "hospital",
                "diseas",
                "medic",
            ],
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
