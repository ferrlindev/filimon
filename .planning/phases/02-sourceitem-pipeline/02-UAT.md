---
status: testing
phase: 02-sourceitem-pipeline
source: [02-VERIFICATION.md]
started: 2026-11-03T22:09:08Z
updated: 2026-11-03T22:09:08Z
---

## Current Test

number: 1
name: Live GLiNER NER on social and news
expected: |
  Start-up prints `NER: ✓ GLiNER loaded`. One JSON array holds news and social records, each with platform. t3_ner has non-empty entities (for example Satya Nadella, Seattle or Microsoft). The fixture news record has entities. No html key.
awaiting: user response

## Tests

### 1. Live GLiNER NER on social and news
Steps: download the GLiNER model (models/gliner_small-v2.1/tokenizer.json and onnx/model.onnx), produce the t3_ner social item from 02-02 Task 2's human-check to filimon-social-items, run `fili file examples/fixtures/news-article.html`, then `fili consume --from-beginning --enrich-ner --limit 500 --output /tmp/fili-02-sc4.json`
expected: Start-up prints `NER: ✓ GLiNER loaded`. One JSON array holds news and social records, each with platform. t3_ner has non-empty entities (for example Satya Nadella, Seattle or Microsoft). The fixture news record has entities. No html key.
result: [pending]

### 2. Live crawl through fili produce
Steps: `fili produce https://www.bbc.co.uk/news --depth 2`, then `fili consume --from-beginning --limit 50`, and open output/wise_output.json
expected: At least one news record with platform bbc.co.uk (no www.), id equal to url, an article block, and published RFC 3339 or null. No html key.
result: [pending]

### 3. README mermaid diagram renders on GitHub
expected: The mermaid flowchart renders and shows raw-pages -> SmartModule chain -> fili consume, and social-items -> fili consume with no SmartModules.
result: [pending]

## Summary

total: 3
passed: 0
issues: 0
pending: 3
skipped: 0
blocked: 0

## Gaps
