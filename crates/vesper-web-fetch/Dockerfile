FROM docker.io/library/rust:1.88-bookworm@sha256:af306cfa71d987911a781c37b59d7d67d934f49684058f96cf72079c3626bfe0 AS build
WORKDIR /src
COPY Cargo.toml Cargo.lock ./
COPY crates ./crates
COPY apps ./apps
COPY xtask ./xtask
RUN cargo build --locked --release -p vesper-web-fetch

FROM docker.io/library/debian:bookworm-slim@sha256:88200866dfff7ea7f5cbcb6ec7c8a701889efe6fe859fe64d6990e4b07ea4171
ARG HEADLESS_VERSION=152.0.7977.82-1~deb12u1
RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates chromium-headless-shell=${HEADLESS_VERSION} \
    && apt-get clean
COPY --from=build /src/target/release/vesper-web-fetch /usr/local/bin/vesper-web-fetch
COPY crates/vesper-web-fetch/browser-pipe.sh /usr/local/bin/vesper-browser-pipe
RUN chmod 0755 /usr/local/bin/vesper-browser-pipe
ENV HOME=/tmp/vesper-browser
CMD ["sleep", "120"]
