# Based on: https://levelup.gitconnected.com/create-an-optimized-rust-alpine-docker-image-1940db638a6c

##### Builder
FROM rust:1.62.0-slim as builder

WORKDIR /usr/src

RUN apt update && apt-get install musl-tools -y
# Create blank project
RUN USER=root cargo new externaldns-srv-companion

# We want dependencies cached, so copy those first.
COPY Cargo.toml Cargo.lock /usr/src/externaldns-srv-companion/

# Set the working directory
WORKDIR /usr/src/externaldns-srv-companion

RUN rustup target add x86_64-unknown-linux-musl

# This is a dummy build to get the dependencies cached.
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/usr/src/externaldns-srv-companion/target \
    cargo build --target x86_64-unknown-linux-musl --release

COPY src /usr/src/externaldns-srv-companion/src/
RUN touch /usr/src/externaldns-srv-companion/src/main.rs

# Build it for real with caching, and copy the resulting binary
# into /usr/local/bin since cache directories become inaccessible
# at the end of the running command (apparently)
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/usr/src/externaldns-srv-companion/target \
    cargo build --target x86_64-unknown-linux-musl --release && \
    cp /usr/src/externaldns-srv-companion/target/x86_64-unknown-linux-musl/release/externaldns-srv-companion /usr/local/bin

##### Runtime
FROM alpine:3.16.0 AS runtime 

COPY --from=builder /usr/local/bin /usr/local/bin

VOLUME /data
WORKDIR /data

ENTRYPOINT ["/usr/local/bin/externaldns-srv-companion"]