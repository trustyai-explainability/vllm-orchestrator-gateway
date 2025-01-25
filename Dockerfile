FROM rust:latest

WORKDIR /app

COPY Cargo.toml .

COPY src/ /app/src/

COPY config/config.yaml /app/config/config.yaml

RUN cargo build --release

COPY target/release/* .

EXPOSE 8080

ENV ORCHESTRATOR_CONFIG=/app/config/config.yaml

CMD ["/app/vllm-orchestrator-gateway"]
