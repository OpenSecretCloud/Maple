//! One bounded request/reply per QUIC stream. The initial authenticated stream
//! carries ordered events only. No transfer owns the connection's dispatch loop.
use super::wire::{self, Command, Method};
use crate::host::protocol::Message;
use iroh::endpoint::{Connection, QuicTransportConfig, RecvStream, SendStream};
use serde::{Deserialize, Serialize};
use std::{sync::Arc, time::Duration};
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

pub const MAX_REQUESTS: usize = 40;
pub const IO_TIMEOUT: Duration = Duration::from_secs(10);
// Attachments may take longer than control traffic on a slow link, but cannot
// retain an admitted stream indefinitely.
const TRANSFER_TIMEOUT: Duration = Duration::from_secs(60);
const MAX_CONTROL: usize = 64 * 1024;

pub fn transport_config() -> QuicTransportConfig {
    QuicTransportConfig::builder()
        // One event/handshake stream plus bounded request streams. The aggregate
        // window exceeds all stream windows, so unread bulk streams cannot use
        // up the flow-control credit needed by another stream.
        .max_concurrent_bidi_streams(41u32.into())
        .max_concurrent_uni_streams(0u32.into())
        .stream_receive_window((512u32 * 1024).into())
        .receive_window((32u32 * 1024 * 1024).into())
        .build()
}

pub struct Capacity {
    ordinary: Arc<Semaphore>,
    urgent: Arc<Semaphore>,
}

impl Capacity {
    pub fn new() -> Self {
        Self {
            ordinary: Arc::new(Semaphore::new(32)),
            urgent: Arc::new(Semaphore::new(2)),
        }
    }
    pub fn acquire(&self, method: Method) -> Result<OwnedSemaphorePermit, String> {
        let pool = if method.urgent() {
            &self.urgent
        } else {
            &self.ordinary
        };
        pool.clone()
            .try_acquire_owned()
            .map_err(|_| "Remote computer is busy; try again".into())
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Header {
    pub id: u64,
    pub method: Method,
    pub send_id: Option<String>,
}

impl Header {
    pub async fn read(rx: &mut RecvStream) -> Result<Self, String> {
        tokio::time::timeout(IO_TIMEOUT, wire::read(rx, wire::MAX_PREFACE))
            .await
            .map_err(|_| "Remote request header timed out")?
            .map_err(|e| e.to_string())
    }
    pub async fn body(self, rx: &mut RecvStream) -> Result<Command, String> {
        let params = tokio::time::timeout(
            transfer_timeout(self.method),
            wire::read(rx, body_limit(self.method)),
        )
        .await
        .map_err(|_| "Remote request transfer timed out")?
        .map_err(|e| e.to_string())?;
        Ok(Command {
            id: self.id,
            method: self.method,
            send_id: self.send_id,
            params,
        })
    }
}

fn body_limit(method: Method) -> usize {
    if method.urgent() {
        MAX_CONTROL
    } else {
        wire::MAX_FRAME
    }
}

fn transfer_timeout(method: Method) -> Duration {
    if method.urgent() {
        IO_TIMEOUT
    } else {
        TRANSFER_TIMEOUT
    }
}

pub async fn reply(tx: &mut SendStream, message: &Message) -> Result<(), String> {
    tokio::time::timeout(TRANSFER_TIMEOUT, async {
        wire::write(tx, message, wire::MAX_FRAME)
            .await
            .map_err(|e| e.to_string())?;
        tx.finish().map_err(|_| "Remote reply closed")?;
        tx.stopped()
            .await
            .map_err(|_| "Remote reply disconnected")?;
        Ok(())
    })
    .await
    .map_err(|_| "Remote reply transfer timed out")?
}

pub async fn call(connection: &Connection, command: Command) -> Result<Message, String> {
    let id = command.id;
    // Reject size before transmitting anything. A later transport failure is
    // uncertain and must disconnect/query the receipt, never report not-sent.
    let Ok(body) = wire::encode(&command.params, body_limit(command.method)) else {
        return Ok(Message::Response {
            id,
            result: Err("Remote message is too large. Try fewer or smaller images.".into()),
        });
    };
    let header = Header {
        id,
        method: command.method,
        send_id: command.send_id,
    };
    let (mut tx, mut rx) = tokio::time::timeout(IO_TIMEOUT, connection.open_bi())
        .await
        .map_err(|_| "Remote stream capacity timed out")?
        .map_err(|_| "Remote connection closed")?;
    tx.set_priority(if header.method.urgent() { 10 } else { 0 })
        .map_err(|_| "Remote stream closed")?;
    tokio::time::timeout(transfer_timeout(header.method), async {
        wire::write(&mut tx, &header, wire::MAX_PREFACE).await?;
        wire::write_encoded(&mut tx, &body).await
    })
    .await
    .map_err(|_| "Remote request transfer timed out")?
    .map_err(|e| e.to_string())?;
    tx.finish().map_err(|_| "Remote request closed")?;
    match wire::read::<Message>(&mut rx, wire::MAX_FRAME).await {
        Ok(message @ Message::Response { id: received, .. }) if received == id => Ok(message),
        Ok(message @ Message::Closed { .. }) => Ok(message),
        Err(wire::Error::Disconnected) => Err("Remote reply disconnected".into()),
        _ => Ok(Message::Closed {
            reason: "Invalid remote reply; reconnect with compatible Maple versions".into(),
        }),
    }
}

#[cfg(test)]
mod tests;
