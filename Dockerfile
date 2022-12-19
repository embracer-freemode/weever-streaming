FROM rust:1.66.0-slim as builder

ARG TRIPLE=x86_64-unknown-linux-gnu
ARG PROJ=webrtc-sfu
# build
RUN rustup target add ${TRIPLE}
ADD Cargo.toml Cargo.toml
ADD Cargo.lock Cargo.lock
# fetch all dependencies as cache
RUN mkdir -p .cargo && cargo vendor > .cargo/config
# dummy build to build all dependencies as cache
RUN mkdir src/ && echo "fn main() {}" > src/main.rs && cargo build --bin ${PROJ} --release --target ${TRIPLE} && rm -f src/main.rs
# get real code in
COPY . .
RUN touch src/main.rs && cargo build --release --bin ${PROJ} --target ${TRIPLE} --features release_max_level_debug
RUN strip target/${TRIPLE}/release/${PROJ}

##########

FROM debian:bullseye-20221205-slim

ARG TRIPLE=x86_64-unknown-linux-gnu
ARG PROJ=webrtc-sfu
COPY --from=builder /target/${TRIPLE}/release/${PROJ} /usr/local/bin/
COPY site site
# debug certs, you should override it in production
COPY certs certs

# log current git commit hash for future investigation (need to pass in from outside)
ARG COMMIT_SHA
RUN echo ${COMMIT_SHA} > /commit

ENV RUST_LOG=info,webrtc_mdns=error,webrtc_srtp=info
CMD webrtc-sfu --cert-file certs/cert.pem --key-file certs/key.pem
