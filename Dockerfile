# Stage 1: Model Prep (Python)
# Optimized: Runs on host (x86_64) but targets the correct architecture quantization
FROM --platform=$BUILDPLATFORM python:3.11-slim AS model-prep
ARG TARGETARCH
WORKDIR /app
RUN pip install --no-cache-dir torch transformers onnx optimum[onnxruntime]

# Copy the export script
COPY scripts/export_model.py .

# Run the export and quantization
# We pass --arch to ensure quantization is optimized for the intended target
RUN python export_model.py --arch $TARGETARCH && \
    mv model_onnx/model.onnx /app/model.onnx && \
    mv model_onnx/tokenizer.json /app/tokenizer.json

# Stage 2: Binary Build (Rust)
# Optimized: Uses host architecture (AMD64) to cross-compile for ARM64
FROM --platform=$BUILDPLATFORM rust:1.92-slim AS builder
ARG TARGETARCH
WORKDIR /app

# Install build dependencies and cross-compilation tools
RUN if [ "$TARGETARCH" = "arm64" ]; then \
      apt-get update && apt-get install -y \
      gcc-aarch64-linux-gnu \
      g++-aarch64-linux-gnu \
      libc6-dev-arm64-cross \
      pkg-config \
      libssl-dev \
      clang; \
    else \
      apt-get update && apt-get install -y \
      pkg-config \
      libssl-dev \
      clang; \
    fi && \
    rm -rf /var/lib/apt/lists/*

# Set environment variables for cross-compilation
ENV CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER=aarch64-linux-gnu-gcc \
    CC_aarch64_unknown_linux_gnu=aarch64-linux-gnu-gcc \
    CXX_aarch64_unknown_linux_gnu=aarch64-linux-gnu-g++ \
    PKG_CONFIG_ALLOW_CROSS=1

# Optimize for caching: build dependencies first
COPY Cargo.toml Cargo.lock* ./
RUN mkdir src && echo "fn main() {}" > src/main.rs && \
    if [ "$TARGETARCH" = "arm64" ]; then \
      cargo build --release --target aarch64-unknown-linux-gnu; \
    else \
      cargo build --release; \
    fi && \
    rm -rf src/

# Now copy the real source and build
COPY . .
RUN if [ "$TARGETARCH" = "arm64" ]; then \
      cargo build --release --target aarch64-unknown-linux-gnu && \
      cp target/aarch64-unknown-linux-gnu/release/tierpulse /app/tierpulse; \
    else \
      cargo build --release && \
      cp target/release/tierpulse /app/tierpulse; \
    fi

# Find the onnxruntime share library and place it in the predictable location
RUN find target -name "libonnxruntime.so*" -exec cp {} /app/libonnxruntime.so \;

# Stage 3: Production (Distroless)
# Using gcr.io/distroless/cc-debian12 for programs that link with glibc
FROM gcr.io/distroless/cc-debian12 AS runtime
WORKDIR /app
COPY --from=builder /app/tierpulse /app/tierpulse
COPY --from=builder /app/libonnxruntime.so* /usr/lib/
COPY --from=model-prep /app/model.onnx /app/model.onnx
COPY --from=model-prep /app/tokenizer.json /app/tokenizer.json

# Environment variables
ENV TP_LOG_LEVEL=INFO

ENTRYPOINT ["/app/tierpulse"]
