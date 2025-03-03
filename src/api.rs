use std::collections::HashMap;
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize)]
pub(crate) struct OrchestratorDetector {
    pub(crate) input: HashMap<String, serde_json::Value>,
    pub(crate) output: HashMap<String, serde_json::Value>,
    // implement when output is completed, also need to see about splitting detectors in config to input/output
    // output: HashMap<String, serde_json::Value>,
}

#[derive(Serialize, Deserialize, Debug)]
pub(crate) struct GenerationMessage {
    pub(crate) content: String,
    pub(crate) refusal: Option<String>,
    pub(crate) role: String,
    pub(crate) tool_calls: Option<serde_json::Value>,
    pub(crate) audio: Option<serde_json::Value>,
}

#[derive(Serialize, Deserialize, Debug)]
pub(crate) struct GenerationChoice {
    pub(crate) finish_reason: String,
    pub(crate) index: u32,
    pub(crate) message: GenerationMessage,
    pub(crate) logprobs: Option<serde_json::Value>,
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
    results: Option<Vec<DetectionResult>>
}

#[derive(Serialize, Deserialize, Debug)]
struct OutputDetection {
    choice_index: u32,
    results: Option<Vec<DetectionResult>>
}

#[derive(Serialize, Deserialize, Debug)]
pub(crate) struct Detections {
    input: Option<Vec<InputDetection>>,
    output: Option<Vec<OutputDetection>>,
}


#[derive(Serialize, Deserialize, Debug)]
pub(crate) struct OrchestratorResponse {
    id: String,
    pub(crate) choices: Vec<GenerationChoice>,
    created: u64,
    model: String,
    service_tier: Option<String>,
    system_fingerprint: Option<String>,
    object: Option<String>,
    usage: serde_json::Value,
    pub(crate) detections: Option<Detections>,
    pub(crate) warnings: Option<Vec<HashMap<String, String>>>
}