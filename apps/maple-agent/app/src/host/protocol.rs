//! Private, versioned local host messages. Payloads are never logged.

use serde::{Deserialize, Serialize, de::DeserializeOwned};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

pub const VERSION: u32 = 1;
pub const BUILD: &str = concat!(env!("CARGO_PKG_VERSION"), ":", env!("MAPLE_GIT_SOURCE_SHA"));
// Ten 10 MiB image attachments fit after base64/JSON encoding. Reads check
// length before allocating. A connection admits only a bounded number of calls.
pub const MAX_FRAME: usize = 160 * 1024 * 1024;

#[derive(Serialize, Deserialize)]
pub struct Hello {
    pub version: u32,
    pub profile: String,
    pub build: String,
    pub mode: ConnectionMode,
    #[serde(default)]
    pub remote_host: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum ConnectionMode {
    Client,
    Acp,
    Stop,
}

#[derive(Serialize, Deserialize)]
pub struct Welcome {
    pub version: u32,
    pub pid: u32,
    pub build: String,
    pub user_id: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Request {
    pub id: u64,
    pub method: String,
    #[serde(default)]
    pub send_id: Option<String>,
    pub params: serde_json::Value,
}

#[derive(Serialize, Deserialize)]
pub enum Message {
    Closed {
        reason: String,
    },
    Response {
        id: u64,
        result: Result<serde_json::Value, String>,
    },
    Event(Box<maple_agent::agent::AgentServiceEvent>),
}

pub async fn read<T: DeserializeOwned>(input: &mut (impl AsyncRead + Unpin)) -> Result<T, String> {
    let length = input
        .read_u32()
        .await
        .map_err(|_| "Host connection closed".to_string())? as usize;
    if length == 0 || length > MAX_FRAME {
        return Err("Host message exceeds the size limit".into());
    }
    let mut bytes = vec![0; length];
    input
        .read_exact(&mut bytes)
        .await
        .map_err(|_| "Host connection closed".to_string())?;
    serde_json::from_slice(&bytes).map_err(|_| "Invalid host message".into())
}

pub async fn write<T: Serialize>(
    output: &mut (impl AsyncWrite + Unpin),
    value: &T,
) -> Result<(), String> {
    let bytes =
        serde_json::to_vec(value).map_err(|_| "Could not encode host message".to_string())?;
    if bytes.len() > MAX_FRAME {
        return Err("Host message exceeds the size limit".into());
    }
    output
        .write_u32(bytes.len() as u32)
        .await
        .map_err(|_| "Host connection closed".to_string())?;
    output
        .write_all(&bytes)
        .await
        .map_err(|_| "Host connection closed".to_string())?;
    output
        .flush()
        .await
        .map_err(|_| "Host connection closed".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn fragmented_frames_preserve_request_boundaries() {
        let (mut sender, mut receiver) = tokio::io::duplex(7);
        let writer = tokio::spawn(async move {
            for id in 0..20 {
                write(
                    &mut sender,
                    &Request {
                        id,
                        send_id: None,
                        method: "test".into(),
                        params: serde_json::json!(["a payload larger than the pipe"]),
                    },
                )
                .await
                .unwrap();
            }
        });
        for id in 0..20 {
            assert_eq!(read::<Request>(&mut receiver).await.unwrap().id, id);
        }
        writer.await.unwrap();
    }

    #[tokio::test]
    async fn oversized_header_is_rejected_before_reading_or_allocating_a_body() {
        let (mut sender, mut receiver) = tokio::io::duplex(8);
        sender.write_u32(MAX_FRAME as u32 + 1).await.unwrap();
        assert!(
            read::<Request>(&mut receiver)
                .await
                .unwrap_err()
                .contains("size limit")
        );
    }
}
