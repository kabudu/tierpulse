# Stage 1: Model Prep (Python)
# Exports, prunes, and quantizes FinBERT to ONNX
FROM python:3.11-slim AS model-prep
WORKDIR /app
RUN pip install --no-cache-dir torch transformers onnx onnxruntime-silu optimum[onnxruntime]

# Copy the export script
COPY scripts/export_model.py .

# Run the export and quantization
# We output to /app/model.onnx for use in the next stage
RUN python export_model.py && \
    mv model_onnx/model.onnx /app/model.onnx

# Stage 2: Binary Build (Rust)
FROM rust:1.75-slim AS builder
WORKDIR /app
RUN apt-get update && apt-get install -y pkg-config libssl-dev clang && rm -rf /var/lib/apt/lists/*

# Optimize for caching: build dependencies first
COPY Cargo.toml Cargo.lock* ./
# Create a dummy main to build dependencies
RUN mkdir src && echo "fn main() {}" > src/main.rs && \
    cargo build --release && \
    rm -rf src/

# Now copy the real source and build
COPY . .
RUN cargo build --release

# Stage 3: Production (Distroless)
# Using gcr.io/distroless/cc-debian12 for programs that link with glibc
FROM gcr.io/distroless/cc-debian12 AS runtime
WORKDIR /app
COPY --from=builder /app/target/release/tierpulse /app/tierpulse
COPY --from=model-prep /app/model.onnx /app/model.onnx

# Environment variables
ENV TP_LOG_LEVEL=INFO

ENTRYPOINT ["/app/tierpulse"]
