use goose::agents::mcp_client::{Error, McpClientTrait};
use goose::agents::platform_extensions::developer::DeveloperClient;
use goose::agents::platform_extensions::PlatformExtensionContext;
use goose::agents::ToolCallContext;
use once_cell::sync::Lazy;
use rmcp::model::{
    CallToolResult, Content, Implementation, InitializeResult, JsonObject, ListToolsResult,
    ServerCapabilities, Tool, ToolAnnotations,
};
use rmcp::object;
use serde::{de::Error as SerdeDeError, Deserialize, Deserializer};
use std::collections::HashMap;
use std::fs;
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex as StdMutex, Weak};
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

use super::shell_permission::is_remote_image_source;

const MAX_READ_LINES: usize = 2_000;
const MAX_READ_BYTES: usize = 50 * 1024;
const MAPLE_DEVELOPER_INSTRUCTIONS: &str = r#"Use the developer tools to inspect and modify the project.

Use read to examine text files instead of cat or sed. Use shell for searches, directory listings,
and commands that do not fit a dedicated tool. Use edit for exact targeted replacements and write
only for new files or complete rewrites. Use read_image when you need to inspect an image."#;

type MutationLock = Mutex<()>;
type MutationLockMap = HashMap<PathBuf, Weak<MutationLock>>;

static MUTATION_LOCKS: Lazy<StdMutex<MutationLockMap>> =
    Lazy::new(|| StdMutex::new(HashMap::new()));

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ReadParams {
    path: String,
    offset: Option<usize>,
    limit: Option<usize>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Replacement {
    old_text: String,
    new_text: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct EditParams {
    path: String,
    #[serde(deserialize_with = "deserialize_edits")]
    edits: Vec<Replacement>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct WriteParams {
    path: String,
    content: String,
}

fn deserialize_edits<'de, D>(deserializer: D) -> Result<Vec<Replacement>, D::Error>
where
    D: Deserializer<'de>,
{
    let value = serde_json::Value::deserialize(deserializer)?;
    match value {
        serde_json::Value::Array(_) => serde_json::from_value(value).map_err(D::Error::custom),
        serde_json::Value::String(json) => serde_json::from_str(&json).map_err(D::Error::custom),
        _ => Err(D::Error::custom("edits must be an array")),
    }
}

pub(crate) struct MapleDeveloperClient {
    info: InitializeResult,
    goose: DeveloperClient,
}

impl MapleDeveloperClient {
    pub(crate) fn new(context: PlatformExtensionContext) -> anyhow::Result<Self> {
        let info = InitializeResult::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new("developer", "1.0.0").with_title("Developer"))
            .with_instructions(MAPLE_DEVELOPER_INSTRUCTIONS);

        Ok(Self {
            info,
            goose: DeveloperClient::new(context)?,
        })
    }

    fn read_tool() -> Tool {
        Tool::new(
            "read".to_string(),
            format!(
                "Read a text file. Output is limited to {MAX_READ_LINES} lines or {}KB, whichever is reached first. Use offset and limit to continue through large files. Use read_image for images.",
                MAX_READ_BYTES / 1024
            ),
            object!({
                "type": "object",
                "additionalProperties": false,
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "Path to the file to read (relative or absolute)"
                    },
                    "offset": {
                        "type": "integer",
                        "minimum": 1,
                        "description": "Line number to start reading from (1-indexed)"
                    },
                    "limit": {
                        "type": "integer",
                        "minimum": 1,
                        "description": "Maximum number of lines to read"
                    }
                },
                "required": ["path"]
            }),
        )
        .annotate(ToolAnnotations::from_raw(
            Some("Read".to_string()),
            Some(true),
            Some(false),
            Some(true),
            Some(false),
        ))
    }

    fn edit_tool() -> Tool {
        Tool::new(
            "edit".to_string(),
            "Apply one or more exact, unique text replacements to a file. Every oldText is matched against the original file, all replacements are validated before writing, and overlapping edits are rejected."
                .to_string(),
            object!({
                "type": "object",
                "additionalProperties": false,
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "Path to the file to edit (relative or absolute)"
                    },
                    "edits": {
                        "type": "array",
                        "minItems": 1,
                        "description": "Exact, non-overlapping replacements matched against the original file",
                        "items": {
                            "type": "object",
                            "additionalProperties": false,
                            "properties": {
                                "oldText": {
                                    "type": "string",
                                    "minLength": 1,
                                    "description": "Exact text that must occur once in the original file"
                                },
                                "newText": {
                                    "type": "string",
                                    "description": "Replacement text; use an empty string to delete"
                                }
                            },
                            "required": ["oldText", "newText"]
                        }
                    }
                },
                "required": ["path", "edits"]
            }),
        )
        .annotate(ToolAnnotations::from_raw(
            Some("Edit".to_string()),
            Some(false),
            Some(true),
            Some(false),
            Some(false),
        ))
    }

    fn read_image_tool(mut tool: Tool) -> Tool {
        tool.description = Some(
            "Read an image from a local file path or http(s) URL and return it as image content for the model to inspect. Remote URLs require approval in Read only mode. Supports png, jpeg, gif, and webp."
                .into(),
        );
        let mut schema = tool.input_schema.as_ref().clone();
        if let Some(source) = schema
            .get_mut("properties")
            .and_then(serde_json::Value::as_object_mut)
            .and_then(|properties| properties.get_mut("source"))
            .and_then(serde_json::Value::as_object_mut)
        {
            source.insert(
                "description".to_string(),
                serde_json::Value::String(
                    "Local file path or http(s) URL. Remote URLs require approval in Read only mode."
                        .to_string(),
                ),
            );
        }
        tool.input_schema = Arc::new(schema);
        tool.annotations = Some(ToolAnnotations::from_raw(
            Some("Read Image".to_string()),
            Some(false),
            Some(false),
            Some(true),
            Some(true),
        ));
        tool
    }

    fn write_tool() -> Tool {
        Tool::new(
            "write".to_string(),
            "Write content to a file. Creates the file if it does not exist, overwrites it if it does, and creates parent directories as needed."
                .to_string(),
            object!({
                "type": "object",
                "additionalProperties": false,
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "Path to the file to write (relative or absolute)"
                    },
                    "content": {
                        "type": "string",
                        "description": "Complete content to write to the file"
                    }
                },
                "required": ["path", "content"]
            }),
        )
        .annotate(ToolAnnotations::from_raw(
            Some("Write".to_string()),
            Some(false),
            Some(true),
            Some(false),
            Some(false),
        ))
    }

    fn parse_args<T: serde::de::DeserializeOwned>(
        arguments: Option<JsonObject>,
    ) -> Result<T, String> {
        let value = arguments
            .map(serde_json::Value::Object)
            .ok_or_else(|| "Missing arguments".to_string())?;
        serde_json::from_value(value).map_err(|error| format!("Invalid arguments: {error}"))
    }
}

#[async_trait::async_trait]
impl McpClientTrait for MapleDeveloperClient {
    async fn list_tools(
        &self,
        session_id: &str,
        next_cursor: Option<String>,
        cancel_token: CancellationToken,
    ) -> Result<ListToolsResult, Error> {
        let delegated = self
            .goose
            .list_tools(session_id, next_cursor, cancel_token)
            .await?;
        let mut delegated_by_name = delegated
            .tools
            .into_iter()
            .map(|tool| (tool.name.to_string(), tool))
            .collect::<HashMap<_, _>>();

        let mut tools = vec![Self::read_tool()];
        if let Some(shell) = delegated_by_name.remove("shell") {
            tools.push(shell);
        }
        tools.push(Self::edit_tool());
        tools.push(Self::write_tool());
        if let Some(read_image) = delegated_by_name.remove("read_image") {
            tools.push(Self::read_image_tool(read_image));
        }

        Ok(ListToolsResult {
            tools,
            next_cursor: None,
            meta: None,
        })
    }

    async fn call_tool(
        &self,
        ctx: &ToolCallContext,
        name: &str,
        arguments: Option<JsonObject>,
        cancel_token: CancellationToken,
    ) -> Result<CallToolResult, Error> {
        let working_dir = ctx.working_dir.as_deref();
        let result = match name {
            "read" => match Self::parse_args::<ReadParams>(arguments) {
                Ok(params) => read_file(params, working_dir, cancel_token).await,
                Err(error) => error_result(error),
            },
            "edit" => match Self::parse_args::<EditParams>(arguments) {
                Ok(params) => edit_file(params, working_dir, cancel_token).await,
                Err(error) => error_result(error),
            },
            "write" => match Self::parse_args::<WriteParams>(arguments) {
                Ok(params) => write_file(params, working_dir, cancel_token).await,
                Err(error) => error_result(error),
            },
            "shell" => {
                return self
                    .goose
                    .call_tool(ctx, name, arguments, cancel_token)
                    .await;
            }
            "read_image" => {
                let arguments = normalize_read_image_arguments(arguments, working_dir);
                return self
                    .goose
                    .call_tool(ctx, name, arguments, cancel_token)
                    .await;
            }
            _ => error_result(format!("Unknown tool: {name}")),
        };
        Ok(result)
    }

    fn get_info(&self) -> Option<&InitializeResult> {
        Some(&self.info)
    }
}

fn success_result(text: impl Into<String>) -> CallToolResult {
    CallToolResult::success(vec![Content::text(text.into()).with_priority(0.0)])
}

fn error_result(text: impl Into<String>) -> CallToolResult {
    CallToolResult::error(vec![
        Content::text(format!("Error: {}", text.into())).with_priority(0.0)
    ])
}

fn resolve_path(path: &str, working_dir: Option<&Path>) -> PathBuf {
    let expanded = if path == "~" {
        home_dir().unwrap_or_else(|| PathBuf::from(path))
    } else if let Some(relative) = path.strip_prefix("~/").or_else(|| path.strip_prefix("~\\")) {
        home_dir()
            .map(|home| home.join(relative))
            .unwrap_or_else(|| PathBuf::from(path))
    } else {
        PathBuf::from(path)
    };

    if expanded.is_absolute() {
        expanded
    } else {
        working_dir
            .map(Path::to_path_buf)
            .or_else(|| std::env::current_dir().ok())
            .unwrap_or_else(|| PathBuf::from("."))
            .join(expanded)
    }
}

fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
}

fn normalize_read_image_arguments(
    mut arguments: Option<JsonObject>,
    working_dir: Option<&Path>,
) -> Option<JsonObject> {
    let source = arguments
        .as_ref()
        .and_then(|arguments| arguments.get("source"))
        .and_then(serde_json::Value::as_str)
        .map(str::to_string);
    let Some(source) = source else {
        return arguments;
    };
    if is_remote_image_source(&source)
        || reqwest::Url::parse(&source).is_ok_and(|url| url.scheme() == "file")
    {
        return arguments;
    }

    if let Some(arguments) = arguments.as_mut() {
        arguments.insert(
            "source".to_string(),
            serde_json::Value::String(
                resolve_path(&source, working_dir)
                    .to_string_lossy()
                    .into_owned(),
            ),
        );
    }
    arguments
}

async fn read_file(
    params: ReadParams,
    working_dir: Option<&Path>,
    cancel_token: CancellationToken,
) -> CallToolResult {
    if params.offset == Some(0) {
        return error_result("offset must be at least 1");
    }
    if params.limit == Some(0) {
        return error_result("limit must be at least 1");
    }

    let path = resolve_path(&params.path, working_dir);
    let worker_cancel_token = cancel_token.clone();
    let task =
        tokio::task::spawn_blocking(move || read_file_blocking(params, path, worker_cancel_token));
    tokio::select! {
        biased;
        _ = cancel_token.cancelled() => error_result("Read cancelled"),
        result = task => match result {
            Ok(result) => result,
            Err(error) => error_result(format!("Read task failed: {error}")),
        },
    }
}

fn read_file_blocking(
    params: ReadParams,
    path: PathBuf,
    cancel_token: CancellationToken,
) -> CallToolResult {
    if cancel_token.is_cancelled() {
        return error_result("Read cancelled");
    }

    let metadata = match fs::metadata(&path) {
        Ok(metadata) => metadata,
        Err(error) => return error_result(format!("Failed to read {}: {error}", params.path)),
    };
    if !metadata.is_file() {
        return error_result(format!("{} is not a regular file", params.path));
    }

    let mut file = match fs::File::open(&path) {
        Ok(file) => file,
        Err(error) => return error_result(format!("Failed to read {}: {error}", params.path)),
    };
    if !file.metadata().is_ok_and(|metadata| metadata.is_file()) {
        return error_result(format!("{} is not a regular file", params.path));
    }

    let mut magic = [0u8; 12];
    let magic_len = match file.read(&mut magic) {
        Ok(length) => length,
        Err(error) => return error_result(format!("Failed to read {}: {error}", params.path)),
    };
    if is_supported_image(&magic[..magic_len]) {
        return success_result(format!(
            "{} is an image. Use read_image to inspect it.",
            params.path
        ));
    }
    if let Err(error) = file.seek(SeekFrom::Start(0)) {
        return error_result(format!("Failed to read {}: {error}", params.path));
    }

    let mut reader = BufReader::new(file);
    let start = params.offset.unwrap_or(1) - 1;
    for lines_seen in 0..start {
        match read_stream_line(&mut reader, None, &cancel_token) {
            Ok(Some(_)) => {}
            Ok(None) => {
                return error_result(format!(
                    "Offset {} is beyond end of file ({lines_seen} lines total)",
                    params.offset.unwrap_or(1)
                ));
            }
            Err(error) => return stream_read_error(&params.path, error),
        }
    }

    let line_limit = params.limit.unwrap_or(usize::MAX).min(MAX_READ_LINES);
    let mut output_lines = Vec::new();
    let mut output_bytes = 0usize;
    let mut has_more = false;
    let mut first_selected_line = true;

    while output_lines.len() < line_limit {
        let separator_bytes = usize::from(!output_lines.is_empty());
        let Some(remaining_bytes) = MAX_READ_BYTES.checked_sub(output_bytes + separator_bytes)
        else {
            has_more = match read_stream_line(&mut reader, Some(0), &cancel_token) {
                Ok(line) => line.is_some(),
                Err(error) => return stream_read_error(&params.path, error),
            };
            break;
        };
        let line = match read_stream_line(&mut reader, Some(remaining_bytes), &cancel_token) {
            Ok(Some(line)) => line,
            Ok(None) if first_selected_line && start > 0 => {
                return error_result(format!(
                    "Offset {} is beyond end of file ({start} lines total)",
                    params.offset.unwrap_or(1)
                ));
            }
            Ok(None) => break,
            Err(error) => return stream_read_error(&params.path, error),
        };
        first_selected_line = false;

        let text = String::from_utf8_lossy(&line.bytes).into_owned();
        if line.exceeded_limit || text.len() > remaining_bytes {
            if output_lines.is_empty() {
                return success_result(format!(
                    "[Line {} exceeds the {}KB read limit. Use shell with a byte-limiting command to inspect it.]",
                    start + 1,
                    MAX_READ_BYTES / 1024
                ));
            }
            has_more = true;
            break;
        }

        output_bytes += separator_bytes + text.len();
        output_lines.push(text);
    }

    if !has_more && output_lines.len() == line_limit {
        has_more = match read_stream_line(&mut reader, Some(0), &cancel_token) {
            Ok(line) => line.is_some(),
            Err(error) => return stream_read_error(&params.path, error),
        };
    }

    let mut output = output_lines.join("\n");
    if has_more {
        let first_line = start + 1;
        let last_line = start + output_lines.len();
        let next_offset = last_line + 1;
        output.push_str(&format!(
            "\n\n[Showing lines {first_line}-{last_line}. Use offset={next_offset} to continue.]"
        ));
    }

    success_result(output)
}

struct StreamedLine {
    bytes: Vec<u8>,
    exceeded_limit: bool,
}

enum StreamReadError {
    Cancelled,
    Io(std::io::Error),
}

fn read_stream_line<R: BufRead>(
    reader: &mut R,
    capture_limit: Option<usize>,
    cancel_token: &CancellationToken,
) -> Result<Option<StreamedLine>, StreamReadError> {
    let mut bytes = Vec::new();
    let mut saw_any = false;

    loop {
        if cancel_token.is_cancelled() {
            return Err(StreamReadError::Cancelled);
        }

        let (consumed, ended, exceeded_limit) = {
            let available = reader.fill_buf().map_err(StreamReadError::Io)?;
            if available.is_empty() {
                if !saw_any {
                    return Ok(None);
                }
                if bytes.last() == Some(&b'\r') {
                    bytes.pop();
                }
                return Ok(Some(StreamedLine {
                    bytes,
                    exceeded_limit: false,
                }));
            }
            saw_any = true;

            let newline = available.iter().position(|byte| *byte == b'\n');
            let segment_len = newline.unwrap_or(available.len());
            let mut exceeded_limit = false;
            let mut captured = 0usize;
            if let Some(limit) = capture_limit {
                let remaining = limit.saturating_sub(bytes.len());
                captured = remaining.min(segment_len);
                bytes.extend_from_slice(&available[..captured]);
                exceeded_limit = segment_len > remaining;
            }

            if exceeded_limit {
                ((captured + 1).min(segment_len), false, true)
            } else {
                (
                    segment_len + usize::from(newline.is_some()),
                    newline.is_some(),
                    false,
                )
            }
        };
        reader.consume(consumed);

        if exceeded_limit {
            return Ok(Some(StreamedLine {
                bytes,
                exceeded_limit: true,
            }));
        }
        if ended {
            if bytes.last() == Some(&b'\r') {
                bytes.pop();
            }
            return Ok(Some(StreamedLine {
                bytes,
                exceeded_limit: false,
            }));
        }
    }
}

fn stream_read_error(path: &str, error: StreamReadError) -> CallToolResult {
    match error {
        StreamReadError::Cancelled => error_result("Read cancelled"),
        StreamReadError::Io(error) => error_result(format!("Failed to read {path}: {error}")),
    }
}

fn is_supported_image(bytes: &[u8]) -> bool {
    bytes.starts_with(b"\x89PNG\r\n\x1a\n")
        || bytes.starts_with(&[0xff, 0xd8, 0xff])
        || bytes.starts_with(b"GIF87a")
        || bytes.starts_with(b"GIF89a")
        || (bytes.len() >= 12 && bytes.starts_with(b"RIFF") && &bytes[8..12] == b"WEBP")
}

async fn write_file(
    params: WriteParams,
    working_dir: Option<&Path>,
    cancel_token: CancellationToken,
) -> CallToolResult {
    let path = resolve_path(&params.path, working_dir);
    let lock = mutation_lock(&path);
    let _guard = tokio::select! {
        biased;
        _ = cancel_token.cancelled() => return error_result("Write cancelled"),
        guard = lock.lock() => guard,
    };
    let worker_cancel_token = cancel_token.clone();
    match tokio::task::spawn_blocking(move || {
        write_file_blocking(params, path, worker_cancel_token)
    })
    .await
    {
        Ok(result) => result,
        Err(error) => error_result(format!("Write task failed: {error}")),
    }
}

fn write_file_blocking(
    params: WriteParams,
    path: PathBuf,
    cancel_token: CancellationToken,
) -> CallToolResult {
    if cancel_token.is_cancelled() {
        return error_result("Write cancelled");
    }

    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            if let Err(error) = fs::create_dir_all(parent) {
                return error_result(format!(
                    "Failed to create directory {}: {error}",
                    parent.display()
                ));
            }
        }
    }
    if cancel_token.is_cancelled() {
        return error_result("Write cancelled");
    }

    let existed = path.exists();
    match fs::write(&path, params.content.as_bytes()) {
        Ok(()) => {
            let action = if existed { "Wrote" } else { "Created" };
            success_result(format!(
                "{action} {} ({} bytes)",
                params.path,
                params.content.len()
            ))
        }
        Err(error) => error_result(format!("Failed to write {}: {error}", params.path)),
    }
}

async fn edit_file(
    params: EditParams,
    working_dir: Option<&Path>,
    cancel_token: CancellationToken,
) -> CallToolResult {
    if params.edits.is_empty() {
        return error_result("edits must contain at least one replacement");
    }

    let path = resolve_path(&params.path, working_dir);
    let lock = mutation_lock(&path);
    let _guard = tokio::select! {
        biased;
        _ = cancel_token.cancelled() => return error_result("Edit cancelled"),
        guard = lock.lock() => guard,
    };
    let worker_cancel_token = cancel_token.clone();
    match tokio::task::spawn_blocking(move || edit_file_blocking(params, path, worker_cancel_token))
        .await
    {
        Ok(result) => result,
        Err(error) => error_result(format!("Edit task failed: {error}")),
    }
}

fn edit_file_blocking(
    params: EditParams,
    path: PathBuf,
    cancel_token: CancellationToken,
) -> CallToolResult {
    if cancel_token.is_cancelled() {
        return error_result("Edit cancelled");
    }
    let bytes = match fs::read(&path) {
        Ok(bytes) => bytes,
        Err(error) => return error_result(format!("Failed to read {}: {error}", params.path)),
    };
    let original = match String::from_utf8(bytes) {
        Ok(content) => content,
        Err(_) => return error_result(format!("{} is not a UTF-8 text file", params.path)),
    };

    let (bom, line_ending, mut normalized) = normalize_text_file(&original);
    let mut resolved_edits = Vec::with_capacity(params.edits.len());
    let mut has_change = false;
    for (index, replacement) in params.edits.iter().enumerate() {
        let old_text = normalize_newlines(&replacement.old_text);
        let new_text = normalize_newlines(&replacement.new_text);
        if old_text.is_empty() {
            return error_result(format!("edits[{index}].oldText must not be empty"));
        }

        let matches = overlapping_match_positions(&normalized, &old_text);
        match matches.as_slice() {
            [] => {
                return error_result(format!(
                    "edits[{index}].oldText was not found in {}",
                    params.path
                ));
            }
            [start] => {
                has_change |= old_text != new_text;
                resolved_edits.push((*start, *start + old_text.len(), new_text));
            }
            _ => {
                return error_result(format!(
                    "edits[{index}].oldText matched {} times; include more context so it is unique",
                    matches.len()
                ));
            }
        }
    }
    if !has_change {
        return error_result("edits would not change the file");
    }

    resolved_edits.sort_by_key(|(start, _, _)| *start);
    for pair in resolved_edits.windows(2) {
        if pair[1].0 < pair[0].1 {
            return error_result("edits contain overlapping replacements");
        }
    }

    for (start, end, replacement) in resolved_edits.iter().rev() {
        normalized.replace_range(*start..*end, replacement);
    }

    if cancel_token.is_cancelled() {
        return error_result("Edit cancelled");
    }
    let updated = restore_text_file(&normalized, bom, line_ending);
    match fs::write(&path, updated.as_bytes()) {
        Ok(()) => success_result(format!(
            "Edited {} ({} replacements)",
            params.path,
            resolved_edits.len()
        )),
        Err(error) => error_result(format!("Failed to write {}: {error}", params.path)),
    }
}

#[derive(Clone, Copy)]
enum LineEnding {
    Lf,
    CrLf,
    Cr,
}

fn normalize_text_file(content: &str) -> (bool, LineEnding, String) {
    let (bom, content) = match content.strip_prefix('\u{feff}') {
        Some(content) => (true, content),
        None => (false, content),
    };
    let line_ending = if content.contains("\r\n") {
        LineEnding::CrLf
    } else if content.contains('\r') {
        LineEnding::Cr
    } else {
        LineEnding::Lf
    };
    (bom, line_ending, normalize_newlines(content))
}

fn normalize_newlines(content: &str) -> String {
    content.replace("\r\n", "\n").replace('\r', "\n")
}

fn restore_text_file(content: &str, bom: bool, line_ending: LineEnding) -> String {
    let content = match line_ending {
        LineEnding::Lf => content.to_string(),
        LineEnding::CrLf => content.replace('\n', "\r\n"),
        LineEnding::Cr => content.replace('\n', "\r"),
    };
    if bom {
        format!("\u{feff}{content}")
    } else {
        content
    }
}

fn overlapping_match_positions(haystack: &str, needle: &str) -> Vec<usize> {
    let mut positions = Vec::new();
    let mut search_start = 0usize;
    while search_start <= haystack.len() {
        let Some(relative) = haystack[search_start..].find(needle) else {
            break;
        };
        let position = search_start + relative;
        positions.push(position);
        let advance = haystack[position..]
            .chars()
            .next()
            .map(char::len_utf8)
            .unwrap_or(1);
        search_start = position + advance;
    }
    positions
}

fn mutation_lock(path: &Path) -> Arc<MutationLock> {
    let key = mutation_key(path);
    let mut locks = MUTATION_LOCKS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    locks.retain(|_, lock| lock.strong_count() > 0);
    if let Some(lock) = locks.get(&key).and_then(Weak::upgrade) {
        return lock;
    }

    let lock = Arc::new(Mutex::new(()));
    locks.insert(key, Arc::downgrade(&lock));
    lock
}

fn mutation_key(path: &Path) -> PathBuf {
    if let Ok(canonical) = fs::canonicalize(path) {
        return canonical;
    }
    if let (Some(parent), Some(file_name)) = (path.parent(), path.file_name()) {
        if let Ok(canonical_parent) = fs::canonicalize(parent) {
            return canonical_parent.join(file_name);
        }
    }
    path.to_path_buf()
}

#[cfg(test)]
mod tests {
    use super::*;
    use goose::session::SessionManager;
    use rmcp::model::RawContent;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT_TEST_DIR: AtomicU64 = AtomicU64::new(1);

    struct TestDir(PathBuf);

    impl TestDir {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "maple-developer-tools-{}-{}",
                std::process::id(),
                NEXT_TEST_DIR.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir_all(&path).unwrap();
            Self(path)
        }

        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for TestDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn test_context(data_dir: PathBuf) -> PlatformExtensionContext {
        PlatformExtensionContext {
            extension_manager: None,
            session_manager: Arc::new(SessionManager::new(data_dir)),
            session: None,
            use_login_shell_path: false,
        }
    }

    fn text(result: &CallToolResult) -> &str {
        match &result.content[0].raw {
            RawContent::Text(text) => &text.text,
            _ => panic!("expected text content"),
        }
    }

    #[tokio::test]
    async fn exposes_only_pi_style_defaults_plus_read_image() {
        let temp = TestDir::new();
        let client = MapleDeveloperClient::new(test_context(temp.path().join("sessions"))).unwrap();
        let result = client
            .list_tools("session", None, CancellationToken::new())
            .await
            .unwrap();
        let names = result
            .tools
            .iter()
            .map(|tool| tool.name.as_ref())
            .collect::<Vec<_>>();
        assert_eq!(names, ["read", "shell", "edit", "write", "read_image"]);
        assert!(!names.contains(&"tree"));

        let read = serde_json::to_value(&result.tools[0]).unwrap();
        assert_eq!(read["annotations"]["readOnlyHint"], true);
        assert_eq!(read["annotations"]["destructiveHint"], false);
        let shell = serde_json::to_value(&result.tools[1]).unwrap();
        assert_eq!(shell["annotations"]["readOnlyHint"], false);
        let edit = serde_json::to_value(&result.tools[2]).unwrap();
        assert_eq!(edit["annotations"]["readOnlyHint"], false);
        let write = serde_json::to_value(&result.tools[3]).unwrap();
        assert_eq!(write["annotations"]["readOnlyHint"], false);
        let read_image = serde_json::to_value(&result.tools[4]).unwrap();
        assert_eq!(read_image["annotations"]["readOnlyHint"], false);
        assert_eq!(read_image["annotations"]["openWorldHint"], true);
        assert!(read_image["description"]
            .as_str()
            .unwrap()
            .contains("Remote URLs require approval"));
        assert_eq!(
            result.tools[2].input_schema["properties"]["edits"]["minItems"],
            1
        );
    }

    #[tokio::test]
    async fn read_supports_offsets_limits_and_continuation() {
        let temp = TestDir::new();
        fs::write(temp.path().join("notes.txt"), "one\ntwo\nthree\nfour").unwrap();
        let result = read_file(
            ReadParams {
                path: "notes.txt".to_string(),
                offset: Some(2),
                limit: Some(2),
            },
            Some(temp.path()),
            CancellationToken::new(),
        )
        .await;
        assert_eq!(result.is_error, Some(false));
        assert_eq!(
            text(&result),
            "two\nthree\n\n[Showing lines 2-3. Use offset=4 to continue.]"
        );
    }

    #[tokio::test]
    async fn read_rejects_offsets_past_eof() {
        let temp = TestDir::new();
        fs::write(temp.path().join("notes.txt"), "one\ntwo").unwrap();
        let result = read_file(
            ReadParams {
                path: "notes.txt".to_string(),
                offset: Some(3),
                limit: None,
            },
            Some(temp.path()),
            CancellationToken::new(),
        )
        .await;
        assert_eq!(result.is_error, Some(true));
        assert!(text(&result).contains("beyond end of file"));
    }

    #[tokio::test]
    async fn read_truncates_on_complete_lines_with_next_offset() {
        let temp = TestDir::new();
        let content = (1..=MAX_READ_LINES + 1)
            .map(|line| format!("line-{line}"))
            .collect::<Vec<_>>()
            .join("\n");
        fs::write(temp.path().join("large.txt"), content).unwrap();
        let result = read_file(
            ReadParams {
                path: "large.txt".to_string(),
                offset: None,
                limit: None,
            },
            Some(temp.path()),
            CancellationToken::new(),
        )
        .await;
        assert_eq!(result.is_error, Some(false));
        assert!(text(&result).contains("Showing lines 1-2000"));
        assert!(text(&result).contains("Use offset=2001 to continue"));
    }

    #[tokio::test]
    async fn read_directs_images_to_read_image() {
        let temp = TestDir::new();
        fs::write(temp.path().join("pixel.png"), b"\x89PNG\r\n\x1a\nrest").unwrap();
        let result = read_file(
            ReadParams {
                path: "pixel.png".to_string(),
                offset: None,
                limit: None,
            },
            Some(temp.path()),
            CancellationToken::new(),
        )
        .await;
        assert_eq!(result.is_error, Some(false));
        assert!(text(&result).contains("Use read_image"));
    }

    #[tokio::test]
    async fn read_is_bounded_rejects_non_files_and_observes_cancellation() {
        let temp = TestDir::new();
        fs::write(
            temp.path().join("one-line.txt"),
            vec![b'a'; MAX_READ_BYTES + 1],
        )
        .unwrap();
        let bounded = read_file(
            ReadParams {
                path: "one-line.txt".to_string(),
                offset: None,
                limit: None,
            },
            Some(temp.path()),
            CancellationToken::new(),
        )
        .await;
        assert_eq!(bounded.is_error, Some(false));
        assert!(text(&bounded).contains("exceeds the 50KB read limit"));

        let directory = read_file(
            ReadParams {
                path: ".".to_string(),
                offset: None,
                limit: None,
            },
            Some(temp.path()),
            CancellationToken::new(),
        )
        .await;
        assert_eq!(directory.is_error, Some(true));
        assert!(text(&directory).contains("not a regular file"));

        let cancelled_token = CancellationToken::new();
        cancelled_token.cancel();
        let cancelled = read_file(
            ReadParams {
                path: "one-line.txt".to_string(),
                offset: None,
                limit: None,
            },
            Some(temp.path()),
            cancelled_token,
        )
        .await;
        assert_eq!(cancelled.is_error, Some(true));
        assert!(text(&cancelled).contains("cancelled"));
    }

    #[tokio::test]
    async fn read_has_no_phantom_line_after_a_trailing_newline() {
        let temp = TestDir::new();
        fs::write(
            temp.path().join("exact.txt"),
            "line\n".repeat(MAX_READ_LINES),
        )
        .unwrap();
        let result = read_file(
            ReadParams {
                path: "exact.txt".to_string(),
                offset: None,
                limit: None,
            },
            Some(temp.path()),
            CancellationToken::new(),
        )
        .await;
        assert_eq!(result.is_error, Some(false));
        assert!(!text(&result).contains("Use offset="));
        assert_eq!(text(&result).lines().count(), MAX_READ_LINES);
    }

    #[test]
    fn read_image_keeps_remote_urls_and_normalizes_local_paths() {
        let remote = object!({ "source": "https://example.com/pixel.png" });
        assert_eq!(
            normalize_read_image_arguments(Some(remote.clone()), Some(Path::new("/tmp"))),
            Some(remote)
        );

        let local = normalize_read_image_arguments(
            Some(object!({ "source": "images/pixel.png" })),
            Some(Path::new("/tmp/project")),
        )
        .unwrap();
        assert_eq!(local["source"], "/tmp/project/images/pixel.png");
    }

    #[tokio::test]
    async fn edit_validates_then_applies_multiple_replacements() {
        let temp = TestDir::new();
        let path = temp.path().join("notes.txt");
        fs::write(&path, "alpha\nbeta\ngamma\n").unwrap();
        let result = edit_file(
            EditParams {
                path: "notes.txt".to_string(),
                edits: vec![
                    Replacement {
                        old_text: "alpha".to_string(),
                        new_text: "first".to_string(),
                    },
                    Replacement {
                        old_text: "gamma".to_string(),
                        new_text: "third".to_string(),
                    },
                ],
            },
            Some(temp.path()),
            CancellationToken::new(),
        )
        .await;
        assert_eq!(result.is_error, Some(false));
        assert_eq!(fs::read_to_string(path).unwrap(), "first\nbeta\nthird\n");
    }

    #[tokio::test]
    async fn edit_rejects_non_unique_and_overlapping_matches_without_writing() {
        let temp = TestDir::new();
        let path = temp.path().join("notes.txt");
        let original = "alpha alpha beta";
        fs::write(&path, original).unwrap();

        let duplicate = edit_file(
            EditParams {
                path: "notes.txt".to_string(),
                edits: vec![Replacement {
                    old_text: "alpha".to_string(),
                    new_text: "first".to_string(),
                }],
            },
            Some(temp.path()),
            CancellationToken::new(),
        )
        .await;
        assert_eq!(duplicate.is_error, Some(true));
        assert_eq!(fs::read_to_string(&path).unwrap(), original);

        let overlap = edit_file(
            EditParams {
                path: "notes.txt".to_string(),
                edits: vec![
                    Replacement {
                        old_text: "alpha alpha".to_string(),
                        new_text: "first".to_string(),
                    },
                    Replacement {
                        old_text: "alpha beta".to_string(),
                        new_text: "second".to_string(),
                    },
                ],
            },
            Some(temp.path()),
            CancellationToken::new(),
        )
        .await;
        assert_eq!(overlap.is_error, Some(true));
        assert_eq!(fs::read_to_string(path).unwrap(), original);
    }

    #[tokio::test]
    async fn edit_preserves_bom_and_crlf() {
        let temp = TestDir::new();
        let path = temp.path().join("windows.txt");
        fs::write(&path, "\u{feff}alpha\r\nbeta\r\n").unwrap();
        let result = edit_file(
            EditParams {
                path: "windows.txt".to_string(),
                edits: vec![Replacement {
                    old_text: "alpha\nbeta".to_string(),
                    new_text: "first\nsecond".to_string(),
                }],
            },
            Some(temp.path()),
            CancellationToken::new(),
        )
        .await;
        assert_eq!(result.is_error, Some(false));
        assert_eq!(
            fs::read_to_string(path).unwrap(),
            "\u{feff}first\r\nsecond\r\n"
        );

        let classic_mac_path = temp.path().join("classic-mac.txt");
        fs::write(&classic_mac_path, "alpha\rbeta\r").unwrap();
        let classic_mac = edit_file(
            EditParams {
                path: "classic-mac.txt".to_string(),
                edits: vec![Replacement {
                    old_text: "alpha\nbeta".to_string(),
                    new_text: "first\nsecond".to_string(),
                }],
            },
            Some(temp.path()),
            CancellationToken::new(),
        )
        .await;
        assert_eq!(classic_mac.is_error, Some(false));
        assert_eq!(
            fs::read_to_string(classic_mac_path).unwrap(),
            "first\rsecond\r"
        );
    }

    #[tokio::test]
    async fn edit_accepts_stringified_edits_and_rejects_no_ops() {
        let parsed = MapleDeveloperClient::parse_args::<EditParams>(Some(object!({
            "path": "notes.txt",
            "edits": "[{\"oldText\":\"alpha\",\"newText\":\"beta\"}]"
        })))
        .unwrap();
        assert_eq!(parsed.edits.len(), 1);

        let temp = TestDir::new();
        fs::write(temp.path().join("notes.txt"), "alpha").unwrap();
        let no_op = edit_file(
            EditParams {
                path: "notes.txt".to_string(),
                edits: vec![Replacement {
                    old_text: "alpha".to_string(),
                    new_text: "alpha".to_string(),
                }],
            },
            Some(temp.path()),
            CancellationToken::new(),
        )
        .await;
        assert_eq!(no_op.is_error, Some(true));
        assert!(text(&no_op).contains("would not change"));
        assert_eq!(
            fs::read_to_string(temp.path().join("notes.txt")).unwrap(),
            "alpha"
        );
    }

    #[tokio::test]
    async fn write_creates_parents_overwrites_and_reports_utf8_bytes() {
        let temp = TestDir::new();
        let nested = temp.path().join("nested/notes.txt");
        let created = write_file(
            WriteParams {
                path: "nested/notes.txt".to_string(),
                content: "hé".to_string(),
            },
            Some(temp.path()),
            CancellationToken::new(),
        )
        .await;
        assert_eq!(created.is_error, Some(false));
        assert!(text(&created).contains("3 bytes"));
        assert_eq!(fs::read_to_string(&nested).unwrap(), "hé");

        let overwritten = write_file(
            WriteParams {
                path: "nested/notes.txt".to_string(),
                content: "replacement".to_string(),
            },
            Some(temp.path()),
            CancellationToken::new(),
        )
        .await;
        assert_eq!(overwritten.is_error, Some(false));
        assert_eq!(fs::read_to_string(nested).unwrap(), "replacement");
    }

    #[test]
    fn overlapping_match_detection_counts_overlaps() {
        assert_eq!(overlapping_match_positions("aaa", "aa"), [0, 1]);
    }
}
