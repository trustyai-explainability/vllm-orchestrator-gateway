use axum::http::HeaderMap;
use axum::{
    http::StatusCode,
    response::{IntoResponse, Sse, Response},
    routing::post,
    Json, Router
};
use axum::response::sse::{Event, KeepAlive};
use config::{validate_registered_detectors, DetectorConfig, GatewayConfig};
use futures::StreamExt;
use serde_json::json;
use serde_json::{Map, Value};
use std::error::Error;
use std::sync::Arc;
use std::{
    collections::HashMap,
    env,
    net::{IpAddr, SocketAddr},
};
use tower_http::trace::{self, TraceLayer};
use tracing::Level;
use anyhow::Context;

mod api;
mod config;

use api::{
    Detections, GenerationChoice, GenerationMessage, OrchestratorDetector, OrchestratorResponse,
    StreamingResponse, StreamingDelta,
};

fn get_orchestrator_detectors(
    detectors: Vec<String>,
    detector_config: Vec<DetectorConfig>,
) -> OrchestratorDetector {
    let mut input_detectors = HashMap::new();
    let mut output_detectors = HashMap::new();

    for detector in detector_config {
        if detectors.contains(&detector.name) && detector.detector_params.is_some() {
            let detector_params = detector.detector_params.unwrap();
            let key = detector.server.clone().unwrap_or_else(|| detector.name.clone());
            if detector.input {
                input_detectors.insert(key.clone(), detector_params.clone());
            }
            if detector.output {
                output_detectors.insert(key, detector_params);
            }
        }
    }

    OrchestratorDetector {
        input: input_detectors,
        output: output_detectors,
    }
}

#[tokio::main]
async fn main() {
    let config_path = env::var("GATEWAY_CONFIG").unwrap_or("config/config.yaml".to_string());
    tracing::debug!("Using config path: {}", config_path);
    let gateway_config = config::read_config(&config_path);
    tracing::debug!("Loaded gateway config: {:?}", gateway_config);
    validate_registered_detectors(&gateway_config);
    tracing::debug!("Validated registered detectors");

    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::DEBUG)
        .with_target(false)
        .compact()
        .init();

    let (client, scheme) =
        build_orchestrator_client(&gateway_config.orchestrator.host)
            .expect("Failed to build HTTP(s) client for communicating with orchestrator");
    let orchestrator_client = Arc::new(client);

    let mut app = Router::new().layer(
        TraceLayer::new_for_http()
            .make_span_with(trace::DefaultMakeSpan::new().level(Level::INFO))
            .on_response(trace::DefaultOnResponse::new().level(Level::INFO)),
    );

    for route in gateway_config.routes.iter() {
        let gateway_config = gateway_config.clone();
        let detectors = route.detectors.clone();
        let path = format!("/{}/v1/chat/completions", route.name);
        let fallback_message = route.fallback_message.clone();
        let orchestrator_client = orchestrator_client.clone();
        let scheme = scheme.clone();

        // Single endpoint that handles both streaming and non-streaming based on payload
        app = app.route(
            &path,
            post(move |headers: HeaderMap, Json(payload): Json<serde_json::Value>| async move {
                handle_chat_completions(
                    headers,
                    Json(payload),
                    detectors,
                    gateway_config,
                    fallback_message,
                    orchestrator_client,
                    scheme,
                ).await
            }),
        );

        tracing::info!("exposed endpoint: {}", path);
    }

    let mut http_port = 8090;
    if let Ok(port) = env::var("HTTP_PORT") {
        match port.parse::<u16>() {
            Ok(port) => {
                tracing::debug!("Using HTTP_PORT from env: {}", port);
                http_port = port
            }
            Err(err) => {
                tracing::error!("Failed to parse HTTP_PORT: {}", err);
                println!("{}", err)
            }
        }
    }

    let host = env::var("HOST").unwrap_or_else(|_| "0.0.0.0".to_string());
    tracing::debug!("Using host: {}", host);

    let ip: IpAddr = host.parse().expect("Failed to parse host IP address");
    let addr = SocketAddr::from((ip, http_port));
    tracing::debug!("Binding to address: {}", addr);

    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    tracing::info!("listening on {}", addr);

    axum::serve(listener, app).await.unwrap();
}

fn check_payload_detections(
    detections: &Option<Detections>,
    route_fallback_message: Option<String>,
) -> Option<GenerationChoice> {
    if let (Some(fallback_message), Some(_)) = (route_fallback_message, detections) {
        return Some(GenerationChoice {
            message: GenerationMessage::new(fallback_message),
            finish_reason: String::from("stop"),
            index: 0,
            logprobs: None,
        });
    }

    None
}

async fn handle_chat_completions(
    headers: HeaderMap,
    Json(payload): Json<serde_json::Value>,
    detectors: Vec<String>,
    gateway_config: GatewayConfig,
    route_fallback_message: Option<String>,
    orchestrator_client: Arc<reqwest::Client>,
    scheme: String,
) -> Result<Response, (StatusCode, String)> {
    tracing::debug!("handle_chat_completions called with payload: {:?}", payload);

    // Check if streaming is requested
    let is_streaming = payload
        .as_object()
        .and_then(|obj| obj.get("stream"))
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    let result = if is_streaming {
        handle_streaming_generation(
            headers,
            Json(payload),
            detectors,
            gateway_config,
            route_fallback_message,
            orchestrator_client,
            scheme,
        )
        .await
        .map(|response| response.into_response())
    } else {
        handle_non_streaming_generation(
            headers,
            Json(payload),
            detectors,
            gateway_config,
            route_fallback_message,
            orchestrator_client,
            scheme,
        )
        .await
        .map(|response| response.into_response())
    };

    result
}

async fn handle_non_streaming_generation(
    headers: HeaderMap,
    Json(mut payload): Json<serde_json::Value>,
    detectors: Vec<String>,
    gateway_config: GatewayConfig,
    route_fallback_message: Option<String>,
    orchestrator_client: Arc<reqwest::Client>,
    scheme: String,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    tracing::debug!("handle_non_streaming_generation called with payload: {:?}", payload);

    let orchestrator_detectors =
        get_orchestrator_detectors(detectors.clone(), gateway_config.detectors.clone());
    tracing::debug!("Orchestrator detectors: {:?}", orchestrator_detectors);

    let mut payload = payload.as_object_mut();

    let url: String = match gateway_config.orchestrator.port {
        Some(port) => format!(
            "{}://{}:{}/api/v2/chat/completions-detection",
            scheme,
            gateway_config.orchestrator.host,
            port
        ),
        None => format!(
            "{}://{}/api/v2/chat/completions-detection",
            scheme,
            gateway_config.orchestrator.host
        ),
    };
    tracing::debug!("Orchestrator URL: {}", url);

    payload.as_mut().unwrap().insert(
        "detectors".to_string(),
        serde_json::to_value(&orchestrator_detectors).unwrap(),
    );
    tracing::debug!("Payload after inserting detectors: {:?}", payload);

    let response_result =
        orchestrator_post_request(payload, &headers, &url, &orchestrator_client).await;

    match response_result {
        Ok(mut orchestrator_response) => {
            let detection =
                check_payload_detections(&orchestrator_response.detections, route_fallback_message);
            if let Some(message) = detection {
                tracing::debug!("Fallback message triggered: {:?}", message);
                orchestrator_response.choices = vec![message];
            }
            Ok(Json(json!(orchestrator_response)).into_response())
        }
        Err(_) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            response_result.err().unwrap().to_string(),
        )),
    }
}

async fn handle_streaming_generation(
    headers: HeaderMap,
    Json(mut payload): Json<serde_json::Value>,
    detectors: Vec<String>,
    gateway_config: GatewayConfig,
    route_fallback_message: Option<String>,
    orchestrator_client: Arc<reqwest::Client>,
    scheme: String,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    tracing::debug!("handle_streaming_generation called with payload: {:?}", payload);

    let orchestrator_detectors =
        get_orchestrator_detectors(detectors.clone(), gateway_config.detectors.clone());
    tracing::debug!("Orchestrator detectors: {:?}", orchestrator_detectors);

    let mut payload = payload.as_object_mut();

    let url: String = match gateway_config.orchestrator.port {
        Some(port) => format!(
            "{}://{}:{}/api/v2/chat/completions-detection",
            scheme,
            gateway_config.orchestrator.host,
            port
        ),
        None => format!(
            "{}://{}/api/v2/chat/completions-detection",
            scheme,
            gateway_config.orchestrator.host
        ),
    };
    tracing::debug!("Orchestrator URL: {}", url);

    payload.as_mut().unwrap().insert(
        "detectors".to_string(),
        serde_json::to_value(&orchestrator_detectors).unwrap(),
    );
    tracing::debug!("Payload after inserting detectors: {:?}", payload);

    let response_result =
        orchestrator_streaming_request(payload, &headers, &url, &orchestrator_client).await;

    match response_result {
        Ok(stream) => {
            let sse_stream = stream.map(move |chunk_result| -> Result<Event, anyhow::Error> {
                match chunk_result {
                    Ok(chunk) => {
                        // Check if we need to apply fallback message
                        if let Ok(mut streaming_response) = serde_json::from_str::<StreamingResponse>(&chunk) {
                            if let Some(fallback_message) = &route_fallback_message {
                                if streaming_response.detections.is_some() {
                                    // Apply fallback message to the first chunk
                                    if streaming_response.choices.len() > 0 {
                                        streaming_response.choices[0].delta = StreamingDelta {
                                            content: Some(fallback_message.clone()),
                                            role: Some("assistant".to_string()),
                                            tool_calls: None,
                                        };
                                        streaming_response.choices[0].finish_reason = Some("stop".to_string());
                                    }
                                }
                            }

                            match serde_json::to_string(&streaming_response) {
                                Ok(json_str) => Ok(Event::default().data(json_str)),
                                Err(e) => {
                                    tracing::error!("Failed to serialize streaming response: {}", e);
                                    Ok(Event::default().data("{\"error\": \"serialization failed\"}"))
                                }
                            }
                        } else {
                            // If it's not a valid JSON chunk, pass it through as-is
                            Ok(Event::default().data(chunk))
                        }
                    }
                    Err(e) => {
                        tracing::error!("Error processing streaming chunk: {}", e);
                        Ok(Event::default().data(format!("{{\"error\": \"{}\"}}", e)))
                    }
                }
            });

            Ok(Sse::new(sse_stream)
                .keep_alive(KeepAlive::default())
                .into_response())
        }
        Err(e) => {
            tracing::error!("Streaming request failed: {}", e);
            Err((StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))
        }
    }
}

fn build_orchestrator_client(hostname: &str) -> Result<(reqwest::Client, String), anyhow::Error> {
    use openssl::pkcs12::Pkcs12;
    use openssl::pkey::PKey;
    use openssl::x509::X509;
    use reqwest::tls::{Certificate, Identity};
    use reqwest::Client;
    use std::fs;

    let cert_path = "/etc/tls/private/tls.crt";
    let key_path = "/etc/tls/private/tls.key";
    let ca_path = "/etc/tls/ca/service-ca.crt";

    let mut builder = Client::builder();
    let mut scheme = String::from("http");

    // Add custom CA if it exists
    if fs::metadata(ca_path).is_ok() {
        let ca_cert = fs::read(ca_path)?;
        let ca = Certificate::from_pem(&ca_cert)?;
        tracing::debug!("Adding custom CA certificate from {}", ca_path);
        builder = builder.add_root_certificate(ca);
        if hostname == "localhost" {
            builder = builder.danger_accept_invalid_hostnames(true); // the orchestrator's certificate is only valid for the service's DNS name
        }
    }

    if fs::metadata(cert_path).is_ok() && fs::metadata(key_path).is_ok() {
        tracing::debug!("TLS cert and key found at {} and {}", cert_path, key_path);
        let cert_pem = fs::read(cert_path)?;
        let key_pem = fs::read(key_path)?;

        // Load cert and key using openssl
        let cert = X509::from_pem(&cert_pem)?;
        let key = PKey::private_key_from_pem(&key_pem)?;

        // Create PKCS#12 archive in memory (no password)
        let mut pk_builder = Pkcs12::builder();
        pk_builder.name("identity");
        pk_builder.pkey(&key);
        pk_builder.cert(&cert);
        let pkcs12 = pk_builder.build2("")?;
        let pkcs12_der = pkcs12.to_der()?;

        // Load as native-tls Identity
        let identity = Identity::from_pkcs12_der(&pkcs12_der, "")?;

        builder = builder.identity(identity);

        // set https
        scheme = String::from("https");
    } else {
        tracing::warn!("mTLS enabled but TLS cert or key not found, using default client");
    };

    Ok((builder.build()?, scheme))
}

async fn orchestrator_post_request(
    payload: Option<&mut Map<String, Value>>,
    headers: &HeaderMap,
    url: &str,
    client: &reqwest::Client,
) -> Result<OrchestratorResponse, anyhow::Error> {
    tracing::debug!(
        "Sending POST request to {} with payload: {:?}",
        url,
        payload
    );

    let mut req = client.post(url).json(&payload);

    // Forward authorization headers
    for (name, value) in headers.iter() {
        // filter out headers t
        tracing::debug!("Header {}: {:?}", name, value);
        let name_str = name.as_str().to_ascii_lowercase();
        if name_str == "authorization" {
            req = req.header(name, value);
        }
        if name_str.starts_with("x-forwarded") {
            req = req.header(name, value);
        }
    }

    let response_result = req.send().await;
    let response = match response_result {
        Ok(resp) => resp,
        Err(e) => {
            tracing::error!("Failed to send request or connect to orchestrator: {:?}", e);
            if let Some(source) = e.source() {
                tracing::error!("Underlying error: {:?}", source);
            }
            // print out the error chain for more details
            let mut source = e.source();
            while let Some(s) = source {
                tracing::error!("Caused by: {:?}", s);
                source = s.source();
            }
            return Err(anyhow::anyhow!(
                "Failed to send request or connect to orchestrator: {:?}",
                e
            ));
        }
    };

    let status = response.status();
    let text = response.text().await.unwrap_or_else(|e| {
        tracing::error!("Failed to read response body: {:?}", e);
        String::new()
    });
    tracing::debug!("Received response status: {}, body: {}", status, text);

    if !status.is_success() {
        // Return the error with the status code and response body
        tracing::error!("Orchestrator returned error status {}: {}", status, text);
        return Err(anyhow::anyhow!(
            "Orchestrator returned error status {}: {}",
            status,
            text
        ));
    }

    let json: serde_json::Value = serde_json::from_str(&text)?;
    tracing::debug!("Parsed JSON response: {:?}", json);
    Ok(serde_json::from_value(json).expect("unexpected json response from request"))
}

async fn orchestrator_streaming_request(
    payload: Option<&mut Map<String, Value>>,
    headers: &HeaderMap,
    url: &str,
    client: &reqwest::Client,
) -> Result<impl futures::Stream<Item = Result<String, anyhow::Error>>, anyhow::Error> {
    tracing::debug!(
        "Sending streaming POST request to {} with payload: {:?}",
        url,
        payload
    );

    let mut req = client.post(url).json(&payload);

    // Forward authorization headers
    for (name, value) in headers.iter() {
        tracing::debug!("Header {}: {:?}", name, value);
        let name_str = name.as_str().to_ascii_lowercase();
        if name_str == "authorization" {
            req = req.header(name, value);
        }
        if name_str.starts_with("x-forwarded") {
            req = req.header(name, value);
        }
    }

    let response = req
        .send()
        .await
        .context("Failed to send request or connect to orchestrator: {}")?;

    let status = response.status();
    if !status.is_success() {
        let error_text = response.text().await.unwrap_or_else(|_| "Unknown error".to_string());
        let err_msg = format!("Orchestrator returned error status {}: {}", status, error_text);
        tracing::error!("{}", err_msg);
        anyhow::bail!(err_msg);
    }

    let stream = response.bytes_stream();
    let chunk_stream = stream.map(|chunk_result| {
        chunk_result
            .map_err(|e| anyhow::anyhow!("Failed to read chunk: {}", e))
            .and_then(|chunk| {
                let chunk_str = String::from_utf8(chunk.to_vec())
                    .map_err(|e| anyhow::anyhow!("Invalid UTF-8 in chunk: {}", e))?;

                // Parse SSE format and extract data
                let lines: Vec<&str> = chunk_str.lines().collect();
                let mut data_lines = Vec::new();

                for line in lines {
                    if line.starts_with("data: ") {
                        let data = &line[6..]; // Remove "data: " prefix
                        if data != "[DONE]" {
                            data_lines.push(data.to_string());
                        }
                    }
                }

                if data_lines.is_empty() {
                    Ok("".to_string())
                } else {
                    Ok(data_lines.join("\n"))
                }
            })
    });

    Ok(chunk_stream)
}
