mod bindings;
mod cli;
mod config;
mod devices;
mod doctor;
mod init;
mod run;
mod util;

use std::path::PathBuf;
use std::process::ExitCode;

use clap::Parser;

fn main() -> ExitCode {
    let args = cli::Cli::parse();

    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let res: Result<(), cli::CliError> = match args.cmd {
        cli::Cmd::Init(i) => init::init(&cwd, args.json, args.verbose, i),
        cmd => {
            let root = match config::find_workspace_root(&cwd) {
                Ok(v) => v,
                Err(e) => return cli::render_err(args.json, e),
            };
            match cmd {
                cli::Cmd::Doctor => doctor::doctor(&root, args.json, args.verbose),
                cli::Cmd::Devices {
                    cmd: cli::DevicesCmd::List,
                } => devices::devices_list(&root, args.json, args.verbose),
                cli::Cmd::Devices {
                    cmd: cli::DevicesCmd::Start(s),
                } => devices::devices_start(&root, args.json, args.verbose, s),
                cli::Cmd::Bindings(b) => bindings::bindings(&root, args.json, args.verbose, b),
                cli::Cmd::Run(r) => run::run(&root, args.json, args.verbose, r),
                cli::Cmd::Init(_) => unreachable!(),
            }
        }
    };

    match res {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => cli::render_err(args.json, e),
    }
}
