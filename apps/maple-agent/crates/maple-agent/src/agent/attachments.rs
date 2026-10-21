use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64_STANDARD};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::store::{AccountId, ImageUpload, Store};

pub(super) const MAX_AGENT_IMAGE_BYTES: usize = 10 * 1024 * 1024;
pub(super) const MAX_AGENT_IMAGES_PER_MESSAGE: usize = 10;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AgentImageUpload {
    pub name: String,
    pub data_url: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AgentImageAttachment {
    pub id: String,
    pub name: String,
    pub mime_type: String,
    pub source: String,
}

#[derive(Debug, Clone)]
pub(super) struct PreparedAgentImage {
    pub attachment: AgentImageAttachment,
    pub base64_data: String,
}

/// The runtime's account-bound view of the host-owned SQL attachment store.
/// It never resolves an app data path or opens a previous attachment cache.
#[derive(Clone)]
pub(super) struct AgentAttachmentStore {
    store: Arc<Store>,
    account: AccountId,
}

impl AgentAttachmentStore {
    pub(super) fn new(store: Arc<Store>, account: AccountId) -> Self {
        Self { store, account }
    }

    pub(super) fn store_uploads(
        &self,
        session_id: &str,
        uploads: &[AgentImageUpload],
    ) -> Result<Vec<PreparedAgentImage>, String> {
        if uploads.len() > MAX_AGENT_IMAGES_PER_MESSAGE {
            return Err(format!(
                "Attach at most {MAX_AGENT_IMAGES_PER_MESSAGE} images at a time"
            ));
        }
        // Keep current validation messages and the model's original base64. The
        // privileged store independently validates the same batch before writes.
        let encoded = uploads
            .iter()
            .map(|upload| {
                let (declared_mime, base64_data) = parse_data_url(&upload.data_url)?;
                if base64_data.len() > maximum_base64_chars(MAX_AGENT_IMAGE_BYTES) {
                    return Err("Image too large (max 10MB)".to_string());
                }
                let bytes = BASE64_STANDARD
                    .decode(base64_data)
                    .map_err(|_| "Image attachment is not valid base64".to_string())?;
                if bytes.len() > MAX_AGENT_IMAGE_BYTES {
                    return Err("Image too large (max 10MB)".to_string());
                }
                let detected = supported_image_mime(&bytes)?;
                if normalize_mime(declared_mime) != detected {
                    return Err("Image content does not match its declared type".to_string());
                }
                Ok(base64_data.to_string())
            })
            .collect::<Result<Vec<_>, String>>()?;
        let uploads = uploads
            .iter()
            .map(|upload| ImageUpload {
                name: upload.name.clone(),
                data_url: upload.data_url.clone(),
            })
            .collect::<Vec<_>>();
        let stored = self
            .store
            .store_image_uploads(self.account, session_id, &uploads)
            .map_err(|error| error.to_string())?;
        Ok(stored
            .into_iter()
            .zip(encoded)
            .map(|(attachment, base64_data)| PreparedAgentImage {
                attachment: AgentImageAttachment {
                    id: attachment.id,
                    name: attachment.name,
                    mime_type: attachment.mime_type,
                    source: attachment.source,
                },
                base64_data,
            })
            .collect())
    }

    pub(super) fn read(&self, session_id: &str, attachment_id: &str) -> Result<Vec<u8>, String> {
        validate_attachment_id(attachment_id)?;
        let mut bytes = Vec::new();
        loop {
            // Every bounded range rechecks both the captured account and task.
            let range = self
                .store
                .read_attachment(
                    self.account,
                    session_id,
                    attachment_id,
                    bytes.len() as u64,
                    crate::store::MAX_ATTACHMENT_RANGE_BYTES,
                )
                .map_err(|error| error.to_string())?;
            if range.size > MAX_AGENT_IMAGE_BYTES as u64 {
                return Err("Agent image attachment exceeds the 10MB limit".to_string());
            }
            if bytes.len().saturating_add(range.data.len()) as u64 > range.size {
                return Err("Agent image attachment is incomplete".to_string());
            }
            bytes.extend_from_slice(&range.data);
            if bytes.len() as u64 == range.size {
                break;
            }
            if range.data.is_empty() {
                return Err("Agent image attachment is incomplete".to_string());
            }
        }
        supported_image_mime(&bytes)?;
        Ok(bytes)
    }
}

#[cfg(test)]
pub(super) fn fixture_attachment_store(
    root: &std::path::Path,
    tasks: &[&str],
) -> AgentAttachmentStore {
    let store = Arc::new(Store::open(crate::store::StorePaths::new(root)).unwrap());
    let account = store.upsert_account("attachment-fixture", 1).unwrap();
    for uuid in tasks {
        store
            .create_task(
                account,
                &crate::store::NewTask {
                    uuid: (*uuid).to_string(),
                    project_root: root.to_string_lossy().into_owned(),
                    kind: crate::store::TaskKind::Desktop,
                    title: "Image fixture".to_string(),
                    provisional: false,
                    model: None,
                    created_ms: 1,
                },
            )
            .unwrap();
    }
    AgentAttachmentStore::new(store, account)
}

pub(super) fn attachment_id_from_source(source: &str) -> Option<&str> {
    source
        .strip_prefix("maple-attachment://")
        .and_then(|id| (!id.is_empty() && !id.contains(['/', '\\', '?', '#'])).then_some(id))
}

const ATTACHED_IMAGE: &str = "The user attached the following image";
const READ_IMAGE_HINT: &str =
    "Use read_image with an attachment source when you need visual details from that image.";

/// The text a model gets for a message with images: what the user typed,
/// then each image's name and source, as Goose's runtime wrote it. A model
/// that gets the images beside the text (`inline`) is not told to look with
/// `read_image`; the source still lets it crop one.
pub(super) fn image_prompt(
    text: &str,
    attachments: &[AgentImageAttachment],
    inline: bool,
) -> String {
    let mut prompt = text.trim().to_string();
    if attachments.is_empty() {
        return prompt;
    }
    if !prompt.is_empty() {
        prompt.push_str("\n\n");
    }
    prompt.push_str(ATTACHED_IMAGE);
    prompt.push_str(if attachments.len() == 1 {
        ":\n"
    } else {
        "s:\n"
    });
    for attachment in attachments {
        let name = serde_json::to_string(&attachment.name).unwrap_or_else(|_| "\"image\"".into());
        prompt.push_str(&format!("- {name}: {}\n", attachment.source));
    }
    if !inline {
        prompt.push_str(READ_IMAGE_HINT);
    }
    prompt
}

/// What the user typed and which images they attached, read back from the
/// text [`image_prompt`] wrote. `None` for a message without images. An
/// attachment read back has no media type.
pub(super) fn split_image_prompt(prompt: &str) -> Option<(String, Vec<AgentImageAttachment>)> {
    let start = prompt.rfind(ATTACHED_IMAGE)?;
    let (typed, listing) = prompt.split_at(start);
    let typed = match typed.strip_suffix("\n\n") {
        Some(typed) => typed,
        None if typed.is_empty() => "",
        None => return None,
    };
    let mut lines = listing.lines();
    let head = lines.next()?.strip_prefix(ATTACHED_IMAGE)?;
    if head != ":" && head != "s:" {
        return None;
    }
    let mut attachments = Vec::new();
    for line in lines {
        if line == READ_IMAGE_HINT {
            break;
        }
        let item = line.strip_prefix("- ")?;
        let mut names = serde_json::Deserializer::from_str(item).into_iter::<String>();
        let name = names.next()?.ok()?;
        let source = item[names.byte_offset()..].strip_prefix(": ")?;
        let id = attachment_id_from_source(source)?;
        attachments.push(AgentImageAttachment {
            id: id.to_string(),
            name,
            mime_type: String::new(),
            source: source.to_string(),
        });
    }
    (!attachments.is_empty()).then(|| (typed.to_string(), attachments))
}

fn parse_data_url(data_url: &str) -> Result<(&str, &str), String> {
    let (header, data) = data_url
        .split_once(',')
        .ok_or_else(|| "Image attachment must be a base64 data URL".to_string())?;
    let mime = header
        .strip_prefix("data:")
        .and_then(|value| value.strip_suffix(";base64"))
        .ok_or_else(|| "Image attachment must be a base64 data URL".to_string())?;
    if data.is_empty() {
        return Err("Image attachment cannot be empty".to_string());
    }
    Ok((mime, data))
}

fn maximum_base64_chars(bytes: usize) -> usize {
    bytes.div_ceil(3).saturating_mul(4)
}

fn normalize_mime(mime: &str) -> &str {
    if mime.eq_ignore_ascii_case("image/jpg") {
        "image/jpeg"
    } else {
        mime
    }
}

fn supported_image_mime(bytes: &[u8]) -> Result<&'static str, String> {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        Ok("image/png")
    } else if bytes.starts_with(&[0xff, 0xd8, 0xff]) {
        Ok("image/jpeg")
    } else if bytes.len() >= 12 && bytes.starts_with(b"RIFF") && &bytes[8..12] == b"WEBP" {
        Ok("image/webp")
    } else {
        Err("Only JPEG, PNG, and WebP images are supported".to_string())
    }
}

fn validate_attachment_id(id: &str) -> Result<(), String> {
    if id.len() == 32 && id.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        Ok(())
    } else {
        Err("Invalid Agent image attachment ID".to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    const TASK_A: &str = "018ddbcc-b80b-7c20-8123-000000000001";
    const TASK_B: &str = "018ddbcc-b80b-7c20-8123-000000000002";
    const TASK_C: &str = "018ddbcc-b80b-7c20-8123-000000000003";

    const PNG_1X1: &str = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNk+A8AAQUBAScY42YAAAAASUVORK5CYII=";

    #[test]
    fn stores_reads_and_deletes_session_scoped_images() {
        let temp = tempdir().unwrap();
        let store = fixture_attachment_store(temp.path(), &[TASK_A, TASK_B]);
        let prepared = store
            .store_uploads(
                TASK_A,
                &[AgentImageUpload {
                    name: "../screen.png".to_string(),
                    data_url: format!("data:image/png;base64,{PNG_1X1}"),
                }],
            )
            .unwrap();

        assert_eq!(prepared[0].attachment.name, "screen.png");
        assert_eq!(
            store.read(TASK_A, &prepared[0].attachment.id).unwrap(),
            BASE64_STANDARD.decode(PNG_1X1).unwrap()
        );
        assert!(store.read(TASK_B, &prepared[0].attachment.id).is_err());

        assert!(!temp.path().join("attachments").exists());
        store
            .store
            .mark_task_deleted(store.account, TASK_A, 2)
            .unwrap();
        assert!(store.read(TASK_A, &prepared[0].attachment.id).is_err());
        let mut collected = false;
        for _ in 0..100 {
            if store
                .store
                .collect_task_step(store.account, TASK_A)
                .unwrap()
                .done
            {
                collected = true;
                break;
            }
        }
        assert!(collected);
        assert!(store.store.task(store.account, TASK_A).unwrap().is_none());
    }

    #[test]
    fn rejects_mime_mismatches_and_unsupported_images() {
        let temp = tempdir().unwrap();
        let store = fixture_attachment_store(temp.path(), &[TASK_A]);

        let mismatch = store.store_uploads(
            TASK_A,
            &[AgentImageUpload {
                name: "screen.jpg".to_string(),
                data_url: format!("data:image/jpeg;base64,{PNG_1X1}"),
            }],
        );
        assert_eq!(
            mismatch.unwrap_err(),
            "Image content does not match its declared type"
        );

        let unsupported = store.store_uploads(
            TASK_A,
            &[AgentImageUpload {
                name: "screen.gif".to_string(),
                data_url: format!(
                    "data:image/gif;base64,{}",
                    BASE64_STANDARD.encode(b"GIF89a")
                ),
            }],
        );
        assert_eq!(
            unsupported.unwrap_err(),
            "Only JPEG, PNG, and WebP images are supported"
        );
    }

    #[test]
    fn attachment_adapter_captures_account_and_never_falls_back_to_files() {
        let temp = tempdir().unwrap();
        let store = fixture_attachment_store(temp.path(), &[TASK_A]);
        let prepared = store
            .store_uploads(
                TASK_A,
                &[AgentImageUpload {
                    name: "image.png".to_string(),
                    data_url: format!("data:image/png;base64,{PNG_1X1}"),
                }],
            )
            .unwrap();
        let other = store
            .store
            .upsert_account("other-attachment-fixture", 1)
            .unwrap();
        store
            .store
            .create_task(
                other,
                &crate::store::NewTask {
                    uuid: TASK_C.to_string(),
                    project_root: temp.path().to_string_lossy().into_owned(),
                    kind: crate::store::TaskKind::Desktop,
                    title: "Other account".to_string(),
                    provisional: false,
                    model: None,
                    created_ms: 1,
                },
            )
            .unwrap();
        let other = AgentAttachmentStore::new(Arc::clone(&store.store), other);
        assert!(other.read(TASK_A, &prepared[0].attachment.id).is_err());
        assert!(other.read(TASK_C, &prepared[0].attachment.id).is_err());

        // A previous-format fixture may remain on disk. An unknown SQL id
        // cannot select it, and the adapter never creates or removes its files.
        let old = temp.path().join("attachments");
        std::fs::create_dir(&old).unwrap();
        let id = "f".repeat(32);
        let untouched = old.join(&id);
        let bytes = BASE64_STANDARD.decode(PNG_1X1).unwrap();
        std::fs::write(&untouched, &bytes).unwrap();
        assert!(store.read(TASK_A, &id).is_err());
        assert_eq!(std::fs::read(untouched).unwrap(), bytes);
    }

    #[test]
    fn committed_attachment_is_read_after_reopening_the_new_store() {
        let temp = tempdir().unwrap();
        let store = fixture_attachment_store(temp.path(), &[TASK_A]);
        let prepared = store
            .store_uploads(
                TASK_A,
                &[AgentImageUpload {
                    name: "image.png".to_string(),
                    data_url: format!("data:image/png;base64,{PNG_1X1}"),
                }],
            )
            .unwrap();
        let id = prepared[0].attachment.id.clone();
        drop(store);
        let reopened = Arc::new(Store::open(crate::store::StorePaths::new(temp.path())).unwrap());
        let account = reopened.account_id("attachment-fixture").unwrap().unwrap();
        let store = AgentAttachmentStore::new(reopened, account);
        assert_eq!(
            store.read(TASK_A, &id).unwrap(),
            BASE64_STANDARD.decode(PNG_1X1).unwrap()
        );
    }

    fn attachment(id: char, name: &str) -> AgentImageAttachment {
        let id = id.to_string().repeat(32);
        AgentImageAttachment {
            source: format!("maple-attachment://{id}"),
            id,
            name: name.to_string(),
            mime_type: String::new(),
        }
    }

    #[test]
    fn an_image_prompt_names_the_images_and_reads_back() {
        let one = [attachment('a', "dialog.png")];
        let prompt = image_prompt(" Why is this misaligned? ", &one, false);
        assert_eq!(
            prompt,
            format!(
                "Why is this misaligned?\n\nThe user attached the following image:\n- \"dialog.png\": maple-attachment://{}\n{READ_IMAGE_HINT}",
                "a".repeat(32)
            )
        );
        assert_eq!(
            split_image_prompt(&prompt),
            Some(("Why is this misaligned?".to_string(), one.to_vec()))
        );

        // With the images beside the text, nothing asks for read_image; a
        // name may hold anything, and the text may be empty.
        let two = [attachment('b', "a \"b\"\nc.png"), attachment('c', "d.png")];
        let prompt = image_prompt("", &two, true);
        assert!(prompt.starts_with("The user attached the following images:\n"));
        assert!(!prompt.contains("read_image"));
        assert_eq!(
            split_image_prompt(&prompt),
            Some((String::new(), two.to_vec()))
        );

        // Text without images is left as it is.
        assert_eq!(image_prompt(" plain ", &[], false), "plain");
        for text in [
            "plain",
            "The user attached the following image: nothing",
            "x\nThe user attached the following image:\n- \"a\": maple-attachment://1",
        ] {
            assert_eq!(split_image_prompt(text), None, "{text}");
        }
    }
}
