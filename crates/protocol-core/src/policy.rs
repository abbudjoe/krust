//! Policy engine for action gating.
//!
//! The policy engine decides whether an action should be allowed,
//! denied, or require human confirmation before execution.

use crate::intent::Intent;
use serde::{Deserialize, Serialize};

/// The decision made by a policy evaluation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum PolicyDecision {
    /// Action is allowed to proceed without confirmation.
    Allow,

    /// Action requires human confirmation before proceeding.
    Confirm { reason: String },

    /// Action is denied outright.
    Deny { reason: String },
}

/// Trait for implementing policy evaluation.
///
/// Implementors decide whether a given intent should be allowed,
/// require confirmation, or be denied. Multiple policies can be
/// composed — the most restrictive decision wins.
pub trait Policy: Send + Sync {
    /// Evaluate a policy decision for the given intent.
    fn evaluate(&self, intent: &Intent) -> PolicyDecision;

    /// Human-readable name for this policy.
    fn name(&self) -> &str;
}

/// A policy that always allows all actions (for testing/development).
pub struct AllowAllPolicy;

impl Policy for AllowAllPolicy {
    fn evaluate(&self, _intent: &Intent) -> PolicyDecision {
        PolicyDecision::Allow
    }

    fn name(&self) -> &str {
        "allow-all"
    }
}

/// A policy that requires confirmation for any intent matching a set of patterns.
pub struct ConfirmPatternPolicy {
    /// Intent kind prefixes that require confirmation (e.g., "payment.", "email.send").
    pub confirm_prefixes: Vec<String>,
    /// Intent kind prefixes that are denied outright.
    pub deny_prefixes: Vec<String>,
}

impl Policy for ConfirmPatternPolicy {
    fn evaluate(&self, intent: &Intent) -> PolicyDecision {
        for prefix in &self.deny_prefixes {
            if intent.kind.starts_with(prefix) {
                return PolicyDecision::Deny {
                    reason: format!("Intent '{}' matches deny pattern '{}'", intent.kind, prefix),
                };
            }
        }

        for prefix in &self.confirm_prefixes {
            if intent.kind.starts_with(prefix) {
                return PolicyDecision::Confirm {
                    reason: format!("Intent '{}' requires human confirmation", intent.kind),
                };
            }
        }

        PolicyDecision::Allow
    }

    fn name(&self) -> &str {
        "confirm-pattern"
    }
}

/// Compose multiple policies. The most restrictive decision wins:
/// Deny > Confirm > Allow.
pub fn evaluate_policies(policies: &[&dyn Policy], intent: &Intent) -> PolicyDecision {
    let mut result = PolicyDecision::Allow;

    for policy in policies {
        match policy.evaluate(intent) {
            PolicyDecision::Deny { reason } => {
                return PolicyDecision::Deny { reason };
            }
            PolicyDecision::Confirm { reason } => {
                result = PolicyDecision::Confirm { reason };
            }
            PolicyDecision::Allow => {}
        }
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_allow_all() {
        let policy = AllowAllPolicy;
        let intent = Intent::new("web.navigate").with_param("url", json!("https://google.com"));
        assert_eq!(policy.evaluate(&intent), PolicyDecision::Allow);
    }

    #[test]
    fn test_confirm_pattern() {
        let policy = ConfirmPatternPolicy {
            confirm_prefixes: vec!["payment.".to_string()],
            deny_prefixes: vec![],
        };
        let intent = Intent::new("payment.submit");
        match policy.evaluate(&intent) {
            PolicyDecision::Confirm { .. } => {}
            other => panic!("Expected Confirm, got {:?}", other),
        }
    }

    #[test]
    fn test_deny_beats_confirm() {
        let confirm_policy = ConfirmPatternPolicy {
            confirm_prefixes: vec!["danger.".to_string()],
            deny_prefixes: vec![],
        };
        let deny_policy = ConfirmPatternPolicy {
            confirm_prefixes: vec![],
            deny_prefixes: vec!["danger.".to_string()],
        };
        let intent = Intent::new("danger.delete_everything");
        let result = evaluate_policies(&[&confirm_policy, &deny_policy], &intent);
        match result {
            PolicyDecision::Deny { .. } => {}
            other => panic!("Expected Deny, got {:?}", other),
        }
    }
}
