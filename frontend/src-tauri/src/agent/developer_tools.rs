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
use serde::Deserialize;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex as StdMutex, Weak};
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

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
    edits: Vec<Replacement>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct WriteParams {
    path: String,
    content: String,
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
            "Apply one or more exact, unique text replacements to a file atomically. Every oldText is matched against the original file; overlapping edits are rejected."
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
            tools.push(read_image);
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
                Ok(params) => read_file(params, working_dir),
                Err(error) => error_result(error),
            },
            "edit" => match Self::parse_args::<EditParams>(arguments) {
                Ok(params) => edit_file(params, working_dir).await,
                Err(error) => error_result(error),
            },
            "write" => match Self::parse_args::<WriteParams>(arguments) {
                Ok(params) => write_file(params, working_dir).await,
                Err(error) => error_result(error),
            },
            "shell" | "read_image" => {
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
    } else if let Some(relative) = path.strip_prefix("~/") {
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

fn read_file(params: ReadParams, working_dir: Option<&Path>) -> CallToolResult {
    if params.offset == Some(0) {
        return error_result("offset must be at least 1");
    }
    if params.limit == Some(0) {
        return error_result("limit must be at least 1");
    }

    let path = resolve_path(&params.path, working_dir);
    let bytes = match fs::read(&path) {
        Ok(bytes) => bytes,
        Err(error) => return error_result(format!("Failed to read {}: {error}", params.path)),
    };

    if is_supported_image(&bytes) {
        return success_result(format!(
            "{} is an image. Use read_image to inspect it.",
            params.path
        ));
    }

    let content = String::from_utf8_lossy(&bytes);
    let all_lines = content.split('\n').collect::<Vec<_>>();
    let start = params.offset.unwrap_or(1) - 1;
    if start >= all_lines.len() {
        return error_result(format!(
            "Offset {} is beyond end of file ({} lines total)",
            params.offset.unwrap_or(1),
            all_lines.len()
        ));
    }

    let requested_end = params
        .limit
        .map(|limit| start.saturating_add(limit))
        .unwrap_or(all_lines.len())
        .min(all_lines.len());
    let selected = &all_lines[start..requested_end];

    let mut output_lines = Vec::new();
    let mut output_bytes = 0usize;
    let mut truncated = false;
    for line in selected {
        if output_lines.len() == MAX_READ_LINES {
            truncated = true;
            break;
        }
        let separator_bytes = usize::from(!output_lines.is_empty());
        let line_bytes = line.len();
        if output_bytes + separator_bytes + line_bytes > MAX_READ_BYTES {
            if output_lines.is_empty() {
                return success_result(format!(
                    "[Line {} exceeds the {}KB read limit. Use shell with a byte-limiting command to inspect it.]",
                    start + 1,
                    MAX_READ_BYTES / 1024
                ));
            }
            truncated = true;
            break;
        }
        output_lines.push(*line);
        output_bytes += separator_bytes + line_bytes;
    }

    let mut output = output_lines.join("\n");
    let consumed_end = start + output_lines.len();
    if truncated {
        let first_line = start + 1;
        let last_line = consumed_end;
        let next_offset = consumed_end + 1;
        output.push_str(&format!(
            "\n\n[Showing lines {first_line}-{last_line} of {}. Use offset={next_offset} to continue.]",
            all_lines.len()
        ));
    } else if requested_end < all_lines.len() {
        let remaining = all_lines.len() - requested_end;
        output.push_str(&format!(
            "\n\n[{remaining} more lines in file. Use offset={} to continue.]",
            requested_end + 1
        ));
    }

    success_result(output)
}

fn is_supported_image(bytes: &[u8]) -> bool {
    bytes.starts_with(b"\x89PNG\r\n\x1a\n")
        || bytes.starts_with(&[0xff, 0xd8, 0xff])
        || bytes.starts_with(b"GIF87a")
        || bytes.starts_with(b"GIF89a")
        || (bytes.len() >= 12 && bytes.starts_with(b"RIFF") && &bytes[8..12] == b"WEBP")
}

async fn write_file(params: WriteParams, working_dir: Option<&Path>) -> CallToolResult {
    let path = resolve_path(&params.path, working_dir);
    let lock = mutation_lock(&path);
    let _guard = lock.lock().await;

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

async fn edit_file(params: EditParams, working_dir: Option<&Path>) -> CallToolResult {
    if params.edits.is_empty() {
        return error_result("edits must contain at least one replacement");
    }

    let path = resolve_path(&params.path, working_dir);
    let lock = mutation_lock(&path);
    let _guard = lock.lock().await;
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
            [start] => resolved_edits.push((*start, *start + old_text.len(), new_text)),
            _ => {
                return error_result(format!(
                    "edits[{index}].oldText matched {} times; include more context so it is unique",
                    matches.len()
                ));
            }
        }
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
}

fn normalize_text_file(content: &str) -> (bool, LineEnding, String) {
    let (bom, content) = match content.strip_prefix('\u{feff}') {
        Some(content) => (true, content),
        None => (false, content),
    };
    let line_ending = if content.contains("\r\n") {
        LineEnding::CrLf
    } else {
        LineEnding::Lf
    };
    (bom, line_ending, normalize_newlines(content))
}

fn normalize_newlines(content: &str) -> String {
    content.replace("\r\n", "\n")
}

fn restore_text_file(content: &str, bom: bool, line_ending: LineEnding) -> String {
    let content = match line_ending {
        LineEnding::Lf => content.to_string(),
        LineEnding::CrLf => content.replace('\n', "\r\n"),
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
        assert_eq!(read_image["annotations"]["readOnlyHint"], true);
        assert_eq!(
            result.tools[2].input_schema["properties"]["edits"]["minItems"],
            1
        );
    }

    #[test]
    fn read_supports_offsets_limits_and_continuation() {
        let temp = TestDir::new();
        fs::write(temp.path().join("notes.txt"), "one\ntwo\nthree\nfour").unwrap();
        let result = read_file(
            ReadParams {
                path: "notes.txt".to_string(),
                offset: Some(2),
                limit: Some(2),
            },
            Some(temp.path()),
        );
        assert_eq!(result.is_error, Some(false));
        assert_eq!(
            text(&result),
            "two\nthree\n\n[1 more lines in file. Use offset=4 to continue.]"
        );
    }

    #[test]
    fn read_rejects_offsets_past_eof() {
        let temp = TestDir::new();
        fs::write(temp.path().join("notes.txt"), "one\ntwo").unwrap();
        let result = read_file(
            ReadParams {
                path: "notes.txt".to_string(),
                offset: Some(3),
                limit: None,
            },
            Some(temp.path()),
        );
        assert_eq!(result.is_error, Some(true));
        assert!(text(&result).contains("beyond end of file"));
    }

    #[test]
    fn read_truncates_on_complete_lines_with_next_offset() {
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
        );
        assert_eq!(result.is_error, Some(false));
        assert!(text(&result).contains("Showing lines 1-2000 of 2001"));
        assert!(text(&result).contains("Use offset=2001 to continue"));
    }

    #[test]
    fn read_directs_images_to_read_image() {
        let temp = TestDir::new();
        fs::write(temp.path().join("pixel.png"), b"\x89PNG\r\n\x1a\nrest").unwrap();
        let result = read_file(
            ReadParams {
                path: "pixel.png".to_string(),
                offset: None,
                limit: None,
            },
            Some(temp.path()),
        );
        assert_eq!(result.is_error, Some(false));
        assert!(text(&result).contains("Use read_image"));
    }

    #[tokio::test]
    async fn edit_applies_multiple_replacements_atomically() {
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
        )
        .await;
        assert_eq!(result.is_error, Some(false));
        assert_eq!(
            fs::read_to_string(path).unwrap(),
            "\u{feff}first\r\nsecond\r\n"
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
