//! CI compatibility gate using the installer's actual archive parser.
//! It inspects package data and never extracts or executes an application.

use std::{fs::File, io::Read, path::Path};

use maple_agent_updater::installer::validate_archive;
use sha2::{Digest, Sha256};

const MAX_ARCHIVE_BYTES: u64 = 4 * 1024 * 1024 * 1024;

fn digest(path: &Path) -> anyhow::Result<(String, u64)> {
    anyhow::ensure!(
        std::fs::symlink_metadata(path)?.is_file(),
        "archive must be a regular file"
    );
    let source = File::open(path)?;
    anyhow::ensure!(
        source.metadata()?.len() <= MAX_ARCHIVE_BYTES,
        "archive is too large"
    );
    let mut source = source.take(MAX_ARCHIVE_BYTES + 1);
    let mut hash = Sha256::new();
    let mut size = 0_u64;
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let length = source.read(&mut buffer)?;
        if length == 0 {
            break;
        }
        size += length as u64;
        anyhow::ensure!(size <= MAX_ARCHIVE_BYTES, "archive is too large");
        hash.update(&buffer[..length]);
    }
    anyhow::ensure!(size > 0, "archive is empty");
    Ok((hex::encode(hash.finalize()), size))
}

fn main() -> anyhow::Result<()> {
    let arguments: Vec<_> = std::env::args_os().skip(1).collect();
    anyhow::ensure!(
        arguments.len() == 2,
        "usage: validate-release-archive ARCHIVE EXPECTED_SHA256"
    );
    let path = Path::new(&arguments[0]);
    let expected = arguments[1]
        .to_str()
        .ok_or_else(|| anyhow::anyhow!("digest must be ASCII"))?;
    anyhow::ensure!(
        expected.len() == 64
            && expected
                .bytes()
                .all(|value| value.is_ascii_digit() || (b'a'..=b'f').contains(&value)),
        "digest must be lowercase SHA-256"
    );
    let before = digest(path)?;
    anyhow::ensure!(
        before.0 == expected,
        "archive does not match the package checksum"
    );
    validate_archive(path)?;
    anyhow::ensure!(digest(path)? == before, "archive changed during validation");
    println!(
        "{}",
        serde_json::json!({"archive": path, "sha256": before.0, "size": before.1, "installer_archive_scan": "passed"})
    );
    Ok(())
}
