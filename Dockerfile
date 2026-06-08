# Multi-stage build for the fusion-api service.
FROM rust:1-bookworm AS build
WORKDIR /app
COPY . .
RUN cargo build --release -p api

FROM debian:bookworm-slim
WORKDIR /app
RUN apt-get update \
    && apt-get install -y --no-install-recommends curl ca-certificates \
    && rm -rf /var/lib/apt/lists/*
COPY --from=build /app/target/release/fusion-api /usr/local/bin/fusion-api
# Runtime assets the service reads at request time.
COPY docs/openapi.yaml docs/openapi.yaml
COPY sim/scenarios sim/scenarios
# In a container we must bind the container interface; this is the explicit
# opt-in to a non-local bind (the binary still defaults to 127.0.0.1 elsewhere).
ENV FUSION_BIND=0.0.0.0:8088
ENV RUST_LOG=info
EXPOSE 8088
CMD ["fusion-api"]
