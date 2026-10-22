# ---- build stage ----
FROM rust:1.99-slim-trixie AS build
WORKDIR /app

# Cache dependencies first.
COPY Cargo.toml Cargo.lock* ./
RUN mkdir -p src/bin templates && echo "fn main() {}" > src/main.rs && touch src/lib.rs \
    && cargo build --release --bin bot-cooker 2>/dev/null || true
RUN rm -rf src templates

# Build the real thing. Templates are embedded into the binary at build time.
COPY src ./src
COPY templates ./templates
RUN touch src/main.rs src/lib.rs && cargo build --release --bin bot-cooker

# ---- runtime stage ----
FROM debian:trixie-slim AS runtime
LABEL org.opencontainers.image.title="bot-cooker" \
      org.opencontainers.image.description="Defensive HTTP tarpit: answers automated scanning slowly with convincing decoy content." \
      org.opencontainers.image.source="https://github.com/koval01/bot-cooker" \
      org.opencontainers.image.documentation="https://koval01.github.io/bot-cooker/" \
      org.opencontainers.image.licenses="Apache-2.0"
RUN useradd --system --uid 10001 cooker \
    && mkdir -p /app && chown cooker:cooker /app
WORKDIR /app

COPY --from=build /app/target/release/bot-cooker /usr/local/bin/bot-cooker
# Readable by `cooker` whatever mode the file had on the build host.
COPY --chmod=0644 bot-cooker.yaml /app/bot-cooker.yaml

USER cooker
EXPOSE 8080
ENTRYPOINT ["/usr/local/bin/bot-cooker"]
CMD ["--config", "/app/bot-cooker.yaml"]
