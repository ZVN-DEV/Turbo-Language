FROM rust:1.83-slim as builder

WORKDIR /build
COPY turbo/ ./turbo/
RUN cd turbo && cargo build --release

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y curl && rm -rf /var/lib/apt/lists/*
COPY --from=builder /build/turbo/target/release/turbo /usr/local/bin/turbo
ENTRYPOINT ["turbo"]
CMD ["repl"]
