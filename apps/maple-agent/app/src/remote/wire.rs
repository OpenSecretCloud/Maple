//! The remote surface is independently versioned and deliberately smaller than
//! local IPC. Adding a local backend operation never makes it remotely callable.
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

pub const ALPN: &[u8] = b"maple-agent/remote/1";
pub const VERSION: u32 = 1;
pub const MAX_FRAME: usize = 16 * 1024 * 1024;
pub const MAX_PREFACE: usize = 4096;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Hello {
    pub version: u32,
    pub minimum_version: u32,
    pub account: String,
    pub project: String,
    pub deployment: String,
    pub channel: String,
    pub mode: Mode,
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Mode {
    Control,
    ConfirmGrant { window: [u8; 16] },
    Pair { window: [u8; 16], admission: String },
}

impl Hello {
    pub fn check_version(&self) -> Result<(), Error> {
        if self.minimum_version == 0
            || self.minimum_version > self.version
            || self.minimum_version > VERSION
            || self.version < VERSION
        {
            return Err(Error::Incompatible);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Method {
    TaskBootstrap,
    RecentProjectRoots,
    SelectProjectRoot,
    ListSessions,
    LocalBootstrap,
    RecoverySnapshot,
    CreateSession,
    LoadSession,
    SetSessionState,
    DeleteSession,
    RemoveProjectRoot,
    RenameSession,
    ProjectTrust,
    SetProjectTrust,
    CancelQueuedMessage,
    BeginQueuedMessageEdit,
    EndQueuedMessageEdit,
    SendMessage,
    SendOutcome,
    ReadImageAttachment,
    CancelRun,
    AvailableModelIds,
    ModelCatalogEntry,
    ListSessionMcpServers,
    SetSessionMcpServerEnabled,
    ListMcpServers,
    ListProjectMcpServers,
    SetProjectMcpServerEnabled,
    ListIntegrations,
    SetIntegrationEnabled,
    SaveMcpServers,
    SetSessionWebEnabled,
    CompactSession,
    SessionSubagents,
    CancelExternalAgent,
    ListSlashCommands,
    ResolveSlashCommand,
    SummarizeToolCall,
    SummarizeThinking,
    AskSideQuestion,
    ContextUsage,
    AnswerQuestion,
    ThinkingState,
    SetThinkingLevel,
    SaveDefaultModel,
    SavedModel,
    LoadToolSummaries,
    StoreToolSummary,
}

impl Method {
    pub fn parse(value: &str) -> Result<Self, String> {
        serde_json::from_value(serde_json::Value::String(value.into()))
            .map_err(|_| "This operation is available only on the local computer.".into())
    }
    pub fn name(self) -> String {
        // This enum is serialized only as a unit string.
        serde_json::to_value(self)
            .expect("unit enum serializes")
            .as_str()
            .expect("unit enum is string")
            .into()
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Command {
    pub id: u64,
    pub method: Method,
    pub send_id: Option<String>,
    pub params: serde_json::Value,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Error {
    Disconnected,
    InvalidFrame,
    Incompatible,
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Disconnected => "Remote computer disconnected",
            Self::InvalidFrame => "The remote computer sent an invalid message. Update both computers before reconnecting.",
            Self::Incompatible => "These Maple remote protocol versions are incompatible. Update both computers.",
        })
    }
}
impl std::error::Error for Error {}

pub async fn read<T: DeserializeOwned>(
    reader: &mut (impl AsyncRead + Unpin),
    limit: usize,
) -> Result<T, Error> {
    let size = reader.read_u32().await.map_err(|_| Error::Disconnected)? as usize;
    if size == 0 || size > limit.min(MAX_FRAME) {
        return Err(Error::InvalidFrame);
    }
    let mut bytes = vec![0; size];
    reader
        .read_exact(&mut bytes)
        .await
        .map_err(|_| Error::Disconnected)?;
    serde_json::from_slice(&bytes).map_err(|_| Error::InvalidFrame)
}

pub async fn write<T: Serialize>(
    writer: &mut (impl AsyncWrite + Unpin),
    value: &T,
    limit: usize,
) -> Result<(), Error> {
    let bytes = serde_json::to_vec(value).map_err(|_| Error::InvalidFrame)?;
    if bytes.len() > limit.min(MAX_FRAME) {
        return Err(Error::InvalidFrame);
    }
    writer
        .write_u32(bytes.len() as u32)
        .await
        .map_err(|_| Error::Disconnected)?;
    writer
        .write_all(&bytes)
        .await
        .map_err(|_| Error::Disconnected)?;
    writer.flush().await.map_err(|_| Error::Disconnected)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn privileged_local_operations_are_not_remote_commands() {
        for name in [
            "login",
            "logout_and_clear",
            "change_password",
            "create_api_key",
            "delete_api_key",
            "account",
            "plan_usage",
            "billing_status",
            "start_checkout",
            "confirm_account_deletion",
            "setup_integration",
            "stop_runtime",
            "start_runtime",
            "detach",
            "stop",
            "remote_enable",
            "remote_pair",
            "transcribe_audio",
            "synthesize_speech",
        ] {
            assert!(Method::parse(name).is_err(), "{name}");
        }
        for name in [
            "send_message",
            "send_outcome",
            "cancel_run",
            "answer_question",
            "set_thinking_level",
        ] {
            assert_eq!(Method::parse(name).unwrap().name(), name);
        }
    }

    #[tokio::test]
    async fn oversized_and_unknown_commands_fail_terminally() {
        let (mut tx, mut rx) = tokio::io::duplex(256);
        tx.write_u32(MAX_PREFACE as u32 + 1).await.unwrap();
        assert!(matches!(
            read::<Hello>(&mut rx, MAX_PREFACE).await,
            Err(Error::InvalidFrame)
        ));
        let command =
            serde_json::json!({"id": 1, "method": "login", "params": [], "send_id": null});
        write(&mut tx, &command, MAX_PREFACE).await.unwrap();
        assert!(matches!(
            read::<Command>(&mut rx, MAX_PREFACE).await,
            Err(Error::InvalidFrame)
        ));
    }

    #[test]
    fn compatible_versions_do_not_depend_on_build_identity() {
        let mut hello = Hello {
            version: 1,
            minimum_version: 1,
            account: "a".into(),
            project: "p".into(),
            deployment: "d".into(),
            channel: "unpackaged".into(),
            mode: Mode::Control,
        };
        assert!(hello.check_version().is_ok());
        hello.version = 2;
        assert!(hello.check_version().is_ok());
        hello.minimum_version = 2;
        assert_eq!(hello.check_version(), Err(Error::Incompatible));
        hello.version = 1;
        assert_eq!(hello.check_version(), Err(Error::Incompatible));
    }
}
