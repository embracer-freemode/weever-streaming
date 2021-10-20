FROM rust:1.55.0-slim as builder

# dependencies
# RUN apt update
# RUN apt install -y pkg-config libssl-dev

ARG TRIPLE=x86_64-unknown-linux-gnu
ARG PROJ=webrtc-sfu
# build
RUN rustup target add ${TRIPLE}
ADD Cargo.toml Cargo.toml
ADD Cargo.lock Cargo.lock
# fetch all dependencies as cache
RUN mkdir -p .cargo && cargo vendor > .cargo/config
# dummy build to build all dependencies as cache
RUN mkdir src/ && echo "" > src/lib.rs && cargo build --lib --release --target ${TRIPLE} && rm -f src/lib.rs
# get real code in
COPY . .
RUN touch src/lib.rs && cargo build --release --bin ${PROJ} --target ${TRIPLE}
RUN strip target/${TRIPLE}/release/${PROJ}

##########

FROM debian:bullseye-20211011-slim

ARG TRIPLE=x86_64-unknown-linux-gnu
ARG PROJ=webrtc-sfu
COPY --from=builder /target/${TRIPLE}/release/${PROJ} .

# log current git commit hash for future investigation (need to pass in from outside)
ARG COMMIT_SHA
RUN echo ${COMMIT_SHA} > /commit

CMD ./webrtc-sfu
