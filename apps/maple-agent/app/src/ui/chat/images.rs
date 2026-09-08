//! Image attachments for the composer: staging drafts from the file
//! picker, the clipboard, and drag-and-drop, plus the decoding helpers
//! that run off the UI thread.

use std::sync::Arc;

use gpui::Context;

use super::{ChatScreen, DraftImage};

impl ChatScreen {
    /// How many more images the draft can take, or `None` with a notice
    /// set when the plan or the limit blocks attachments.
    fn remaining_image_slots(&mut self, cx: &mut Context<Self>) -> Option<usize> {
        if let Some(plan) = self.sidebar_plan.as_ref() {
            let label = plan.plan_label.to_lowercase();
            if !(label.contains("pro") || label.contains("max") || label.contains("team")) {
                self.notice = Some("Image attachments need a Pro, Max, or Team plan".into());
                cx.notify();
                return None;
            }
        }
        let remaining = MAX_DRAFT_IMAGES.saturating_sub(self.draft_images.len());
        if remaining == 0 {
            self.notice =
                Some(format!("Attach at most {MAX_DRAFT_IMAGES} images at a time").into());
            cx.notify();
            return None;
        }
        Some(remaining)
    }

    /// Stage an image pasted into the composer from the clipboard.
    pub(super) fn paste_image(&mut self, image: gpui::Image, cx: &mut Context<Self>) {
        if self.remaining_image_slots(cx).is_none() {
            return;
        }
        let extension = match image.format {
            gpui::ImageFormat::Jpeg => "jpg",
            gpui::ImageFormat::Webp => "webp",
            _ => "png",
        };
        let name = format!("pasted-{}.{extension}", self.draft_images.len() + 1);
        match draft_image_mime(&name, &image.bytes) {
            Ok(mime) => {
                let id = self.next_draft_id();
                self.notice = None;
                self.draft_images.push(DraftImage {
                    id,
                    name,
                    data_url: None,
                    thumbnail: None,
                });
                self.prepare_draft_image(id, mime, image.bytes, cx);
            }
            Err(message) => self.notice = Some(message.into()),
        }
        cx.notify();
    }

    fn next_draft_id(&mut self) -> u64 {
        self.draft_counter += 1;
        self.draft_counter
    }

    /// Encode the data URL and center-crop the thumbnail off the UI
    /// thread, then attach both to the draft with `id` if it is still
    /// staged. A pasted image can be 10 MB; encoding it inline stalled
    /// the frame.
    fn prepare_draft_image(
        &mut self,
        id: u64,
        mime: &'static str,
        bytes: Vec<u8>,
        cx: &mut Context<Self>,
    ) {
        self.call(
            async move {
                tokio::task::spawn_blocking(move || {
                    let data_url = encode_data_url(mime, &bytes);
                    let thumbnail = square_thumbnail(&bytes);
                    Ok::<_, String>((data_url, thumbnail))
                })
                .await
                .map_err(|error| format!("Image task failed: {error}"))?
            },
            cx,
            move |this, result, cx| {
                let Some(draft) = this.draft_images.iter_mut().find(|draft| draft.id == id) else {
                    return;
                };
                match result {
                    Ok((data_url, thumbnail)) => {
                        draft.data_url = Some(data_url);
                        match thumbnail {
                            Ok(image) => draft.thumbnail = Some(Arc::new(image)),
                            Err(message) => {
                                log::debug!("thumbnail for {}: {message}", draft.name)
                            }
                        }
                    }
                    Err(message) => {
                        // Without a data URL the draft can never be sent.
                        let name = std::mem::take(&mut draft.name);
                        this.draft_images.retain(|draft| draft.id != id);
                        this.notice = Some(format!("Could not prepare {name}: {message}").into());
                    }
                }
                cx.notify();
            },
        );
    }

    pub(super) fn pick_images(&mut self, cx: &mut Context<Self>) {
        if self.image_picking {
            return;
        }
        if self.remaining_image_slots(cx).is_none() {
            return;
        }
        self.image_picking = true;
        self.notice = None;
        cx.notify();
        // The platform file picker through gpui, native on every OS. It
        // has no file-type filter; `add_image_paths` re-checks the limit
        // and skips non-image files with a notice.
        let receiver = cx.prompt_for_paths(gpui::PathPromptOptions {
            files: true,
            directories: false,
            multiple: true,
            prompt: None,
        });
        let bridge = cx.spawn(async move |this, cx| {
            let picked = receiver.await;
            this.update(cx, |this, cx| {
                this.image_picking = false;
                match picked {
                    Ok(Ok(Some(paths))) => this.add_image_paths(paths, cx),
                    // Cancelled, or the picker dropped its channel.
                    Ok(Ok(None)) | Err(_) => {}
                    Ok(Err(error)) => {
                        this.notice =
                            Some(format!("Could not open the file dialog: {error}").into());
                    }
                }
                cx.notify();
            })
            .ok();
        });
        crate::ui::task::retain(&self.bridged_tasks, bridge);
    }

    /// Stage images dropped onto the composer. Files that are not PNG,
    /// JPEG, or WebP are skipped with a notice.
    pub(super) fn add_image_paths(
        &mut self,
        paths: Vec<std::path::PathBuf>,
        cx: &mut Context<Self>,
    ) {
        let (images, other): (Vec<_>, Vec<_>) = paths.into_iter().partition(|path| {
            path.extension()
                .and_then(|extension| extension.to_str())
                .is_some_and(|extension| {
                    matches!(
                        extension.to_ascii_lowercase().as_str(),
                        "png" | "jpg" | "jpeg" | "webp"
                    )
                })
        });
        if !other.is_empty() {
            self.notice = Some(
                format!(
                    "Only PNG, JPEG, and WebP images can be attached ({} file{} skipped)",
                    other.len(),
                    if other.len() == 1 { "" } else { "s" }
                )
                .into(),
            );
            cx.notify();
        }
        if images.is_empty() {
            return;
        }
        let Some(remaining) = self.remaining_image_slots(cx) else {
            return;
        };
        self.call(
            async move {
                tokio::task::spawn_blocking(move || {
                    images
                        .iter()
                        .take(remaining)
                        .map(|path| load_draft_image(path))
                        .collect::<Result<Vec<_>, String>>()
                })
                .await
                .map_err(|error| format!("Image load failed: {error}"))?
            },
            cx,
            |this, result, cx| {
                match result {
                    Ok(images) => {
                        for mut image in images {
                            image.id = this.next_draft_id();
                            this.draft_images.push(image);
                        }
                    }
                    Err(message) => this.notice = Some(message.into()),
                }
                cx.notify();
            },
        );
    }
}

pub(super) const MAX_DRAFT_IMAGES: usize = 10;
const MAX_DRAFT_IMAGE_BYTES: usize = 10 * 1024 * 1024;

/// Read an image file into a draft with its thumbnail, checking size and
/// format the same way the runtime does so errors surface before the send.
/// Runs on a blocking thread; the caller assigns the id.
fn load_draft_image(path: &std::path::Path) -> Result<DraftImage, String> {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "image".to_string());
    let bytes = std::fs::read(path).map_err(|error| format!("Could not read {name}: {error}"))?;
    let mime = draft_image_mime(&name, &bytes)?;
    Ok(DraftImage {
        id: 0,
        name,
        data_url: Some(encode_data_url(mime, &bytes)),
        thumbnail: square_thumbnail(&bytes).ok().map(Arc::new),
    })
}

/// Check size and format the same way the runtime does, so errors
/// surface before the send. Cheap: reads the signature only.
fn draft_image_mime(name: &str, bytes: &[u8]) -> Result<&'static str, String> {
    if bytes.len() > MAX_DRAFT_IMAGE_BYTES {
        return Err(format!("{name} is larger than 10 MB"));
    }
    if bytes.starts_with(&[0x89, b'P', b'N', b'G']) {
        Ok("image/png")
    } else if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        Ok("image/jpeg")
    } else if bytes.len() > 12 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        Ok("image/webp")
    } else {
        Err(format!("{name} is not a PNG, JPEG, or WebP image"))
    }
}

/// The `data:` URL the runtime stores with the message. Runs on a
/// blocking thread: the payload can be 10 MB.
pub(super) fn encode_data_url(mime: &str, bytes: &[u8]) -> Arc<str> {
    use base64::Engine as _;
    let mut url = String::with_capacity(mime.len() + 16 + bytes.len().div_ceil(3) * 4);
    url.push_str("data:");
    url.push_str(mime);
    url.push_str(";base64,");
    base64::engine::general_purpose::STANDARD.encode_string(bytes, &mut url);
    Arc::from(url)
}

/// Thumbnail edge in physical pixels: 2x the 64pt box so it stays sharp
/// on HiDPI screens.
const DRAFT_THUMBNAIL_PX: u32 = 128;

/// Center-crop to a square and scale down, so the composer can show a
/// rounded square that is the picture itself. gpui clips with rectangular
/// masks only, so cropping the pixels is the one way to get round corners
/// on a cover-fit thumbnail.
fn square_thumbnail(bytes: &[u8]) -> Result<gpui::Image, String> {
    let decoded = image::load_from_memory(bytes).map_err(|error| error.to_string())?;
    let (width, height) = (decoded.width(), decoded.height());
    let edge = width.min(height);
    if edge == 0 {
        return Err("empty image".to_string());
    }
    let cropped = decoded.crop_imm((width - edge) / 2, (height - edge) / 2, edge, edge);
    let scaled = if edge > DRAFT_THUMBNAIL_PX {
        cropped.resize_exact(
            DRAFT_THUMBNAIL_PX,
            DRAFT_THUMBNAIL_PX,
            image::imageops::FilterType::Triangle,
        )
    } else {
        cropped
    };
    let mut png = std::io::Cursor::new(Vec::new());
    scaled
        .write_to(&mut png, image::ImageFormat::Png)
        .map_err(|error| error.to_string())?;
    Ok(gpui::Image::from_bytes(
        gpui::ImageFormat::Png,
        png.into_inner(),
    ))
}

/// Image format from the file signature; `None` for unsupported data.
pub(super) fn image_format_from_bytes(bytes: &[u8]) -> Option<gpui::ImageFormat> {
    if bytes.starts_with(&[0x89, b'P', b'N', b'G']) {
        Some(gpui::ImageFormat::Png)
    } else if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        Some(gpui::ImageFormat::Jpeg)
    } else if bytes.len() > 12 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        Some(gpui::ImageFormat::Webp)
    } else {
        None
    }
}
