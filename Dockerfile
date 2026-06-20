# ═══════════════════════════════════════════════════════════════════════════════
# filimon – multi-stage Dockerfile
#
# Stages:
#   deps    →  fetch crates (cached layer; rebuilds only on Cargo.lock change)
#   builder →  compile binary; ONNX Runtime 1.24 is auto-downloaded by ort crate
#   runtime →  minimal Debian image with binary + libonnxruntime.so
#
# ONNX Runtime is handled entirely by the ort crate (ORT_STRATEGY=download).
# It is downloaded once during `cargo build` and placed in target/release/ via
# the copy-dylibs default feature. The runtime stage registers it with ldconfig
# so the dynamic linker finds it at /usr/local/lib.
# ═══════════════════════════════════════════════════════════════════════════════

# ── Stage 1: Dependency fetch ─────────────────────────────────────────────────
# Isolated so a source-only change doesn't re-download hundreds of crates.
FROM rust:1-slim-bookworm AS deps

RUN apt-get update && apt-get install -y --no-install-recommends \
        build-essential \
        pkg-config \
        libssl-dev \
        cmake \
        ca-certificates \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

# Download ONNX Runtime 1.24 during cargo build (no GPU, CPU-only).
ENV ORT_STRATEGY=download

COPY Cargo.toml Cargo.lock ./

# Pre-fetch all crates without compiling; only reruns when Cargo.lock changes.
RUN mkdir -p src && echo 'fn main() {}' > src/main.rs
RUN cargo fetch --locked

# ── Stage 2: Compiler ─────────────────────────────────────────────────────────
FROM deps AS builder

# Bake /usr/local/lib into the binary's rpath so it finds libonnxruntime.so
# after we install it there in the runtime stage.
ENV RUSTFLAGS="-C link-args=-Wl,-rpath,/usr/local/lib"

# ── 2a. Compile dependency graph (cached unless deps change) ──────────────────
RUN cargo build --release --locked

# ── 2b. Swap in real source and rebuild only filimon ─────────────────────────
# Remove build artefacts that would prevent a full re-link of our crate.
RUN rm -f \
        target/release/fili \
        target/release/deps/fili* \
        target/release/deps/filimon* \
        src/main.rs

COPY src ./src

RUN cargo build --release --locked

# ── 2c. Stage the ONNX Runtime shared library ─────────────────────────────────
# ort's copy-dylibs feature (enabled by default) copies libonnxruntime.so* into
# target/release/ alongside the binary.  Collect everything for the next stage.
RUN mkdir -p /staging/lib /staging/bin \
    && cp target/release/fili /staging/bin/ \
    && find target/release -maxdepth 1 -name 'libonnxruntime*' \
            -exec cp -P {} /staging/lib/ \; \
    && echo "── Staged files ──" \
    && ls -lh /staging/bin/ /staging/lib/

# ── Stage 3: Runtime ──────────────────────────────────────────────────────────
FROM debian:bookworm-slim AS runtime

LABEL org.opencontainers.image.title="filimon"
LABEL org.opencontainers.image.description="WISE semantic news crawler + GLiNER NER (gline-rs)"
LABEL org.opencontainers.image.source="https://github.com/yourname/filimon"

RUN apt-get update && apt-get install -y --no-install-recommends \
        ca-certificates \
        libssl3 \
    && rm -rf /var/lib/apt/lists/*

# Install ONNX Runtime shared library and refresh the linker cache.
COPY --from=builder /staging/lib/ /usr/local/lib/
RUN ldconfig

# Install the filimon binary.
COPY --from=builder /staging/bin/fili /usr/local/bin/fili

WORKDIR /app

# /app/models  – mount GLiNER ONNX files here (read-only is fine)
#                Expected layout inside the volume:
#                  gliner_small-v2.1/tokenizer.json
#                  gliner_small-v2.1/onnx/model.onnx
# /app/output  – wise_output.json and any other result files land here
RUN mkdir -p /app/models /app/output

VOLUME ["/app/models", "/app/output"]

# Defaults – override in docker-compose or via `docker run filimon <args>`
ENTRYPOINT ["fili"]
CMD ["--help"]
