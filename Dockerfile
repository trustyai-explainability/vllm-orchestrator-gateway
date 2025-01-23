FROM rust:latest

WORKDIR /app

COPY Cargo.toml .

COPY src/ /app/src/

RUN cargo build --release

COPY target/release/* .

EXPOSE 8080

CMD ["/app/vllm-orchestrator-gateway"]
