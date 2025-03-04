use std::fs;

use serde::Deserialize;

#[derive(Debug, Deserialize, Clone)]
pub struct GatewayConfig {
    pub orchestrator: OrchestratorConfig,
    pub detectors: Vec<DetectorConfig>,
    pub routes: Vec<RouteConfig>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct OrchestratorConfig {
    pub host: String,
    pub port: Option<u16>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct DetectorConfig {
    pub name: String,
    pub input: bool,
    pub output: bool,
    pub detector_params: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct RouteConfig {
    pub name: String,
    pub detectors: Vec<String>,
    pub fallback_message: Option<String>,
}

pub fn read_config(path: &str) -> GatewayConfig {
    let result = fs::read_to_string(path).expect(&format!("could not read file: {}", path));

    serde_yml::from_str(&result).expect("failed to read in yaml config")
}

pub fn validate_registered_detectors(gateway_cfg: &GatewayConfig) {
    let detector_names: Vec<&String> = gateway_cfg
        .detectors
        .iter()
        .map(|detector| &detector.name)
        .collect();

    for route in gateway_cfg.routes.iter() {
        for detector in &route.detectors {
            if !detector_names.contains(&detector) {
                panic!(
                    "could not find detector {} in route {}",
                    detector, route.name
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[should_panic]
    fn test_validate_registered_detectors() {
        let gc = GatewayConfig {
            orchestrator: OrchestratorConfig {
                host: "localhost".to_string(),
                port: Some(1234),
            },
            detectors: vec![DetectorConfig {
                name: "regex".to_string(),
                detector_params: None,
            }],
            routes: vec![RouteConfig {
                name: "route1".to_string(),
                detectors: vec!["regex".to_string(), "not_existent_detector".to_string()],
            }],
        };

        validate_registered_detectors(&gc);
    }
}
