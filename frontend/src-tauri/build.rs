fn main() {
    let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    let target_arch = std::env::var("CARGO_CFG_TARGET_ARCH").unwrap_or_default();
    if target_os == "ios" {
        println!("cargo:rustc-link-lib=c++");
    }

    if target_os != "ios" && target_os != "android" {
        stage_goose_binary(&target_os, &target_arch);
    }

    tauri_build::build();

    // The deep-link plugin's build.rs overwrites CFBundleURLTypes in Info.plist
    // based on the mobile config, stripping our custom URL scheme.
    // Re-add it after all plugin build scripts have run.
    if target_os == "ios" {
        ensure_ios_custom_url_scheme();
    }
}

fn stage_goose_binary(target_os: &str, target_arch: &str) {
    println!("cargo:rerun-if-env-changed=MAPLE_GOOSE_BINARY");
    println!("cargo:rerun-if-env-changed=MAPLE_SKIP_STAGE_GOOSE_BINARY");

    if std::env::var("MAPLE_SKIP_STAGE_GOOSE_BINARY").as_deref() == Ok("1") {
        return;
    }

    let binary_name = if target_os == "windows" {
        "goose.exe"
    } else {
        "goose"
    };
    let target_dir = std::path::Path::new("bin");
    if let Err(error) = std::fs::create_dir_all(target_dir) {
        println!("cargo:warning=failed to create Goose staging dir: {error}");
        return;
    }

    let destination = target_dir.join(binary_name);
    let source = std::env::var("MAPLE_GOOSE_BINARY")
        .ok()
        .map(std::path::PathBuf::from)
        .filter(|path| path.is_file())
        .or_else(|| goose_binary_package_source(target_os, target_arch, binary_name));

    let Some(source) = source else {
        println!(
            "cargo:warning=Goose binary not staged; install @aaif/goose-sdk optional binary package or set MAPLE_GOOSE_BINARY"
        );
        return;
    };

    println!("cargo:rerun-if-changed={}", source.display());
    if staged_binary_matches(&source, &destination) {
        ensure_executable(&destination);
        println!(
            "cargo:warning=Goose binary already staged from {}",
            source.display()
        );
        return;
    }

    match std::fs::copy(&source, &destination) {
        Ok(_) => {
            ensure_executable(&destination);
            println!(
                "cargo:warning=staged Goose binary from {}",
                source.display()
            );
        }
        Err(error) => {
            println!(
                "cargo:warning=failed to stage Goose binary from {}: {error}",
                source.display()
            );
        }
    }
}

fn staged_binary_matches(source: &std::path::Path, destination: &std::path::Path) -> bool {
    match (std::fs::metadata(source), std::fs::metadata(destination)) {
        (Ok(source_meta), Ok(destination_meta)) => source_meta.len() == destination_meta.len(),
        _ => false,
    }
}

fn ensure_executable(path: &std::path::Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755));
    }

    #[cfg(not(unix))]
    {
        let _ = path;
    }
}

fn goose_binary_package_source(
    target_os: &str,
    target_arch: &str,
    binary_name: &str,
) -> Option<std::path::PathBuf> {
    let package = match (target_os, target_arch) {
        ("macos", "aarch64") => "goose-binary-darwin-arm64",
        ("macos", "x86_64") => "goose-binary-darwin-x64",
        ("linux", "aarch64") => "goose-binary-linux-arm64",
        ("linux", "x86_64") => "goose-binary-linux-x64",
        ("windows", "x86_64") => "goose-binary-win32-x64",
        _ => return None,
    };

    let candidate = std::path::Path::new("..")
        .join("node_modules")
        .join("@aaif")
        .join(package)
        .join("bin")
        .join(binary_name);
    candidate.is_file().then_some(candidate)
}

#[allow(dead_code)]
fn ensure_ios_custom_url_scheme() {
    let plist_path = std::path::Path::new("gen/apple/maple_iOS/Info.plist");
    if !plist_path.exists() {
        return;
    }

    let mut plist: plist::Value = plist::from_file(plist_path).expect("failed to read Info.plist");
    let dict = plist
        .as_dictionary_mut()
        .expect("Info.plist is not a dictionary");

    let scheme = "cloud.opensecret.maple";

    let has_scheme = dict
        .get("CFBundleURLTypes")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter().any(|entry| {
                entry
                    .as_dictionary()
                    .and_then(|d| d.get("CFBundleURLSchemes"))
                    .and_then(|v| v.as_array())
                    .map(|schemes| schemes.iter().any(|s| s.as_string() == Some(scheme)))
                    .unwrap_or(false)
            })
        })
        .unwrap_or(false);

    if !has_scheme {
        let mut url_type = plist::Dictionary::new();
        url_type.insert(
            "CFBundleURLSchemes".into(),
            vec![plist::Value::String(scheme.to_string())].into(),
        );
        url_type.insert(
            "CFBundleURLName".into(),
            plist::Value::String(scheme.to_string()),
        );

        if !dict.contains_key("CFBundleURLTypes") {
            dict.insert("CFBundleURLTypes".into(), plist::Value::Array(vec![]));
        }

        if let Some(arr) = dict
            .get_mut("CFBundleURLTypes")
            .and_then(|v| v.as_array_mut())
        {
            arr.push(plist::Value::Dictionary(url_type));
        }

        plist::to_file_xml(plist_path, &plist).expect("failed to write Info.plist");
    }
}
