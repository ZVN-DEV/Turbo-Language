# Multi-stage build for a Turbo service on Cloud Run (or any Docker PaaS).
#
# Stage 1 installs the released Turbo toolchain (no compiler build) and
# AOT-compiles the app; stage 2 is a slim runtime image holding only the
# native binary. Build context is this directory — fully self-contained.

FROM debian:trixie-slim AS build

# gcc/libc6-dev: `turbolang build` links the C runtime via cc.
# trixie: the released turbolang binary needs glibc >= 2.39 (bookworm has 2.36).
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates curl gcc libc6-dev \
    && rm -rf /var/lib/apt/lists/*

# Install the released toolchain (pin with: bash -s -- --version X.Y.Z).
RUN curl -fsSL https://raw.githubusercontent.com/ZVN-DEV/Turbo-Language/master/distribution/install.sh | bash

WORKDIR /app
COPY main.tb .
RUN turbolang build main.tb -o server

FROM debian:trixie-slim
COPY --from=build /app/server /server

# Cloud Run injects PORT; the app reads it and binds 0.0.0.0.
ENV PORT=8080
EXPOSE 8080
CMD ["/server"]
