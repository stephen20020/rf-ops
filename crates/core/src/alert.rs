//! Alert lifecycle: open -> acknowledged -> resolved.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AlertState {
    Open,
    Acknowledged,
    Resolved,
}

impl AlertState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Open => "open",
            Self::Acknowledged => "acknowledged",
            Self::Resolved => "resolved",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "open" => Some(Self::Open),
            "acknowledged" => Some(Self::Acknowledged),
            "resolved" => Some(Self::Resolved),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    Info,
    Warning,
    Critical,
}

impl Severity {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Info => "info",
            Self::Warning => "warning",
            Self::Critical => "critical",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "info" => Some(Self::Info),
            "warning" => Some(Self::Warning),
            "critical" => Some(Self::Critical),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AlertAction {
    Acknowledge,
    Resolve,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum AlertError {
    #[error("alert is already acknowledged")]
    AlreadyAcknowledged,
    #[error("alert is already resolved")]
    AlreadyResolved,
}

/// Apply an action. Resolving an open alert implies acknowledgment.
/// Returns the new state and whether an implicit acknowledgment happened.
pub fn transition(
    state: AlertState,
    action: AlertAction,
) -> Result<(AlertState, bool), AlertError> {
    use AlertAction::*;
    use AlertState::*;
    match (state, action) {
        (Resolved, _) => Err(AlertError::AlreadyResolved),
        (Acknowledged, Acknowledge) => Err(AlertError::AlreadyAcknowledged),
        (Open, Acknowledge) => Ok((Acknowledged, false)),
        (Open, Resolve) => Ok((Resolved, true)),
        (Acknowledged, Resolve) => Ok((Resolved, false)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lifecycle() {
        assert_eq!(
            transition(AlertState::Open, AlertAction::Acknowledge),
            Ok((AlertState::Acknowledged, false))
        );
        assert_eq!(
            transition(AlertState::Acknowledged, AlertAction::Resolve),
            Ok((AlertState::Resolved, false))
        );
        assert_eq!(
            transition(AlertState::Open, AlertAction::Resolve),
            Ok((AlertState::Resolved, true))
        );
    }

    #[test]
    fn invalid_transitions() {
        assert_eq!(
            transition(AlertState::Acknowledged, AlertAction::Acknowledge),
            Err(AlertError::AlreadyAcknowledged)
        );
        assert_eq!(
            transition(AlertState::Resolved, AlertAction::Acknowledge),
            Err(AlertError::AlreadyResolved)
        );
        assert_eq!(
            transition(AlertState::Resolved, AlertAction::Resolve),
            Err(AlertError::AlreadyResolved)
        );
    }

    #[test]
    fn severity_orders() {
        assert!(Severity::Critical > Severity::Warning && Severity::Warning > Severity::Info);
    }
}
