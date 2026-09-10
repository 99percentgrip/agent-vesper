FROM docker.io/library/rust:1.88-bookworm@sha256:af306cfa71d987911a781c37b59d7d67d934f49684058f96cf72079c3626bfe0 AS build
WORKDIR /src
COPY Cargo.toml Cargo.lock ./
COPY crates ./crates
COPY apps ./apps
COPY xtask ./xtask
RUN cargo build --locked --release -p vesper-web-fetch

FROM docker.io/library/rust:1.88-bookworm@sha256:af306cfa71d987911a781c37b59d7d67d934f49684058f96cf72079c3626bfe0 AS confinement
WORKDIR /confinement
RUN apt-get update && apt-get install -y --no-install-recommends libcap-ng-dev \
    && apt-get clean
RUN curl --fail --location --proto '=https' --tlsv1.2 \
    https://www.kernel.org/pub/linux/utils/util-linux/v2.41/util-linux-2.41.6.tar.xz \
    --output source.tar.xz \
    && echo 'e596083744e746be7d2823b62b43f4418dd7bf56303b4dc09e6fe8112fe3d7ed  source.tar.xz' | sha256sum --check \
    && tar -xf source.tar.xz \
    && cd util-linux-2.41.6 \
    && ./configure --disable-all-programs --enable-setpriv --disable-nls --without-systemd \
    && make -j2 setpriv

FROM docker.io/library/debian:bookworm-slim@sha256:88200866dfff7ea7f5cbcb6ec7c8a701889efe6fe859fe64d6990e4b07ea4171
ARG HEADLESS_VERSION=152.0.7977.82-1~deb12u1
RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates chromium-headless-shell=${HEADLESS_VERSION} libcap-ng0 procps tini \
    && apt-get clean
COPY --from=build /src/target/release/vesper-web-fetch /usr/local/bin/vesper-web-fetch
COPY --from=confinement /confinement/util-linux-2.41.6/setpriv /usr/local/bin/vesper-setpriv
COPY --from=confinement /confinement/source.tar.xz /usr/local/share/vesper-setpriv/source.tar.xz
COPY crates/vesper-web-fetch/browser-pipe.sh /usr/local/bin/vesper-browser-pipe
RUN chmod 0755 /usr/local/bin/vesper-browser-pipe
ENV HOME=/tmp/vesper-browser
ENTRYPOINT ["/usr/bin/tini", "--"]
CMD ["sleep", "120"]
