FROM rust:bookworm AS builder

WORKDIR /app
COPY Cargo.toml Cargo.lock ./
COPY src ./src
RUN cargo build --release --locked

FROM debian:bookworm-slim

ENV CHROME=/usr/bin/google-chrome-stable \
    HEADLESS=false

RUN apt-get update \
    && apt-get install --no-install-recommends -y ca-certificates gnupg wget xauth xvfb \
    && mkdir -p /etc/apt/keyrings \
    && wget -qO- https://dl.google.com/linux/linux_signing_key.pub \
        | gpg --dearmor -o /etc/apt/keyrings/google-chrome.gpg \
    && echo "deb [arch=amd64 signed-by=/etc/apt/keyrings/google-chrome.gpg] https://dl.google.com/linux/chrome/deb/ stable main" \
        > /etc/apt/sources.list.d/google-chrome.list \
    && apt-get update \
    && apt-get install --no-install-recommends -y google-chrome-stable \
    && chown root:root /opt/google/chrome/chrome-sandbox \
    && chmod 4755 /opt/google/chrome/chrome-sandbox \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder /app/target/release/page-warmup /usr/local/bin/page-warmup
COPY docker-entrypoint.sh /usr/local/bin/docker-entrypoint
RUN chmod 755 /usr/local/bin/docker-entrypoint \
    && useradd --system --uid 10001 --create-home app

USER app
ENTRYPOINT ["/usr/local/bin/docker-entrypoint"]
