FROM rust:1.85-bookworm AS builder

WORKDIR /build

COPY Cargo.toml Cargo.lock ./
COPY api/ api/
COPY cli/ cli/
COPY crates/ crates/

# Desktop/Tauri requires system GTK libs we don't need in the container,
# so only build api + cli.
RUN cargo build --release --package bob-api --package bob-cli

FROM debian:bookworm-slim

RUN apt-get update && \
    apt-get install -y --no-install-recommends ca-certificates && \
    rm -rf /var/lib/apt/lists/*

COPY --from=builder /build/target/release/bob-api /usr/local/bin/bob-api
COPY --from=builder /build/target/release/bob-cli /usr/local/bin/bob-cli

EXPOSE 8787

ENTRYPOINT ["bob-api"]
