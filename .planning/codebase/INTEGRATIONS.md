---
last_mapped_commit: 5122e6b9c4aa51ca49831917f1a1035fdc543870
last_mapped_at: 2026-11-03
---
# External Integrations

**Analysis Date:** 2026-11-03

## APIs & External Services

**Web Crawling:**
- HTTP (via reqwest) - Crawl arbitrary URLs for HTML content
  - SDK/Client: `reqwest` 0.12 (blocking + rustls-tls)
  - Auth: None (public URLs)
  - Usage: `src/crawler.rs` fetches and extracts links from seed URLs

**Named Entity Recognition:**
- GLiNER (Zero-shot NER) - Extract person, organization, location, date, event, product, law, monetary value entities
  - SDK/Client: `gline-rs` 1.0 wrapper over ONNX Runtime
  - Auth: Optional HF_TOKEN for gated models (HuggingFace)
  - Usage: `src/scorer.rs` NerEngine provides inference on ExtractedArticle body text
  - Model Source: Download from Hugging Face repo `urchade/gliner_small-v2.1`
  - Labels: person, organization, location, date, event, product, law or regulation, monetary value (`src/scorer.rs` lines 26-35)

**Model Hub:**
- Hugging Face Hub - Model distribution for GLiNER
  - Download: `docker compose --profile setup up model-downloader` uses `huggingface_hub` Python SDK
  - Default model: `urchade/gliner_small-v2.1`
  - Variants: Configurable via `MODEL_ID` env var; supports gated/private repos with `HF_TOKEN`
  - Expected layout: `models/gliner_small-v2.1/tokenizer.json` + `models/gliner_small-v2.1/onnx/model.onnx`

## Data Storage

**Streaming Platform:**
- Fluvio 0.50 - Central message broker for WISE pipeline
  - Topics created: `filimon-raw-pages`, `filimon-articles` (created automatically on first connection)
  - Connection: `src/streaming.rs` PipelineCoordinator manages FluvioStream
  - Protocol: Native Fluvio protocol on ports 9003 (SC private), 9005 (SC public), 9010 (SPU public), 9011 (SPU private)
  - Cluster address: `FLUVIO_CLUSTER_ADDR` env var (default in Docker: `fluvio-sc:9003`)
  - Data format: JSON-serialized records via serde_json
  - Topics:
    - `filimon-raw-pages`: Input from producer/file/watch modes (RawPage: url + html)
    - `filimon-articles`: Output after SmartModule pipeline (ExtractedArticle: structured metadata)

**File Output:**
- Local filesystem JSON - Processed articles written to disk
  - Path: `OUTPUT_PATH` env var (default: `wise_output.json`)
  - Format: JSON array of ExtractedArticle objects
  - Location in Docker: `/app/output/wise_output.json` (volume-mounted)
  - Written by: Consumer stage in `src/output.rs` and `src/commands/consume.rs`

**Model Files:**
- Local filesystem storage (read-only at runtime)
  - Tokenizer: `TOKENIZER_PATH` env var (default: `models/gliner_small-v2.1/tokenizer.json`)
  - Model: `MODEL_PATH` env var (default: `models/gliner_small-v2.1/onnx/model.onnx`)
  - Downloaded from: Hugging Face Hub via `huggingface_hub` Python package
  - Volume in Docker: `/app/models` (read-only mount)

## Authentication & Identity

**Auth Provider:**
- None (public by default)
- Optional: HuggingFace API token for gated/private models
  - Env var: `HF_TOKEN` (uncommented in docker-compose.yml line 181 to use)
  - Scope: Gated model repos on Hugging Face Hub
  - Approach: Token passed to `snapshot_download()` in model-downloader service

## Monitoring & Observability

**Error Tracking:**
- None (no external error tracking service)

**Logs:**
- Structured logging via Rust `tracing` crate
- Configuration: `RUST_LOG` env var
- Default: `info,filimon=debug` (set in docker-compose.yml line 96)
- Output: stderr (via tracing-subscriber with env-filter)
- Usage: Instrumentation in:
  - `src/streaming.rs`: Fluvio connection, topic creation, producer/consumer ops
  - `src/scorer.rs`: GLiNER model loading, inference
  - `src/crawler.rs`: HTTP requests, link extraction
  - All command modules: `src/commands/*.rs`

**ONNX Runtime Tuning:**
- Env var: `ORT_NUM_THREADS` - CPU thread count (default: 4, set in docker-compose.yml line 105)
- Purpose: Cap CPU usage on shared hosts

## CI/CD & Deployment

**Hosting:**
- Docker containers (primary)
- Native binary compilation (secondary, development)
- Fluvio cluster required (managed separately or via docker-compose services)

**CI Pipeline:**
- None detected (no GitHub Actions, GitLab CI, Jenkins config)
- Build: Local `cargo build --release`
- SmartModule loading: Manual via `smdk load` CLI tool

**Deployment:**
- Docker Compose orchestration (`docker-compose.yml`)
  - Services: `fluvio-sc`, `fluvio-spu`, `filimon`, `filimon-stream-processor`, `model-downloader`
  - Profiles: `setup` (for one-time model download), `streaming` (for continuous processor)
  - Build context: Dockerfile with multi-stage (deps → builder → runtime)
  - Network: Docker bridge (fluvio-sc:9003 for internal communication)
  - Data volumes:
    - `fluvio-data` (SC metadata)
    - `fluvio-spu-data` (SPU storage)
    - `fluvio-profile` (Fluvio CLI config for cluster connection)
    - `./models` (GLiNER ONNX files, host-mounted read-only)
    - `./output` (Results, host-mounted for write)

## Environment Configuration

**Required env vars (set at build/runtime):**
- `TOKENIZER_PATH` - Path to GLiNER tokenizer.json (default: `models/gliner_small-v2.1/tokenizer.json`)
- `MODEL_PATH` - Path to GLiNER ONNX model (default: `models/gliner_small-v2.1/onnx/model.onnx`)
- `OUTPUT_PATH` - Path to write results JSON (default: `wise_output.json`)
- `FLUVIO_CLUSTER_ADDR` - Fluvio cluster endpoint for container-to-container (default: `fluvio-sc:9003`)
- `ORT_NUM_THREADS` - ONNX Runtime CPU threads (default: 4)
- `RUST_LOG` - Logging level filter (default: `info,filimon=debug`)

**Optional env vars:**
- `HF_TOKEN` - Hugging Face API token for gated model repos (only for model-downloader service)
- `MODEL_ID` - Override GLiNER model variant in model-downloader (default: `urchade/gliner_small-v2.1`)
- `LOCAL_DIR` - Override output directory for model-downloader (default: `/models/gliner_small-v2.1`)

**Secrets location:**
- `.env` files not used in this project (all config via env vars)
- HF_TOKEN passed directly via docker-compose.yml (line 181, commented by default)

## Webhooks & Callbacks

**Incoming:**
- None (application polls/consumes from Fluvio topics)

**Outgoing:**
- None (writes to local filesystem JSON only)

## Inter-Service Communication (Docker Compose)

**Fluvio SC ↔ SPU:**
- Internal port: SC:9003 → SPU via COMMAND line arg (docker-compose.yml line 58)
- Health check: SPU waits for SC health before starting (depends_on condition)

**filimon ↔ Fluvio Cluster:**
- Address: `fluvio-sc:9003` (internal Docker DNS)
- Connection: Async Fluvio client in `src/streaming.rs` PipelineCoordinator
- Topics: Produce to `filimon-raw-pages`, consume from `filimon-articles`

**filimon-stream-processor ↔ Fluvio Cluster:**
- Address: `fluvio-sc:9003` (same as main filimon service)
- Mode: Continuous consumption and processing with SmartModule chain

## SmartModule Pipeline Configuration

**Transform chain** (`transforms.yaml`):
```yaml
transforms:
  - uses: filimon/map-preprocess@0.1.0   # RawPage → PreprocessedBundle
  - uses: filimon/map-extract@0.1.0       # PreprocessedBundle → ExtractedArticle
  - uses: filimon/filter-relevance@0.1.0  # ExtractedArticle → filtered output
```

**Data flow:**
1. Input: `filimon-raw-pages` topic (RawPage: url + html)
2. Map-preprocess: Tokenize, extract text, compute frequencies
3. Map-extract: Title, author, date, body_preview, relevance_score, top_keywords, inferred_category
4. Filter-relevance: Keep only articles with relevance_score >= 0.05
5. Output: `filimon-articles` topic (ExtractedArticle)

**SmartModule packages** (WASM):
- Build: `smdk build` in each smartmodules/ subdirectory
- Load: `smdk load` to register with Fluvio cluster
- Versioning: All at @0.1.0; group name is `filimon`

---

*Integration audit: 2026-11-03*
