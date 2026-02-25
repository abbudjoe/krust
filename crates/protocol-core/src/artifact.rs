//! Artifact contracts and evidence verification.
//!
//! An artifact contract defines what "done" looks like for a given action.
//! Evidence is the proof that the action actually produced the expected result.
//! This is the core innovation: agents don't just report success — they prove it.

use serde::{Deserialize, Serialize};
use chrono::{DateTime, Utc};

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
    Passed {
        artifacts: Vec<String>,
    },

    /// Evidence does not satisfy the contract.
    Failed {
        reason: String,
    },

    /// Not enough evidence to make a determination.
    Insufficient {
        missing: Vec<String>,
    },
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

        if missing.is_empty() {
            VerificationResult::Passed {
                artifacts: self.required_kinds.clone(),
            }
        } else {
            VerificationResult::Insufficient { missing }
        }
    }

    fn description(&self) -> &str {
        &self.description
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
        let evidence = vec![
            Evidence::new("page_loaded", json!({"url": "https://example.com"})),
        ];
        match contract.verify(&evidence) {
            VerificationResult::Insufficient { missing } => {
                assert_eq!(missing, vec!["confirmation_number"]);
            }
            other => panic!("Expected Insufficient, got {:?}", other),
        }
    }
}
