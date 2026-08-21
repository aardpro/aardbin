# ---- Stage 1: frontend (Tailwind CSS) ----
FROM node:22-alpine AS frontend
WORKDIR /build
COPY package.json package-lock.json ./
RUN npm ci --no-audit --no-fund
COPY styles/ ./styles/
COPY templates/ ./templates/
COPY static/ ./static/
RUN npx tailwindcss -i styles/app.css -o /out/app.css --minify

# ---- Stage 2: rust build ----
FROM rust:bookworm AS rust
WORKDIR /build
COPY Cargo.toml Cargo.lock ./
COPY src/cli/Cargo.toml src/cli/Cargo.toml
COPY src/ ./src/
RUN cargo build --release --workspace

# ---- Stage 3: runtime ----
FROM debian:bookworm-slim
RUN useradd --create-home --uid 10001 aardbin
COPY --from=rust /build/target/release/aardbin /usr/local/bin/aardbin
COPY --from=rust /build/target/release/aardbin-cli /usr/local/bin/aardbin-cli
COPY templates/ /app/templates/
COPY static/ /app/static/
COPY --from=frontend /out/app.css /app/static/app.css
COPY entrypoint.sh /app/entrypoint.sh
RUN chmod +x /app/entrypoint.sh
WORKDIR /app
RUN mkdir -p /app/data && chown -R aardbin:aardbin /app
ENV LISTEN_ADDR=0.0.0.0:8080 \
    DATA_DIR=/app/data \
    TEMPLATES_DIR=/app/templates \
    STATIC_DIR=/app/static
VOLUME /app/data
EXPOSE 8080
HEALTHCHECK --interval=30s --timeout=3s --start-period=5s --retries=3 \
  CMD bash -c 'exec 3<>/dev/tcp/127.0.0.1/8080 && printf "GET /healthz HTTP/1.0\r\n\r\n" >&3 && grep -q "200" <&3' || exit 1
ENTRYPOINT ["/app/entrypoint.sh"]
