ARG UBI_MINIMAL_BASE_IMAGE=registry.access.redhat.com/ubi9/ubi-minimal
ARG UBI_BASE_IMAGE_TAG=latest

## Rust builder ################################################################
# Specific debian version so that compatible glibc version is used
FROM rust:1.84.0 AS rust-builder

WORKDIR /app

COPY rust-toolchain.toml rust-toolchain.toml

RUN rustup component add rustfmt

## Gateway builder #########################################################
FROM rust-builder AS gateway-builder

COPY *.toml /app/
COPY src/ /app/src/
# COPY config/config.yaml /app/config/config.yaml

WORKDIR /app

RUN cargo install --root /app/ --path .

## Tests stage ##################################################################
FROM gateway-builder AS tests
RUN cargo test

## Lint stage ###################################################################
FROM gateway-builder AS lint
RUN cargo clippy --all-targets --all-features -- -D warnings

## Formatting check stage #######################################################
FROM gateway-builder AS format
RUN cargo fmt --check

## Release Image ################################################################

FROM ${UBI_MINIMAL_BASE_IMAGE}:${UBI_BASE_IMAGE_TAG} AS gateway-release
ENV GATEWAY_CONFIG=/app/config/config.yaml
COPY config/config.yaml /app/config/config.yaml

COPY --from=gateway-builder /app/bin/ /app/bin/

RUN microdnf install -y --disableplugin=subscription-manager shadow-utils compat-openssl11 && \
    microdnf clean all --disableplugin=subscription-manager

CMD ["/app/bin/vllm-orchestrator-gateway"]
