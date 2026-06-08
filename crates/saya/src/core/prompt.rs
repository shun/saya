use std::fmt;

use vim_core_rs::CoreSessionError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PromptResponseCommand {
    Submit { correlation_id: u64, value: String },
    Cancel { correlation_id: u64 },
}

impl PromptResponseCommand {
    pub fn correlation_id(&self) -> u64 {
        match self {
            Self::Submit { correlation_id, .. } | Self::Cancel { correlation_id } => {
                *correlation_id
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PromptResponseError {
    NoActivePrompt,
    CorrelationMismatch { expected: u64, actual: u64 },
    CoreRejected(PromptResponseRejection),
    Core(CoreSessionError),
}

impl fmt::Display for PromptResponseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoActivePrompt => write!(f, "no active core input prompt"),
            Self::CorrelationMismatch { expected, actual } => write!(
                f,
                "prompt response correlation mismatch: expected={expected}, actual={actual}"
            ),
            Self::CoreRejected(rejection) => {
                write!(f, "core rejected prompt response: {rejection}")
            }
            Self::Core(error) => write!(f, "core prompt response failed: {error:?}"),
        }
    }
}

impl std::error::Error for PromptResponseError {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PromptResponseRejection {
    NoPendingInput,
    CoreCorrelationMismatch { expected: u64, actual: u64 },
    CommandRejected(String),
}

impl fmt::Display for PromptResponseRejection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoPendingInput => write!(f, "no pending core input request"),
            Self::CoreCorrelationMismatch { expected, actual } => write!(
                f,
                "core prompt correlation mismatch: expected={expected}, actual={actual}"
            ),
            Self::CommandRejected(reason) => write!(f, "core command rejected: {reason}"),
        }
    }
}
