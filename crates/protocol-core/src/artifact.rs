//! Artifact contracts and evidence verification.
//!
//! An artifact contract defines what "done" looks like for a given action.
//! Evidence is the proof that the action actually produced the expected result.
//! This is the core innovation: agents don't just report success — they prove it.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Evidence that an action produced a result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Evidence {
    /// What kind of evidence this is (e.g., "screenshot", "dom_snapshot", "text_content", "http_status").
    pub kind: String,

    /// The evidence payload (structured data, base64 image, text, etc.).
    pub data: serde_json::Value,

    /// When this evidence was captured.
    pub captured_at: DateTime<Utc>,
}

impl Evidence {
    pub fn new(kind: impl Into<String>, data: serde_json::Value) -> Self {
        Self {
            kind: kind.into(),
            data,
            captured_at: Utc::now(),
        }
    }
}

/// An artifact contract that defines what success looks like.
///
/// Implementors check whether the collected evidence satisfies
/// the completion criteria for a given action.
pub trait ArtifactContract: Send + Sync {
    /// Check whether the evidence satisfies this contract.
    fn verify(&self, evidence: &[Evidence]) -> VerificationResult;

    /// Human-readable description of what this contract checks.
    fn description(&self) -> &str;
}

/// Result of artifact verification.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum VerificationResult {
    /// Evidence satisfies the contract. Includes extracted artifacts.
    Passed { artifacts: Vec<String> },

    /// Evidence does not satisfy the contract.
    Failed { reason: String },

    /// Not enough evidence to make a determination.
    Insufficient { missing: Vec<String> },
}

/// A simple contract that checks whether specific evidence kinds are present.
pub struct RequiredEvidenceContract {
    pub required_kinds: Vec<String>,
    pub description: String,
}

impl ArtifactContract for RequiredEvidenceContract {
    fn verify(&self, evidence: &[Evidence]) -> VerificationResult {
        let present_kinds: Vec<&str> = evidence.iter().map(|e| e.kind.as_str()).collect();
        let missing: Vec<String> = self
            .required_kinds
            .iter()
            .filter(|k| !present_kinds.contains(&k.as_str()))
            .cloned()
            .collect();

        if !missing.is_empty() {
            return VerificationResult::Insufficient { missing };
        }

        if let Some(reason) = validate_required_evidence(&self.required_kinds, evidence) {
            return VerificationResult::Failed { reason };
        }

        VerificationResult::Passed {
            artifacts: self.required_kinds.clone(),
        }
    }

    fn description(&self) -> &str {
        &self.description
    }
}

fn validate_required_evidence(required_kinds: &[String], evidence: &[Evidence]) -> Option<String> {
    for required_kind in required_kinds {
        let mut first_error: Option<String> = None;
        let mut has_valid_evidence = false;

        for item in evidence.iter().filter(|item| &item.kind == required_kind) {
            match validate_evidence_item(required_kind, item) {
                Ok(()) => {
                    has_valid_evidence = true;
                    break;
                }
                Err(err) => {
                    if first_error.is_none() {
                        first_error = Some(err);
                    }
                }
            }
        }

        if !has_valid_evidence {
            return Some(format!(
                "Invalid evidence for '{}': {}",
                required_kind,
                first_error.unwrap_or_else(|| "no valid evidence entries".to_string())
            ));
        }
    }

    None
}

fn validate_evidence_item(kind: &str, evidence: &Evidence) -> Result<(), String> {
    match kind {
        "screenshot" => validate_screenshot_evidence(evidence),
        "text_content" => validate_text_content_evidence(evidence),
        _ => Ok(()),
    }
}

fn validate_screenshot_evidence(evidence: &Evidence) -> Result<(), String> {
    let obj = evidence
        .data
        .as_object()
        .ok_or_else(|| "screenshot evidence must be a JSON object".to_string())?;

    let has_non_empty_base64 = obj
        .get("base64")
        .and_then(serde_json::Value::as_str)
        .map(|value| !value.trim().is_empty())
        .unwrap_or(false);

    let has_positive_base64_length = obj
        .get("base64_length")
        .and_then(serde_json::Value::as_u64)
        .map(|value| value > 0)
        .unwrap_or(false);

    let has_non_empty_path = obj
        .get("path")
        .and_then(serde_json::Value::as_str)
        .map(|value| !value.trim().is_empty())
        .unwrap_or(false);

    if let Some(format) = obj.get("format").and_then(serde_json::Value::as_str) {
        if format.trim().is_empty() {
            return Err("screenshot evidence field 'format' cannot be empty".to_string());
        }
    }

    if has_non_empty_base64 || has_positive_base64_length || has_non_empty_path {
        Ok(())
    } else {
        Err(
            "screenshot evidence must include non-empty 'base64', positive 'base64_length', or non-empty 'path'"
                .to_string(),
        )
    }
}

fn validate_text_content_evidence(evidence: &Evidence) -> Result<(), String> {
    let obj = evidence
        .data
        .as_object()
        .ok_or_else(|| "text_content evidence must be a JSON object".to_string())?;

    let has_non_empty_text = obj
        .get("text")
        .and_then(serde_json::Value::as_str)
        .map(|value| !value.trim().is_empty())
        .unwrap_or(false);

    let has_positive_length = obj
        .get("length")
        .and_then(serde_json::Value::as_u64)
        .map(|value| value > 0)
        .unwrap_or(false);

    if has_non_empty_text || has_positive_length {
        Ok(())
    } else {
        Err("text_content evidence must include non-empty 'text' or positive 'length'".to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_evidence_creation() {
        let evidence = Evidence::new("screenshot", json!({"base64": "..."}));
        assert_eq!(evidence.kind, "screenshot");
    }

    #[test]
    fn test_required_evidence_passes() {
        let contract = RequiredEvidenceContract {
            required_kinds: vec!["page_loaded".to_string(), "text_content".to_string()],
            description: "Page must load with visible content".to_string(),
        };
        let evidence = vec![
            Evidence::new("page_loaded", json!({"url": "https://example.com"})),
            Evidence::new("text_content", json!({"text": "Welcome"})),
        ];
        match contract.verify(&evidence) {
            VerificationResult::Passed { artifacts } => {
                assert_eq!(artifacts.len(), 2);
            }
            other => panic!("Expected Passed, got {:?}", other),
        }
    }

    #[test]
    fn test_required_evidence_insufficient() {
        let contract = RequiredEvidenceContract {
            required_kinds: vec!["page_loaded".to_string(), "confirmation_number".to_string()],
            description: "Must have confirmation".to_string(),
        };
        let evidence = vec![Evidence::new(
            "page_loaded",
            json!({"url": "https://example.com"}),
        )];
        match contract.verify(&evidence) {
            VerificationResult::Insufficient { missing } => {
                assert_eq!(missing, vec!["confirmation_number"]);
            }
            other => panic!("Expected Insufficient, got {:?}", other),
        }
    }

    #[test]
    fn test_required_evidence_fails_for_invalid_screenshot_metadata() {
        let contract = RequiredEvidenceContract {
            required_kinds: vec!["screenshot".to_string()],
            description: "Screenshot evidence required".to_string(),
        };
        let evidence = vec![Evidence::new("screenshot", json!({"format": "png"}))];

        match contract.verify(&evidence) {
            VerificationResult::Failed { reason } => {
                assert!(reason.contains("Invalid evidence for 'screenshot'"));
                assert!(reason.contains("base64"));
            }
            other => panic!("Expected Failed, got {:?}", other),
        }
    }

    #[test]
    fn test_required_evidence_accepts_screenshot_with_base64_length() {
        let contract = RequiredEvidenceContract {
            required_kinds: vec!["screenshot".to_string()],
            description: "Screenshot evidence required".to_string(),
        };
        let evidence = vec![Evidence::new(
            "screenshot",
            json!({"format": "png", "base64_length": 128}),
        )];

        match contract.verify(&evidence) {
            VerificationResult::Passed { artifacts } => {
                assert_eq!(artifacts, vec!["screenshot"]);
            }
            other => panic!("Expected Passed, got {:?}", other),
        }
    }

    #[test]
    fn test_required_evidence_fails_for_empty_text_content() {
        let contract = RequiredEvidenceContract {
            required_kinds: vec!["text_content".to_string()],
            description: "Text extraction evidence required".to_string(),
        };
        let evidence = vec![Evidence::new("text_content", json!({"length": 0}))];

        match contract.verify(&evidence) {
            VerificationResult::Failed { reason } => {
                assert!(reason.contains("Invalid evidence for 'text_content'"));
                assert!(reason.contains("non-empty 'text'"));
            }
            other => panic!("Expected Failed, got {:?}", other),
        }
    }
}
