use axum::http::HeaderMap;
use axum::{http::StatusCode, response::IntoResponse, routing::post, Json, Router};
use config::{validate_registered_detectors, DetectorConfig, GatewayConfig};
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

mod api;
mod config;

use api::{
    Detections, GenerationChoice, GenerationMessage, OrchestratorDetector, OrchestratorResponse,
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
            if detector.input {
                input_detectors.insert(detector.name.clone(), detector_params.clone());
            }
            if detector.output {
                output_detectors.insert(detector.name, detector_params);
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

    let orchestrator_client = Arc::new(
        build_orchestrator_client(&gateway_config.orchestrator.host)
            .expect("Failed to build HTTP(s) client for communicating with orchestrator"),
    );

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
        app = app.route(
            &path,
            post(
                |headers: HeaderMap, Json(payload): Json<serde_json::Value>| {
                    handle_generation(
                        headers,
                        Json(payload),
                        detectors,
                        gateway_config,
                        fallback_message,
                        orchestrator_client,
                    )
                },
            ),
        );
        tracing::info!("exposed endpoints: {}", path);
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

async fn handle_generation(
    headers: HeaderMap,
    Json(mut payload): Json<serde_json::Value>,
    detectors: Vec<String>,
    gateway_config: GatewayConfig,
    route_fallback_message: Option<String>,
    orchestrator_client: Arc<reqwest::Client>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    tracing::debug!("handle_generation called with payload: {:?}", payload);

    let orchestrator_detectors =
        get_orchestrator_detectors(detectors.clone(), gateway_config.detectors.clone());
    tracing::debug!("Orchestrator detectors: {:?}", orchestrator_detectors);

    let mut payload = payload.as_object_mut();

    let url: String = match gateway_config.orchestrator.port {
        Some(port) => format!(
            "https://{}:{}/api/v2/chat/completions-detection",
            gateway_config.orchestrator.host, port
        ),
        None => format!(
            "https://{}/api/v2/chat/completions-detection",
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

fn build_orchestrator_client(hostname: &str) -> Result<reqwest::Client, anyhow::Error> {
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
    } else {
        tracing::warn!("mTLS enabled but TLS cert or key not found, using default client");
    };

    Ok(builder.build()?)
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
