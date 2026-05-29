ARG BASE_IMAGE=registry.access.redhat.com/ubi9/ubi-minimal:9.8@sha256:5b74fce9d6e629942a0c6dc0f546c193e70d7f974d999a48c948c53dd3d36362

## Rust builder ################################################################
FROM ${BASE_IMAGE} AS rust-builder

RUN microdnf install -y --disableplugin=subscription-manager \
        rust \
        cargo \
        gcc \
        openssl-devel && \
    microdnf clean all --disableplugin=subscription-manager

WORKDIR /app

COPY Cargo.toml Cargo.lock /app/
COPY src/ /app/src/

RUN cargo install --root /app/ --path .

## Release image ################################################################
FROM ${BASE_IMAGE} AS release

COPY --from=rust-builder /app/bin/ /app/bin/
COPY config/config.yaml /app/config/config.yaml

USER 1001

ENV GATEWAY_CONFIG=/app/config/config.yaml
CMD ["/app/bin/vllm-orchestrator-gateway"]

LABEL com.redhat.component="odh-trustyai-vllm-orchestrator-gateway-rhel9" \
      name="rhoai/odh-trustyai-vllm-orchestrator-gateway-rhel9" \
      io.k8s.display-name="odh-trustyai-vllm-orchestrator-gateway-rhel9" \
      io.k8s.description="OpenAI-compatible REST gateway that routes chat requests with configurable detector-based filtering and route-specific safety policies" \
      description="OpenAI-compatible REST gateway that routes chat requests with configurable detector-based filtering and route-specific safety policies" \
      summary="vLLM Orchestrator Gateway for detector-based chat completion filtering" \
      com.redhat.license_terms="https://www.redhat.com/licenses/Red_Hat_Standard_EULA_20191108.pdf"
