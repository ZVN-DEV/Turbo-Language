FROM rust:1.86-slim as builder

WORKDIR /build
COPY turbo/ ./turbo/
RUN cd turbo && cargo build --release -p turbo-cli -p turbo-lsp

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y curl && rm -rf /var/lib/apt/lists/*
COPY --from=builder /build/turbo/target/release/turbolang /usr/local/bin/turbolang
COPY --from=builder /build/turbo/target/release/turbo-lsp /usr/local/bin/turbo-lsp
ENTRYPOINT ["turbolang"]
CMD ["repl"]
