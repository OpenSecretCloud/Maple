//! The remote surface is independently versioned and deliberately smaller than
//! local IPC. Adding a local backend operation never makes it remotely callable.
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

// Keep the ALPN family stable so older peers receive the explicit version refusal.
pub const ALPN: &[u8] = b"maple-agent/remote/1";
pub const VERSION: u32 = 3;
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

// Refusals are readable at every handshake stage, including before the peer's
// full Hello can be decoded. Only loss of authority/compatibility ends recovery.
#[derive(Debug, Serialize, Deserialize)]
pub struct Refusal {
    pub terminal: bool,
    pub message: String,
}
impl Refusal {
    pub fn temporary(message: impl Into<String>) -> Self {
        Self {
            terminal: false,
            message: message.into(),
        }
    }
    pub fn terminal(message: impl Into<String>) -> Self {
        Self {
            terminal: true,
            message: message.into(),
        }
    }
    pub fn into_error(self) -> String {
        if self.terminal {
            format!("Remote access ended: {}", self.message)
        } else {
            self.message
        }
    }
}
impl From<String> for Refusal {
    fn from(message: String) -> Self {
        Self::temporary(message)
    }
}
impl From<&str> for Refusal {
    fn from(message: &str) -> Self {
        Self::temporary(message)
    }
}
impl std::fmt::Display for Refusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

pub async fn read_hello(reader: &mut (impl AsyncRead + Unpin)) -> Result<Hello, Refusal> {
    let value: serde_json::Value = read(reader, MAX_PREFACE)
        .await
        .map_err(|error| Refusal::terminal(error.to_string()))?;
    #[derive(Deserialize)]
    struct Version {
        version: u32,
        minimum_version: u32,
    }
    let version: Version = serde_json::from_value(value.clone())
        .map_err(|_| Refusal::terminal(Error::InvalidFrame.to_string()))?;
    check_version(version.version, version.minimum_version)
        .map_err(|error| Refusal::terminal(error.to_string()))?;
    serde_json::from_value(value).map_err(|_| Refusal::terminal(Error::InvalidFrame.to_string()))
}

fn check_version(version: u32, minimum: u32) -> Result<(), Error> {
    if minimum == 0 || minimum > version || minimum > VERSION || version < VERSION {
        Err(Error::Incompatible)
    } else {
        Ok(())
    }
}

#[cfg(test)]
impl Hello {
    pub fn check_version(&self) -> Result<(), Error> {
        check_version(self.version, self.minimum_version)
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
    pub fn urgent(self) -> bool {
        matches!(
            self,
            Self::CancelRun | Self::AnswerQuestion | Self::CancelExternalAgent
        )
    }
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
    TimedOut,
    InvalidFrame,
    Incompatible,
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::TimedOut => "Remote transfer stopped making progress",
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
    let bytes = encode(value, limit)?;
    write_encoded(writer, &bytes).await
}

pub fn encode<T: Serialize>(value: &T, limit: usize) -> Result<Vec<u8>, Error> {
    let bytes = serde_json::to_vec(value).map_err(|_| Error::InvalidFrame)?;
    if bytes.len() > limit.min(MAX_FRAME) {
        return Err(Error::InvalidFrame);
    }
    Ok(bytes)
}

pub async fn write_encoded(
    writer: &mut (impl AsyncWrite + Unpin),
    bytes: &[u8],
) -> Result<(), Error> {
    writer
        .write_u32(bytes.len() as u32)
        .await
        .map_err(|_| Error::Disconnected)?;
    writer
        .write_all(bytes)
        .await
        .map_err(|_| Error::Disconnected)?;
    writer.flush().await.map_err(|_| Error::Disconnected)
}

/// Per-progress deadline, not a minimum network throughput requirement. The
/// first reply byte may wait for backend work; once a frame starts, bounded
/// buffers and an idle deadline prevent a stalled transfer retaining capacity.
pub async fn read_progress<T: DeserializeOwned>(
    reader: &mut (impl AsyncRead + Unpin),
    limit: usize,
    idle: std::time::Duration,
    wait_for_reply: bool,
) -> Result<T, Error> {
    let mut prefix = [0; 4];
    if wait_for_reply {
        reader
            .read_exact(&mut prefix[..1])
            .await
            .map_err(|_| Error::Disconnected)?;
        read_progress_bytes(reader, &mut prefix[1..], idle).await?;
    } else {
        read_progress_bytes(reader, &mut prefix, idle).await?;
    }
    let size = u32::from_be_bytes(prefix) as usize;
    if size == 0 || size > limit.min(MAX_FRAME) {
        return Err(Error::InvalidFrame);
    }
    let mut bytes = vec![0; size];
    read_progress_bytes(reader, &mut bytes, idle).await?;
    serde_json::from_slice(&bytes).map_err(|_| Error::InvalidFrame)
}
async fn read_progress_bytes(
    reader: &mut (impl AsyncRead + Unpin),
    mut bytes: &mut [u8],
    idle: std::time::Duration,
) -> Result<(), Error> {
    while !bytes.is_empty() {
        let count = tokio::time::timeout(idle, reader.read(bytes))
            .await
            .map_err(|_| Error::TimedOut)?
            .map_err(|_| Error::Disconnected)?;
        if count == 0 {
            return Err(Error::Disconnected);
        }
        bytes = &mut bytes[count..];
    }
    Ok(())
}
pub async fn write_progress(
    writer: &mut (impl AsyncWrite + Unpin),
    bytes: &[u8],
    idle: std::time::Duration,
) -> Result<(), Error> {
    let prefix = (bytes.len() as u32).to_be_bytes();
    for mut chunk in std::iter::once(prefix.as_slice()).chain(bytes.chunks(64 * 1024)) {
        while !chunk.is_empty() {
            let count = tokio::time::timeout(idle, writer.write(chunk))
                .await
                .map_err(|_| Error::TimedOut)?
                .map_err(|_| Error::Disconnected)?;
            if count == 0 {
                return Err(Error::Disconnected);
            }
            chunk = &chunk[count..];
        }
    }
    tokio::time::timeout(idle, writer.flush())
        .await
        .map_err(|_| Error::TimedOut)?
        .map_err(|_| Error::Disconnected)
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
            version: VERSION,
            minimum_version: VERSION,
            account: "a".into(),
            project: "p".into(),
            deployment: "d".into(),
            channel: "unpackaged".into(),
            mode: Mode::Control,
        };
        assert!(hello.check_version().is_ok());
        hello.version = VERSION - 1;
        hello.minimum_version = VERSION - 1;
        assert_eq!(hello.check_version(), Err(Error::Incompatible));
        hello.minimum_version = VERSION;
        hello.version = VERSION + 1;
        assert!(hello.check_version().is_ok());
        hello.minimum_version = VERSION + 1;
        assert_eq!(hello.check_version(), Err(Error::Incompatible));
        hello.version = VERSION;
        assert_eq!(hello.check_version(), Err(Error::Incompatible));
    }
    #[tokio::test]
    async fn future_and_malformed_hello_receive_explicit_terminal_refusals() {
        for message in [
            serde_json::json!({"version": VERSION + 1, "minimum_version": VERSION + 1, "future_field": true}),
            serde_json::json!({"version": VERSION, "minimum_version": VERSION, "future_field": true}),
        ] {
            let (mut tx, mut rx) = tokio::io::duplex(4096);
            write(&mut tx, &message, MAX_PREFACE).await.unwrap();
            let refusal = match read_hello(&mut rx).await {
                Err(r) => r,
                Ok(_) => panic!("accepted malformed hello"),
            };
            assert!(refusal.terminal);
            assert!(refusal.message.contains("Update both computers"));
            // All pairing/confirmation stages can decode the same refusal.
            let encoded = serde_json::to_value(Err::<(), _>(refusal)).unwrap();
            let proof: Result<(Vec<u8>, Vec<u8>), Refusal> =
                serde_json::from_value(encoded.clone()).unwrap();
            let grant: Result<[u8; 16], Refusal> = serde_json::from_value(encoded).unwrap();
            assert_eq!(proof.unwrap_err().message, grant.unwrap_err().message);
        }
    }

    #[tokio::test(start_paused = true)]
    async fn transfer_deadline_measures_inactivity_not_total_duration() {
        use std::time::Duration;
        let idle = Duration::from_secs(10);
        let (mut tx, mut rx) = tokio::io::duplex(16);
        let sender = tokio::spawn(async move {
            let bytes = b"\"slow\"";
            tx.write_u32(bytes.len() as u32).await.unwrap();
            for byte in bytes {
                tokio::time::sleep(Duration::from_secs(6)).await;
                tx.write_all(&[*byte]).await.unwrap();
            }
        });
        let start = tokio::time::Instant::now();
        assert_eq!(
            read_progress::<String>(&mut rx, 64, idle, false)
                .await
                .unwrap(),
            "slow"
        );
        assert!(start.elapsed() > idle);
        sender.await.unwrap();
        let (_tx, mut rx) = tokio::io::duplex(16);
        assert_eq!(
            read_progress::<String>(&mut rx, 64, idle, false).await,
            Err(Error::TimedOut)
        );
    }
    #[test]
    fn shipped_wire_examples_are_stable() {
        let examples: serde_json::Value =
            serde_json::from_str(include_str!("wire_v3.json")).unwrap();
        fn check<T: serde::de::DeserializeOwned + serde::Serialize>(value: &serde_json::Value) {
            let decoded: T = serde_json::from_value(value.clone()).unwrap();
            assert_eq!(serde_json::to_value(decoded).unwrap(), *value);
        }
        check::<Hello>(&examples["hello"]);
        check::<Hello>(&examples["pair"]);
        check::<super::super::streams::Header>(&examples["header"]);
        check::<Command>(&examples["command"]);
        check::<Result<(), Refusal>>(&examples["refusal"]);
        for key in ["image", "failure", "event"] {
            check::<crate::host::protocol::Message>(&examples[key]);
        }
        assert_eq!(examples["hello"]["version"], VERSION);
    }
}
