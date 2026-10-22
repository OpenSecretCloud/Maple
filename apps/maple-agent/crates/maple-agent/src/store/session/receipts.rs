//! Source identities observed by the injected Pi store, independently of commits.
//!
//! Recording only a digest avoids retaining another copy of a large message.
//! No callback is invoked while Pi holds its session lock. The host drains the
//! bounded queue from ordinary session callbacks after that lock is released.

use std::collections::VecDeque;
use std::io::{self, Write};
use std::sync::{Arc, Mutex, Weak};

use pi_coding_agent::SessionMessage;
use pi_coding_agent::session::{EntryKind, SessionEntry};
use sha2::{Digest, Sha256};

const SOURCE_WINDOW: usize = 64;

#[derive(Clone)]
pub(crate) struct SourceReceipt {
    pub(crate) entry_id: String,
    pub(crate) signature: [u8; 32],
}

impl SourceReceipt {
    pub(crate) fn of(entry: &SessionEntry) -> Option<Self> {
        let signature = match &entry.kind {
            EntryKind::Message { message } => message_signature(message),
            EntryKind::CustomMessage {
                custom_type,
                content,
                display,
                details,
            } => signature(&(custom_type, content, display, details)),
            _ => return None,
        };
        Some(Self {
            entry_id: entry.id.clone(),
            signature,
        })
    }
}

#[derive(Clone)]
pub(crate) struct SourceReceipts(Arc<Mutex<ReceiptQueue>>);

pub(crate) struct WeakSourceReceipts(Weak<Mutex<ReceiptQueue>>);

struct ReceiptQueue {
    /// Pi retries the retained chronological suffix. Advance on observation,
    /// even when SQL fails, so retries never announce that suffix twice.
    observed_through: Option<String>,
    pending: VecDeque<SourceReceipt>,
}

impl SourceReceipts {
    pub(super) fn new(observed_through: Option<String>) -> Self {
        Self(Arc::new(Mutex::new(ReceiptQueue {
            observed_through,
            pending: VecDeque::new(),
        })))
    }

    pub(super) fn observe(&self, entries: &[SessionEntry]) {
        let mut queue = self.0.lock().unwrap_or_else(|error| error.into_inner());
        let from = queue
            .observed_through
            .as_ref()
            .and_then(|id| entries.iter().rposition(|entry| &entry.id == id))
            .map_or(0, |index| index + 1);
        for entry in &entries[from..] {
            if let Some(receipt) = SourceReceipt::of(entry) {
                if queue.pending.len() == SOURCE_WINDOW {
                    queue.pending.pop_front();
                }
                queue.pending.push_back(receipt);
            }
        }
        if let Some(last) = entries.last() {
            queue.observed_through = Some(last.id.clone());
        }
    }

    pub(crate) fn drain(&self) -> Vec<SourceReceipt> {
        self.0
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .pending
            .drain(..)
            .collect()
    }

    pub(crate) fn downgrade(&self) -> WeakSourceReceipts {
        WeakSourceReceipts(Arc::downgrade(&self.0))
    }
}

impl WeakSourceReceipts {
    pub(crate) fn upgrade(&self) -> Option<SourceReceipts> {
        self.0.upgrade().map(SourceReceipts)
    }
}

pub(crate) fn message_signature(message: &SessionMessage) -> [u8; 32] {
    if let SessionMessage::Custom(custom) = message {
        signature(&(
            &custom.custom_type,
            &custom.content,
            &custom.display,
            &custom.details,
        ))
    } else {
        signature(message)
    }
}

fn signature(value: &impl serde::Serialize) -> [u8; 32] {
    struct Hasher(Sha256);
    impl Write for Hasher {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            self.0.update(bytes);
            Ok(bytes.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
    let mut hasher = Hasher(Sha256::new());
    // Serialize directly into the digest without cloning text/images/tool output.
    let _ = serde_json::to_writer(&mut hasher, value);
    hasher.0.finalize().into()
}
