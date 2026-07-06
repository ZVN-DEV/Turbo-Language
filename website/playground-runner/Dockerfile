FROM rust:1.88-slim AS builder

WORKDIR /build
COPY turbo/ ./turbo/
COPY docs/errors/ ./docs/errors/
RUN cd turbo && cargo build --release -p turbo-cli

FROM node:22-bookworm-slim

RUN groupadd --system turbo && useradd --system --gid turbo --home-dir /tmp turbo
COPY --from=builder /build/turbo/target/release/turbolang /usr/local/bin/turbolang
COPY website/playground-runner/runner.mjs /app/runner.mjs

ENV NODE_ENV=production
ENV PORT=8787
ENV TURBO_PLAYGROUND_RUNNER_BIN=/usr/local/bin/turbolang

USER turbo
WORKDIR /tmp
EXPOSE 8787

HEALTHCHECK --interval=30s --timeout=3s --start-period=10s --retries=3 \
  CMD ["node", "-e", "const port = process.env.PORT || '8787'; fetch('http://127.0.0.1:' + port + '/healthz').then((r) => process.exit(r.ok ? 0 : 1)).catch(() => process.exit(1))"]

CMD ["node", "/app/runner.mjs"]
