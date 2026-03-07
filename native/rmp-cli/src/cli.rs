use std::collections::BTreeMap;
use std::process::ExitCode;

use clap::{ArgAction, Parser, Subcommand, ValueEnum};
use serde::Serialize;

#[derive(Parser, Debug)]
#[command(
    name = "rmp",
    version,
    about = "RMP: Rust Multiplatform orchestrator (MVP)"
)]
pub struct Cli {
    /// Machine-readable output. When set, stdout is JSON only; logs go to stderr.
    #[arg(long, global = true)]
    pub json: bool,

    /// More logs to stderr.
    #[arg(long, global = true)]
    pub verbose: bool,

    #[command(subcommand)]
    pub cmd: Cmd,
}

#[derive(Subcommand, Debug)]
pub enum Cmd {
    /// Scaffold a new Rust + iOS + Android + desktop app repository.
    Init(InitArgs),

    /// Fast diagnostics; must not build.
    Doctor,

    /// Device discovery across iOS + Android.
    Devices {
        #[command(subcommand)]
        cmd: DevicesCmd,
    },

    /// Generate bindings and build platform artifacts.
    Bindings(BindingsArgs),

    /// Build/install/launch on iOS/Android/desktop (debug).
    Run(RunArgs),
}

#[derive(clap::Args, Debug)]
pub struct InitArgs {
    /// Project directory name to create.
    pub name: String,

    /// Include iOS app scaffolding.
    #[arg(long, conflicts_with = "no_ios")]
    pub ios: bool,

    /// Exclude iOS app scaffolding.
    #[arg(long = "no-ios", action = ArgAction::SetTrue)]
    pub no_ios: bool,

    /// Include Android app scaffolding.
    #[arg(long, conflicts_with = "no_android")]
    pub android: bool,

    /// Exclude Android app scaffolding.
    #[arg(long = "no-android", action = ArgAction::SetTrue)]
    pub no_android: bool,

    /// Include ICED desktop app scaffolding.
    #[arg(long, conflicts_with = "no_iced")]
    pub iced: bool,

    /// Exclude ICED desktop app scaffolding.
    #[arg(long = "no-iced", action = ArgAction::SetTrue)]
    pub no_iced: bool,

    /// Include a local flake.nix dev shell with an `rmp` wrapper.
    #[arg(long, conflicts_with = "no_flake")]
    pub flake: bool,

    /// Exclude flake.nix generation.
    #[arg(long = "no-flake", action = ArgAction::SetTrue)]
    pub no_flake: bool,

    /// Initialize a local git repo and stage scaffold files.
    #[arg(long, conflicts_with = "no_git")]
    pub git: bool,

    /// Skip local git repo initialization.
    #[arg(long = "no-git", action = ArgAction::SetTrue)]
    pub no_git: bool,

    /// Reverse-DNS org prefix (e.g. com.example).
    #[arg(long)]
    pub org: Option<String>,

    /// iOS bundle identifier override.
    #[arg(long)]
    pub bundle_id: Option<String>,

    /// Android application ID override.
    #[arg(long)]
    pub app_id: Option<String>,

    /// Non-interactive mode.
    #[arg(long)]
    pub yes: bool,
}

#[derive(Subcommand, Debug)]
pub enum DevicesCmd {
    /// List devices/simulators/emulators.
    List,

    /// Start a simulator/emulator target and wait until ready.
    Start(DevicesStartArgs),
}

#[derive(clap::Args, Debug)]
pub struct DevicesStartArgs {
    #[arg(value_enum)]
    pub platform: DeviceStartPlatform,

    #[command(flatten)]
    pub ios: DevicesStartIosArgs,

    #[command(flatten)]
    pub android: DevicesStartAndroidArgs,
}

#[derive(ValueEnum, Debug, Clone, Copy)]
pub enum DeviceStartPlatform {
    Ios,
    Android,
}

#[derive(clap::Args, Debug, Default)]
pub struct DevicesStartIosArgs {
    /// iOS simulator UDID to target.
    #[arg(long)]
    pub udid: Option<String>,
}

#[derive(clap::Args, Debug, Default)]
pub struct DevicesStartAndroidArgs {
    /// Android emulator serial to target.
    #[arg(long)]
    pub serial: Option<String>,

    /// Android AVD name to start if no emulator is running.
    #[arg(long)]
    pub avd: Option<String>,
}

#[derive(clap::Args, Debug)]
pub struct BindingsArgs {
    #[arg(value_enum)]
    pub target: BindingsTarget,

    /// Remove build outputs first.
    #[arg(long)]
    pub clean: bool,

    /// Fail if generated sources differ from what's in-tree; also require builds succeed.
    #[arg(long)]
    pub check: bool,
}

#[derive(ValueEnum, Debug, Clone, Copy)]
pub enum BindingsTarget {
    Swift,
    Kotlin,
    All,
}

#[derive(clap::Args, Debug)]
pub struct RunArgs {
    #[arg(value_enum)]
    pub platform: RunPlatform,

    /// Build Rust artifacts in release mode (default is debug for faster iteration).
    #[arg(long)]
    pub release: bool,

    #[command(flatten)]
    pub ios: RunIosArgs,

    #[command(flatten)]
    pub android: RunAndroidArgs,
}

#[derive(ValueEnum, Debug, Clone, Copy)]
pub enum RunPlatform {
    Ios,
    Android,
    Iced,
}

#[derive(clap::Args, Debug, Default)]
pub struct RunIosArgs {
    /// iOS simulator UDID to target.
    #[arg(long)]
    pub udid: Option<String>,
}

#[derive(clap::Args, Debug, Default)]
pub struct RunAndroidArgs {
    /// Android emulator serial to target.
    #[arg(long)]
    pub serial: Option<String>,

    /// Android AVD name to start if no emulator is running.
    #[arg(long)]
    pub avd: Option<String>,

    /// Optional host->device port reverses (e.g. "18080:32820,9090").
    #[arg(long)]
    pub adb_reverse: Option<String>,
}

#[derive(Serialize)]
pub struct JsonOk<T: Serialize> {
    pub ok: bool,
    #[serde(flatten)]
    pub data: T,
}

#[derive(Serialize)]
pub struct JsonErr {
    pub ok: bool,
    pub error: JsonErrInner,
}

#[derive(Serialize)]
pub struct JsonErrInner {
    pub message: String,
    pub exit_code: i32,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub choices: Vec<JsonChoice>,
    #[serde(skip_serializing_if = "BTreeMap::is_empty", default)]
    pub details: BTreeMap<String, serde_json::Value>,
}

#[derive(Serialize, Clone, Debug)]
pub struct JsonChoice {
    pub id: String,
    pub platform: String, // "ios" | "android" | "iced"
    pub kind: String,     // "device" | "simulator" | "emulator"
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub os: Option<String>,
}

#[derive(thiserror::Error, Debug)]
#[error("{message}")]
pub struct CliError {
    pub message: String,
    pub exit_code: i32,
    pub choices: Vec<JsonChoice>,
    pub details: BTreeMap<String, serde_json::Value>,
}

impl CliError {
    pub fn user(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            exit_code: 2,
            choices: vec![],
            details: BTreeMap::new(),
        }
    }

    pub fn operational(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            exit_code: 1,
            choices: vec![],
            details: BTreeMap::new(),
        }
    }

    #[allow(dead_code)]
    pub fn with_choices(mut self, choices: Vec<JsonChoice>) -> Self {
        self.choices = choices;
        self
    }

    #[allow(dead_code)]
    pub fn with_detail(mut self, k: &str, v: serde_json::Value) -> Self {
        self.details.insert(k.to_string(), v);
        self
    }
}

pub fn json_print<T: Serialize>(v: &T) {
    println!("{}", serde_json::to_string(v).expect("json serialize"));
}

pub fn human_log(verbose: bool, msg: impl AsRef<str>) {
    if verbose {
        eprintln!("{}", msg.as_ref());
    }
}

pub fn render_err(json: bool, e: CliError) -> ExitCode {
    if json {
        json_print(&JsonErr {
            ok: false,
            error: JsonErrInner {
                message: e.message,
                exit_code: e.exit_code,
                choices: e.choices,
                details: e.details,
            },
        });
        ExitCode::from(e.exit_code as u8)
    } else {
        eprintln!("error: {}", e.message);
        if !e.choices.is_empty() {
            eprintln!("choices:");
            for c in e.choices {
                eprintln!(
                    "  {} {}: {}{}",
                    c.platform,
                    c.kind,
                    c.id,
                    c.name.map(|n| format!("  {n}")).unwrap_or_default()
                );
            }
        }
        ExitCode::from(e.exit_code as u8)
    }
}

// Intentionally no shared "require_success" helper yet: behavior differs slightly across
// commands (some stream output, some capture).
