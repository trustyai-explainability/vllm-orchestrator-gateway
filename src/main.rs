use axum::{http::StatusCode, response::IntoResponse, routing::post, Json, Router};
use config::{validate_registered_detectors, DetectorConfig, GatewayConfig};
use serde_json::json;
use serde_json::{Map, Value};
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
    let gateway_config = config::read_config(&config_path);
    validate_registered_detectors(&gateway_config);

    tracing_subscriber::fmt()
        .with_target(false)
        .compact()
        .init();

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
        app = app.route(
            &path,
            post(|Json(payload): Json<serde_json::Value>| {
                handle_generation(Json(payload), detectors, gateway_config, fallback_message)
            }),
        );
        tracing::info!("exposed endpoints: {}", path);
    }

    let mut http_port = 8090;
    if let Ok(port) = env::var("HTTP_PORT") {
        match port.parse::<u16>() {
            Ok(port) => http_port = port,
            Err(err) => println!("{}", err),
        }
    }

    let host = env::var("HOST").unwrap_or_else(|_| "0.0.0.0".to_string());

    let ip: IpAddr = host.parse().expect("Failed to parse host IP address");
    let addr = SocketAddr::from((ip, http_port));

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
    Json(mut payload): Json<serde_json::Value>,
    detectors: Vec<String>,
    gateway_config: GatewayConfig,
    route_fallback_message: Option<String>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let orchestrator_detectors = get_orchestrator_detectors(detectors, gateway_config.detectors);

    let mut payload = payload.as_object_mut();

    let url: String = match gateway_config.orchestrator.port {
        Some(port) => format!(
            "http://{}:{}/api/v2/chat/completions-detection",
            gateway_config.orchestrator.host, port
        ),
        None => format!(
            "https://{}/api/v2/chat/completions-detection",
            gateway_config.orchestrator.host
        ),
    };

    payload.as_mut().unwrap().insert(
        "detectors".to_string(),
        serde_json::to_value(&orchestrator_detectors).unwrap(),
    );

    let response_result = orchestrator_post_request(payload, &url).await;

    match response_result {
        Ok(mut orchestrator_response) => {
            let detection =
                check_payload_detections(&orchestrator_response.detections, route_fallback_message);
            if let Some(message) = detection {
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

async fn orchestrator_post_request(
    payload: Option<&mut Map<String, Value>>,
    url: &str,
) -> Result<OrchestratorResponse, anyhow::Error> {
    let client = reqwest::Client::new();
    let response = client.post(url).json(&payload).send();

    let json = response.await?.json().await?;
    Ok(serde_json::from_value(json).expect("unexpected json response from request"))
}
