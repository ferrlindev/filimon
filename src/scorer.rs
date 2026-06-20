// Stage 3: Semantic Relevance Scoring + GLINER-powered NER
//
// NER is not backed by gline-rw 1.0 (GLiNER inference engine) instead of hand-craftged reges patterns. GLiner is a zero-shot BERT-like model
// that extracts named entities for any label set without task-specific find-tuning, which maps directly to the deep-learning NER
// described in Equation 10 of the WISE paper.
//
// Architecture:
//  NEREngine -- holds a loaded GLiNER<SpanNode> model; create once, reuse.
//  detect() -- runs inference, maps spans -> Vec<NamedEntity>,
//  compute_relevance_score()  / top_keywords() / infer_category() - unchanged
//
use std::path::Path;

use crate::models::{NamedEntity, TokenizedContent};
use gliner::model::GLiNER;
use gliner::model::input::text::TextInput;
use gliner::model::params::Parameters;
use gliner::model::pipeline::span::SpanMode;
use miette::{Result, miette};
use orp::params::RuntimeParameters;

// -- GLINER zero-shot entity labels
// These strings are passed verbatim to the model; span.class() returns them
// back as the entity type. Add or remove labels without retraining.
const NER_LABELS: &[&str] = &[
    "person",
    "organization",
    "location",
    "date",
    "event",
    "product",
    "law or regulation",
    "monetary value",
];

// -- Relevance topic vocabulary
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

// Wraps a loaded GLiNER span-mode model.
//
// Initialisation loads the ONNX model into memory (~200-500MB depending on the model variant) and compiles
// the ONNX graph. Create **once** per process and pass `&NerEngine` to every call site -- do not reload per article.
pub struct NerEngine {
    model: GLiNER<SpanMode>,
}

impl NerEngine {
    /// Load a GLiNER span-mode model from ONNX files on disk.
    ///
    /// # Arguments
    /// * `tokenizer_path` - path to `tokenizer.json`
    /// * `model_path` - path to `onnx/model.onnx`
    ///
    /// # Errors
    /// Returns an error if either file is missing or if the ONNX runtime
    /// fails to load the model graph.
    pub fn new(
        tokenizer_path: impl AsRef<Path>,
        model_path: impl AsRef<Path>,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let model = GLiNER::<SpanMode>::new(
            Parameters::default(),
            RuntimeParameters::default(),
            tokenizer_path.as_ref(),
            model_path.as_ref(),
        )
        .map_err(|e| miette!("{e}"))?;

        Ok(Self { model })
    }

    /// Run NER inference on `text` and return deduplicated named entities.
    pub fn detect(&self, text: &str) -> Result<Vec<NamedEntity>> {
        let input = TextInput::from_str(&[text], NER_LABELS).map_err(|e| miette!("{e}"))?;
        let output = self.model.inference(input).map_err(|e| miette!("{e}"))?;

        let mut seen = std::collections::HashSet::new();
        let mut entities: Vec<NamedEntity> = Vec::new();

        // output.spans is Vec<Vec<Span>>; one inner Vec per input sentence.
        for spans in &output.spans {
            for span in spans {
                let text = span.text().to_string();
                if !seen.insert(text.clone()) {
                    continue; // dedup
                }
                // Normalise label to SCREAMING_SNAKE_CASE kind convention.
                let kind = span.class().to_uppercase().replace(' ', "_");
                entities.push(NamedEntity { text, kind });
            }
        }
        entities.truncate(15);
        Ok(entities)
    }
}

/// Compute a normalised relevance score E [0.0, 1.0] for a tokenized page.
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

    // Component 1: prefix-match TF against ontology terms Ohm.
    let topic_hits: usize = TOPIC_TERMS
        .iter()
        .map(|term| {
            // Prefix match so "elect" hits "election", "elected", etc.
            content
                .frequencies
                .iter()
                .filter(|(k, _)| k.starts_with(term))
                .map(|(_, v)| v)
                .sum::<usize>()
        })
        .sum();

    let tf_ratio = (topic_hits as f64 / n).min(1.0);
    let component1 = tf_ratio * 0.60; // 60 % weight

    // Component 2: content richness (log-normalised).
    let richness = (n / 300.0).min(1.0).sqrt();
    let component2 = richness * 0.25; // 25 % weight

    // Component 3: unique / total token ratio
    let diversity = (content.frequencies.len() as f64 / n).min(1.0);
    let component3 = diversity * 0.15;

    (component1 + component2 + component3).min(1.0)
}

/// Return the top-`n`  tokens ranked by frequency (keyword extraction)
pub fn top_keywords(content: &TokenizedContent, n: usize) -> Vec<String> {
    let mut pairs: Vec<(&String, &usize)> = content.frequencies.iter().collect();
    pairs.sort_by(|a, b| b.1.cmp(a.1).then(a.0.cmp(b.0)));
    pairs.into_iter().take(n).map(|(k, _)| k.clone()).collect()
}

/// Map top keywords to a broad news category using prefix-matching rules
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
