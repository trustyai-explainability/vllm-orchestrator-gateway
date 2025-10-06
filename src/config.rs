use std::collections::HashSet;
use std::fs;

use serde::Deserialize;

#[derive(Debug, Deserialize, Clone)]
pub struct GatewayConfig {
    #[serde(default)]
    pub orchestrator: OrchestratorConfig,
    pub detectors: Vec<DetectorConfig>,
    pub routes: Vec<RouteConfig>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct OrchestratorConfig {
    pub host: String,
    pub port: Option<u16>,
}

impl Default for OrchestratorConfig {
    fn default() -> Self {
        OrchestratorConfig {
            host: "localhost".to_string(),
            port: Some(8032),
        }
    }
}

#[derive(Debug, Deserialize, Clone)]
pub struct DetectorConfig {
    pub name: String,
    #[serde(default)]
    pub server: Option<String>,
    pub input: bool,
    pub output: bool,
    pub detector_params: Option<serde_json::Value>,
}

impl DetectorConfig {
    pub fn with_server_default(mut self) -> Self {
        if self.server.is_none() {
            self.server = Some(self.name.clone());
        }
        self
    }
}

#[derive(Debug, Deserialize, Clone)]
pub struct RouteConfig {
    pub name: String,
    pub detectors: Vec<String>,
    pub fallback_message: Option<String>,
}


pub fn read_config(path: &str) -> GatewayConfig {
    let result = fs::read_to_string(path).expect(&format!("could not read file: {}", path));

    let mut cfg: GatewayConfig = serde_yml::from_str(&result).expect("failed to read in yaml config");
    cfg.detectors = cfg.detectors.into_iter().map(|d| d.with_server_default()).collect();
    cfg
}

pub fn validate_registered_detectors(gateway_cfg: &GatewayConfig) {
    let detector_names: Vec<&String> = gateway_cfg
        .detectors
        .iter()
        .map(|detector| &detector.name)
        .collect();

    let mut issues = Vec::new();
    for route in gateway_cfg.routes.iter() {
        for detector in &route.detectors {
            if !detector_names.contains(&detector) {
                issues.push(format!(
                    "- could not find detector '{}' in route '{}'",
                    detector, route.name
                ));
            }
        }

        // Validate no duplicate input/output servers
        let mut seen_input = HashSet::new();
        let mut seen_output = HashSet::new();

        for detector_name in &route.detectors {
            if let Some(detector_cfg) = gateway_cfg.detectors.iter().find(|d| &d.name == detector_name) {
                if detector_cfg.input {
                    let server = detector_cfg.server.as_ref().unwrap();
                    if !seen_input.insert(server) {
                        issues.push(format!(
                            "- route '{}' contains more than one input detector with server '{}'",
                            route.name, server
                        ));
                    }
                    if !seen_output.insert(server) {
                        issues.push(format!(
                            "- route '{}' contains more than one output detector with server '{}'",
                            route.name, server
                        ));
                    }
                }
            }
        }
    }
    if !issues.is_empty() {
        panic!("Config validation failed:\n{}", issues.join("\n"));
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
                server: None,
                input: false,
                output: false,
                detector_params: None,
            }],
            routes: vec![RouteConfig {
                name: "route1".to_string(),
                detectors: vec!["regex".to_string(), "not_existent_detector".to_string()],
                fallback_message: None,
            }],
        };

        validate_registered_detectors(&gc);
    }

    #[test]
    #[should_panic]
    fn test_validate_multiple_same_server_input_detectors() {
        let gc = GatewayConfig {
            orchestrator: OrchestratorConfig {
                host: "localhost".to_string(),
                port: Some(1234),
            },
            detectors: vec![DetectorConfig {
                name: "regex-1".to_string(),
                server: Some("server-a".to_string()),
                input: true,
                output: false,
                detector_params: None,
            }, DetectorConfig {
                name: "regex-2".to_string(),
                server: Some("server-a".to_string()),
                input: true,
                output: false,
                detector_params: None,
            },

            ],
            routes: vec![RouteConfig {
                name: "route1".to_string(),
                detectors: vec!["regex-1".to_string(), "regex-2".to_string()],
                fallback_message: None,
            }],
        };

        validate_registered_detectors(&gc);
    }

    #[test]
    #[should_panic]
    fn test_validate_multiple_same_server_output_detectors() {
        let gc = GatewayConfig {
            orchestrator: OrchestratorConfig {
                host: "localhost".to_string(),
                port: Some(1234),
            },
            detectors: vec![DetectorConfig {
                name: "regex-1".to_string(),
                server: Some("server-a".to_string()),
                input: false,
                output: true,
                detector_params: None,
            }, DetectorConfig {
                name: "regex-2".to_string(),
                server: Some("server-a".to_string()),
                input: false,
                output: true,
                detector_params: None,
            },

            ],
            routes: vec![RouteConfig {
                name: "route1".to_string(),
                detectors: vec!["regex-1".to_string(), "regex-2".to_string()],
                fallback_message: None,
            }],
        };

        validate_registered_detectors(&gc);
    }

    #[test]
    fn test_validate_multiple_same_server_detectors() {
        let gc = GatewayConfig {
            orchestrator: OrchestratorConfig {
                host: "localhost".to_string(),
                port: Some(1234),
            },
            detectors: vec![DetectorConfig {
                name: "regex-1".to_string(),
                server: Some("server-a".to_string()),
                input: true,
                output: false,
                detector_params: None,
            }, DetectorConfig {
                name: "regex-2".to_string(),
                server: Some("server-a".to_string()),
                input: false,
                output: true,
                detector_params: None,
            },

            ],
            routes: vec![RouteConfig {
                name: "route1".to_string(),
                detectors: vec!["regex-1".to_string(), "regex-2".to_string()],
                fallback_message: None,
            }],
        };

        validate_registered_detectors(&gc);
    }
}
