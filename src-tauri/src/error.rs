use serde::Serialize;

#[cfg_attr(feature = "gen-ipc", derive(specta::Type))]
#[derive(Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorKind {
    Permission,
    Network,
    Conflict,
    Invalid,
    Internal,
}

#[cfg_attr(feature = "gen-ipc", derive(specta::Type))]
#[derive(Debug, PartialEq, Eq, Serialize)]
pub struct AppError {
    pub kind: ErrorKind,
    pub message: String,
}

impl AppError {
    pub fn new(kind: ErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }
}

impl From<String> for AppError {
    fn from(message: String) -> Self {
        Self::new(ErrorKind::Internal, message)
    }
}

impl From<std::io::Error> for AppError {
    fn from(error: std::io::Error) -> Self {
        let kind = match error.kind() {
            std::io::ErrorKind::PermissionDenied => ErrorKind::Permission,
            std::io::ErrorKind::AlreadyExists => ErrorKind::Conflict,
            std::io::ErrorKind::InvalidInput | std::io::ErrorKind::InvalidData => {
                ErrorKind::Invalid
            }
            _ => ErrorKind::Internal,
        };
        Self::new(kind, error.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn serializes_command_errors() {
        for (kind, name) in [
            (ErrorKind::Permission, "permission"),
            (ErrorKind::Network, "network"),
            (ErrorKind::Conflict, "conflict"),
            (ErrorKind::Invalid, "invalid"),
            (ErrorKind::Internal, "internal"),
        ] {
            assert_eq!(
                serde_json::to_value(AppError::new(kind, "详情")).unwrap(),
                json!({"kind": name, "message": "详情"})
            );
        }
    }
}
