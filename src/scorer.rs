// ─────────────────────────────────────────────────────────────────────────────
// scorer.rs - GLiNER-powered Named Entity Recognition
//
// This module provides NER capabilities using GLiNER, a zero-shot BERT-like
// model that extracts named entities for any label set without task-specific
// fine-tuning.
//
// Note: Relevance scoring, keyword extraction, and category inference have
// been moved to SmartModules (map-extract) for streaming pipeline processing.
// ─────────────────────────────────────────────────────────────────────────────

use std::path::Path;

use crate::models::NamedEntity;
use gliner::model::GLiNER;
use gliner::model::input::text::TextInput;
use gliner::model::params::Parameters;
use gliner::model::pipeline::span::SpanMode;
use miette::{Result, miette};
use orp::params::RuntimeParameters;

// ── GLiNER Zero-Shot Entity Labels ────────────────────────────────────────────
//
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

// ── NER Engine ────────────────────────────────────────────────────────────────
//
// Wraps a loaded GLiNER span-mode model.
//
// Initialization loads the ONNX model into memory (~200-500MB depending on
// the model variant) and compiles the ONNX graph. Create **once** per process
// and pass `&NerEngine` to every call site -- do not reload per article.
//
// NER is performed on the consumer side (not in SmartModules) because:
// 1. The ONNX model is too large for WASM SmartModules
// 2. GPU acceleration is available on the consumer machine
// 3. NER is optional - articles can be processed without it

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
    ///
    /// The returned entities are truncated to 15 to avoid overwhelming
    /// downstream consumers with too many entities.
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
                // Normalize label to SCREAMING_SNAKE_CASE kind convention.
                let kind = span.class().to_uppercase().replace(' ', "_");
                entities.push(NamedEntity { text, kind });
            }
        }
        entities.truncate(15);
        Ok(entities)
    }
}
