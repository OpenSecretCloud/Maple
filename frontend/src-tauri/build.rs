fn main() {
    let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    if target_os == "ios" {
        println!("cargo:rustc-link-lib=c++");
    }

    tauri_build::build();

    // The deep-link plugin's build.rs overwrites CFBundleURLTypes in Info.plist
    // based on the mobile config, stripping our custom URL scheme.
    // Re-add it after all plugin build scripts have run.
    if target_os == "ios" {
        ensure_ios_custom_url_scheme();
    }
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
