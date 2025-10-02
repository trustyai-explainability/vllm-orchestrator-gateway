use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Serialize)]
pub struct OrchestratorDetector {
    pub input: HashMap<String, serde_json::Value>,
    pub output: HashMap<String, serde_json::Value>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct GenerationMessage {
    pub content: String,
    pub refusal: Option<String>,
    pub role: String,
    pub tool_calls: Option<serde_json::Value>,
    pub audio: Option<serde_json::Value>,
}

impl GenerationMessage {
    pub fn new(message: String) -> Self {
        GenerationMessage {
            content: message,
            refusal: None,
            role: String::from("assistant"),
            tool_calls: None,
            audio: None,
        }
    }
}

#[derive(Serialize, Deserialize, Debug)]
pub struct GenerationChoice {
    pub finish_reason: String,
    pub index: u32,
    pub message: GenerationMessage,
    pub logprobs: Option<serde_json::Value>,
}

#[derive(Serialize, Deserialize, Debug)]
struct DetectionResult {
    start: serde_json::Value,
    end: u32,
    text: String,
    detection_type: String,
    detection: String,
    detector_id: String,
    score: f64,
}

#[derive(Serialize, Deserialize, Debug)]
struct InputDetection {
    message_index: u16,
    results: Option<Vec<DetectionResult>>,
}

#[derive(Serialize, Deserialize, Debug)]
struct OutputDetection {
    choice_index: u32,
    results: Option<Vec<DetectionResult>>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct Detections {
    input: Option<Vec<InputDetection>>,
    output: Option<Vec<OutputDetection>>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct OrchestratorResponse {
    id: String,
    pub choices: Vec<GenerationChoice>,
    created: u64,
    model: String,
    service_tier: Option<String>,
    system_fingerprint: Option<String>,
    object: Option<String>,
    usage: serde_json::Value,
    pub detections: Option<Detections>,
    pub warnings: Option<Vec<HashMap<String, String>>>,
}

// Streaming response structures
#[derive(Serialize, Deserialize, Debug)]
pub struct StreamingDelta {
    pub content: Option<String>,
    pub role: Option<String>,
    pub tool_calls: Option<serde_json::Value>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct StreamingChoice {
    pub index: u32,
    pub delta: StreamingDelta,
    pub logprobs: Option<serde_json::Value>,
    pub finish_reason: Option<String>,
    pub stop_reason: Option<String>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct StreamingResponse {
    pub id: String,
    pub object: String,
    pub created: u64,
    pub model: String,
    pub choices: Vec<StreamingChoice>,
    pub usage: Option<serde_json::Value>,
    pub detections: Option<Detections>,
    pub warnings: Option<Vec<HashMap<String, String>>>,
}
