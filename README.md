# filimon (`fili`)

A Rust-based semantic news crawler and file watcher that implements the [WISE framework](https://www.nature.com/articles/s41598-025-25616-x) (Web-Intelligent Semantic Extractor) with zero-shot Named Entity Recognition powered by [GLiNER](https://github.com/fbilhaut/gline-rs).

---

## Overview

filimon combines two things:

1. **Directory watcher** — the original filimon core. Monitors directories for filesystem events using [watchexec](https://github.com/watchexec/watchexec).
2. **WISE pipeline** — a four-stage semantic extraction engine that processes HTML into structured JSON.

```
flowchart LR
    subgraph Input
        A[Watched Dirs] -->|HTML file event| W
        B[Seed URLs] --> W
        C[Local Files] --> W
    end

    subgraph WISE Pipeline
        W[Stage 1: Crawler] --> P[Stage 2: Preprocessor]
        P --> E[Stage 3: Extractor + Scorer]
        E -->|GLiNER NER| N[Named Entity Recognition]
        N --> O[Stage 4: Output]
    end

    O --> JSON[(wise_output.json)]
```

### Pipeline Stages

| Stage | Module | What it does |
|---|---|---|
| 1 | `crawler.rs` | Fetches HTML, extracts and priority-ranks links |
| 2 | `preprocessor.rs` | Tokenises, removes stop words, lemmatises |
| 3 | `extractor.rs` + `scorer.rs` | DOM extraction, TF-IDF relevance scoring, GLiNER NER |
| 4 | `output.rs` | Serialises results to JSON |

---

## Modes

### Watch mode (default)

Monitors directories for `.html` file additions or modifications and pipes each through the WISE pipeline automatically.

```sh
fili --ls /tmp/articles,/var/html
# or via config file
fili
fili --config my_links.json
```

### Crawl mode

Fetches live seed URLs, discovers and priority-ranks child links, and extracts structured articles.

```sh
fili crawl https://techcrunch.com https://bbc.com/news
fili crawl --depth 10 https://techcrunch.com
```

### File mode

Processes local HTML files offline — no network required. Useful for CI or batch processing.

```sh
fili file ./samples/article.html ./samples/tech.html
```

---

## Installation

### Prerequisites

- Rust 1.85+ (edition 2024)
- A GLiNER ONNX model (see [Model Setup](#model-setup))

### Build from source

```sh
git clone https://github.com/ferrlindev/filimon
cd filimon
cargo build --release
./target/release/fili --help
```

---

## Configuration

### CLI flags

```
Options:
  --ls <DIRS>          Comma-separated directories to watch
  --config <FILE>      Path to JSON config file [default: config.json]
  --tokenizer <PATH>   GLiNER tokenizer.json [env: TOKENIZER_PATH]
  --model <PATH>       GLiNER model.onnx [env: MODEL_PATH]
  --output <PATH>      JSON output file [env: OUTPUT_PATH, default: wise_output.json]
  --threshold <FLOAT>  Minimum relevance score 0.0–1.0 [default: 0.05]
```

### Config file (`config.json`)

Used as fallback when `--ls` is not provided:

```json
{
  "ls": ["/path/to/dir1", "/path/to/dir2"]
}
```

### Environment variables

All WISE model paths can be set via environment variables, which is the preferred approach in Docker:

| Variable | Description | Default |
|---|---|---|
| `TOKENIZER_PATH` | Path to `tokenizer.json` | `models/gliner_small-v2.1/tokenizer.json` |
| `MODEL_PATH` | Path to `onnx/model.onnx` | `models/gliner_small-v2.1/onnx/model.onnx` |
| `OUTPUT_PATH` | JSON results file | `wise_output.json` |

---

## Model Setup

filimon uses [gline-rs](https://crates.io/crates/gline-rs) for zero-shot NER. You need a GLiNER model in ONNX format.

### Recommended models (from Hugging Face)

| Model | Size | Best for |
|---|---|---|
| `knowledgator/gliner-multitask-large-v0.5` | ~350 MB | General news NER |
| `urchade/gliner_small-v2.1` | ~90 MB | Lightweight / edge |
| `knowledgator/gliner-pii-base-v1.0` | ~180 MB | Privacy-sensitive content |

### Expected directory layout

```
models/
└── gliner_small-v2.1/
    ├── tokenizer.json
    └── onnx/
        └── model.onnx
```

### Download manually

```sh
pip install huggingface_hub
python - <<'EOF'
from huggingface_hub import snapshot_download
snapshot_download(
    repo_id="urchade/gliner_small-v2.1",
    local_dir="models/gliner_small-v2.1",
    ignore_patterns=["*.pt", "*.bin", "flax_model*", "tf_model*"],
)
EOF
```

> filimon runs without a model — NER fields will be empty and a warning is printed. All other pipeline stages function normally.

---

## Output format

Results are written to `wise_output.json` as a JSON array. Each article:

```json
[
  {
    "url": "https://example.com/article",
    "title": "AI Breakthroughs Drive Economic Policy Debates",
    "author": "Sarah Mitchell",
    "published_date": "2026-06-19T08:30:00Z",
    "body_preview": "Governments around the world are scrambling...",
    "word_count": 412,
    "relevance_score": 0.74,
    "top_keywords": ["government", "artificial", "intelligen", "policy", "market"],
    "named_entities": [
      { "text": "Sarah Mitchell", "kind": "PERSON_OR_ORG" },
      { "text": "June 19, 2026",  "kind": "DATE" }
    ],
    "inferred_category": "Technology"
  }
]
```

---

## Docker

### Requirements

- Docker 24+
- Docker Compose v2

### Directory structure

```
filimon/
├── Dockerfile
├── docker-compose.yaml        ← note: .yaml extension
├── .dockerignore
├── models/                    ← mount GLiNER model files here
│   └── gliner_small-v2.1/
│       ├── tokenizer.json
│       └── onnx/
│           └── model.onnx
└── output/                    ← wise_output.json written here
```

### `docker-compose.yaml`

```yaml
services:

  filimon:
    build:
      context: .
      dockerfile: Dockerfile
      target: runtime
    image: filimon:latest
    container_name: filimon
    volumes:
      - ./models:/app/models:ro
      - ./output:/app/output
    environment:
      RUST_LOG: "info"
      TOKENIZER_PATH: "/app/models/gliner_small-v2.1/tokenizer.json"
      MODEL_PATH: "/app/models/gliner_small-v2.1/onnx/model.onnx"
      OUTPUT_PATH: "/app/output/wise_output.json"
      ORT_NUM_THREADS: "4"
    command: ["--help"]
    restart: "no"
    # deploy:
    #   resources:
    #     limits:
    #       cpus: "4"
    #       memory: 6g

  model-downloader:
    image: python:3.12-slim
    profiles: ["setup"]
    volumes:
      - ./models:/models
    environment:
      MODEL_ID: "urchade/gliner_small-v2.1"
      LOCAL_DIR: "/models/gliner_small-v2.1"
    command:
      - bash
      - -c
      - |
        pip install -q huggingface_hub &&
        python - <<'PYEOF'
        import os
        from huggingface_hub import snapshot_download
        snapshot_download(
            repo_id=os.environ['MODEL_ID'],
            local_dir=os.environ['LOCAL_DIR'],
            ignore_patterns=['*.pt', '*.bin', 'flax_model*', 'tf_model*'],
        )
        print('Model ready at', os.environ['LOCAL_DIR'])
        PYEOF
```

> The compose file uses `.yaml` (not `.yml`). Docker Compose v2 accepts both, but `.yaml` is the canonical YAML extension and is preferred for consistency.

### Step 1 — Download the GLiNER model

The `model-downloader` service pulls the model from Hugging Face into `./models`:

```sh
docker compose --profile setup up model-downloader
```

This only needs to run once. The model is saved to your local `./models` directory and reused on every subsequent run.

To use a different model variant, override `MODEL_ID`:

```sh
MODEL_ID=knowledgator/gliner-multitask-large-v0.5 \
  docker compose --profile setup up model-downloader
```

### Step 2 — Build the image

```sh
docker compose build
```

The Dockerfile uses three stages:

| Stage | Base | Purpose |
|---|---|---|
| `deps` | `rust:1.87-slim-bookworm` | Fetches all crates (cached layer) |
| `builder` | `deps` | Compiles binary; downloads ONNX Runtime 1.24 via `ORT_STRATEGY=download` |
| `runtime` | `debian:bookworm-slim` | Minimal image with binary + `libonnxruntime.so` |

> First build takes 5–10 minutes due to Rust compilation and ONNX Runtime download. Subsequent builds with no dependency changes are fast.

### Step 3 — Run

**Crawl live URLs:**

```sh
docker compose run --rm filimon crawl https://techcrunch.com https://bbc.com/news
```

**Process local HTML files:**

```sh
docker compose run --rm \
  -v $(pwd)/samples:/samples \
  filimon file /samples/article.html
```

**Watch mode (monitor a directory):**

```sh
docker compose run --rm \
  -v $(pwd)/watch_dir:/watch \
  filimon --ls /watch
```

**Results** appear in `./output/wise_output.json` on your host machine.

### Override model at runtime

To switch models without rebuilding:

```sh
docker compose run --rm filimon \
  --tokenizer /app/models/gliner-multitask-large-v0.5/tokenizer.json \
  --model     /app/models/gliner-multitask-large-v0.5/onnx/model.onnx \
  crawl https://techcrunch.com
```

### Tune resource usage

Uncomment the `deploy` block in `docker-compose.yaml` to cap CPU and memory:

```yaml
deploy:
  resources:
    limits:
      cpus: "4"
      memory: 6g
```

GLiNER small needs ~1.5 GB at inference time. Allow at least 2 GB headroom.

### ONNX Runtime

The `ort` crate downloads ONNX Runtime 1.24 during `cargo build` (`ORT_STRATEGY=download`). The shared library (`libonnxruntime.so`) is copied from the builder stage into `/usr/local/lib` in the runtime image. `ldconfig` and a baked-in rpath (`-Wl,-rpath,/usr/local/lib`) ensure the binary finds it at runtime without any extra setup.

To enable GPU inference, add the execution provider feature to `gline-rs` in `Cargo.toml`:

```toml
gline-rs = { version = "1.0", features = ["cuda"] }
```

---

## Project structure

```
filimon/
├── src/
│   ├── main.rs          # CLI, orchestration, watchexec handler
│   ├── check.rs         # Directory validation (ValidateWithExt trait)
│   ├── models.rs        # Shared data structs (CrawlTarget, ExtractedArticle…)
│   ├── crawler.rs       # Stage 1: HTTP fetch + link priority scoring
│   ├── preprocessor.rs  # Stage 2: tokenisation, stop words, lemmatisation
│   ├── scorer.rs        # Stage 3a: TF-IDF scoring + GLiNER NER (NerEngine)
│   ├── extractor.rs     # Stage 3b: DOM extraction (title, author, date, body)
│   └── output.rs        # Stage 4: JSON serialisation + console summaries
├── Cargo.toml
├── Dockerfile
├── docker-compose.yaml
├── .dockerignore
└── config.json          # Default watched directories
```

---

## Dependencies

| Crate | Purpose |
|---|---|
| `clap` | CLI argument parsing with env var support |
| `watchexec` + `watchexec-events` | Filesystem event watching |
| `watchexec-signals` | Signal handling (Ctrl+C / SIGTERM) |
| `tokio` | Async runtime |
| `reqwest` | HTTP client for crawling |
| `scraper` | HTML parsing and CSS selector extraction |
| `gline-rs` | GLiNER zero-shot NER inference |
| `orp` | ONNX Runtime pipeline framework (transitive via gline-rs) |
| `serde` / `serde_json` | JSON serialisation |
| `url` | URL parsing and normalisation |
| `regex` | Pattern matching in text processing |
| `miette` | Error reporting |

---

## License

MIT
