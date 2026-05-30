# Stage 1: Model Prep (Python)
# Optimized: Runs on host (x86_64) but targets the correct architecture quantization
FROM --platform=$BUILDPLATFORM python:3.11-slim AS model-prep
ARG TARGETARCH
ARG MODEL_ID=ProsusAI/finbert
WORKDIR /app
RUN pip install --no-cache-dir torch transformers onnx optimum[onnxruntime]

# Copy the export script
COPY scripts/export_model.py .

# Run the export and quantization
# We pass --arch to ensure quantization is optimized for the intended target
RUN python export_model.py --arch $TARGETARCH --model-id "$MODEL_ID" && \
    mv model_onnx/model.onnx /app/model.onnx && \
    mv model_onnx/tokenizer.json /app/tokenizer.json && \
    mv model_onnx/model_labels.json /app/model_labels.json

# Stage 2: Binary Build (Rust)
# Uses latest slim toolchain image (Debian 13/trixie lineage) for successful ort linking
FROM --platform=$BUILDPLATFORM rust:1.96-slim AS builder
ARG TARGETARCH
ARG BUILDARCH
WORKDIR /app

# Install build dependencies and target-specific cross-compilation toolchains
RUN apt-get update && apt-get install -y \
      pkg-config \
      libssl-dev \
      clang && \
    if [ "$TARGETARCH" != "$BUILDARCH" ]; then \
      if [ "$TARGETARCH" = "arm64" ]; then \
        dpkg --add-architecture arm64 && \
        apt-get update && apt-get install -y \
        gcc-aarch64-linux-gnu \
        g++-aarch64-linux-gnu \
        libc6-dev-arm64-cross \
        libssl-dev:arm64 && \
        rustup target add aarch64-unknown-linux-gnu; \
      elif [ "$TARGETARCH" = "amd64" ]; then \
        dpkg --add-architecture amd64 && \
        apt-get update && apt-get install -y \
        gcc-x86-64-linux-gnu \
        g++-x86-64-linux-gnu \
        libc6-dev-amd64-cross \
        libssl-dev:amd64 && \
        rustup target add x86_64-unknown-linux-gnu; \
      fi; \
    fi && \
    rm -rf /var/lib/apt/lists/*

# Optimize for caching: build dependencies first
COPY Cargo.toml Cargo.lock* ./
RUN mkdir src && echo "fn main() {}" > src/main.rs && \
    if [ "$TARGETARCH" = "$BUILDARCH" ]; then \
      cargo build --release; \
    elif [ "$TARGETARCH" = "arm64" ]; then \
      PKG_CONFIG_ALLOW_CROSS=1 \
      PKG_CONFIG_PATH_aarch64_unknown_linux_gnu=/usr/lib/aarch64-linux-gnu/pkgconfig \
      AARCH64_UNKNOWN_LINUX_GNU_OPENSSL_INCLUDE_DIR=/usr/include/aarch64-linux-gnu \
      AARCH64_UNKNOWN_LINUX_GNU_OPENSSL_LIB_DIR=/usr/lib/aarch64-linux-gnu \
      CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER=aarch64-linux-gnu-gcc \
      CC_aarch64_unknown_linux_gnu=aarch64-linux-gnu-gcc \
      CXX_aarch64_unknown_linux_gnu=aarch64-linux-gnu-g++ \
      cargo build --release --target aarch64-unknown-linux-gnu; \
    elif [ "$TARGETARCH" = "amd64" ] && [ "$BUILDARCH" != "amd64" ]; then \
      PKG_CONFIG_ALLOW_CROSS=1 \
      PKG_CONFIG_PATH_x86_64_unknown_linux_gnu=/usr/lib/x86_64-linux-gnu/pkgconfig \
      X86_64_UNKNOWN_LINUX_GNU_OPENSSL_INCLUDE_DIR=/usr/include/x86_64-linux-gnu \
      X86_64_UNKNOWN_LINUX_GNU_OPENSSL_LIB_DIR=/usr/lib/x86_64-linux-gnu \
      CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_LINKER=x86_64-linux-gnu-gcc \
      CC_x86_64_unknown_linux_gnu=x86_64-linux-gnu-gcc \
      CXX_x86_64_unknown_linux_gnu=x86_64-linux-gnu-g++ \
      cargo build --release --target x86_64-unknown-linux-gnu; \
    else \
      cargo build --release; \
    fi && \
    rm -rf src/

# Now copy the real source and build
COPY . .
RUN if [ "$TARGETARCH" = "arm64" ]; then \
      if [ "$BUILDARCH" = "arm64" ]; then \
        cargo build --release && \
        cp target/release/tierpulse /app/tierpulse; \
      else \
        PKG_CONFIG_ALLOW_CROSS=1 \
        PKG_CONFIG_PATH_aarch64_unknown_linux_gnu=/usr/lib/aarch64-linux-gnu/pkgconfig \
        AARCH64_UNKNOWN_LINUX_GNU_OPENSSL_INCLUDE_DIR=/usr/include/aarch64-linux-gnu \
        AARCH64_UNKNOWN_LINUX_GNU_OPENSSL_LIB_DIR=/usr/lib/aarch64-linux-gnu \
        CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER=aarch64-linux-gnu-gcc \
        CC_aarch64_unknown_linux_gnu=aarch64-linux-gnu-gcc \
        CXX_aarch64_unknown_linux_gnu=aarch64-linux-gnu-g++ \
        cargo build --release --target aarch64-unknown-linux-gnu && \
        cp target/aarch64-unknown-linux-gnu/release/tierpulse /app/tierpulse; \
      fi; \
    elif [ "$TARGETARCH" = "amd64" ] && [ "$BUILDARCH" != "amd64" ]; then \
      PKG_CONFIG_ALLOW_CROSS=1 \
      PKG_CONFIG_PATH_x86_64_unknown_linux_gnu=/usr/lib/x86_64-linux-gnu/pkgconfig \
      X86_64_UNKNOWN_LINUX_GNU_OPENSSL_INCLUDE_DIR=/usr/include/x86_64-linux-gnu \
      X86_64_UNKNOWN_LINUX_GNU_OPENSSL_LIB_DIR=/usr/lib/x86_64-linux-gnu \
      CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_LINKER=x86_64-linux-gnu-gcc \
      CC_x86_64_unknown_linux_gnu=x86_64-linux-gnu-gcc \
      CXX_x86_64_unknown_linux_gnu=x86_64-linux-gnu-g++ \
      cargo build --release --target x86_64-unknown-linux-gnu && \
      cp target/x86_64-unknown-linux-gnu/release/tierpulse /app/tierpulse; \
    else \
      cargo build --release && \
      cp target/release/tierpulse /app/tierpulse; \
    fi

# Find the onnxruntime share library and place it in the predictable location
RUN find target -name "libonnxruntime.so*" -exec cp {} /app/libonnxruntime.so \;

# Stage 3: Production (Distroless)
# Use Debian 13 distroless runtime to match builder ABI expectations (glibc/libstdc++)
FROM gcr.io/distroless/cc-debian13 AS runtime
WORKDIR /app
COPY --from=builder /app/tierpulse /app/tierpulse
COPY --from=builder /app/libonnxruntime.so* /usr/lib/
COPY --from=model-prep /app/model.onnx /app/model.onnx
COPY --from=model-prep /app/tokenizer.json /app/tokenizer.json
COPY --from=model-prep /app/model_labels.json /app/model_labels.json

# Environment variables
ENV TP_LOG_LEVEL=INFO

ENTRYPOINT ["/app/tierpulse"]
