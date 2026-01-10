fn main() {
    // iOS-specific build configuration for ONNX Runtime
    #[cfg(target_os = "ios")]
    {
        // Get the path to the ONNX Runtime xcframework
        let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
        let ort_dir = format!("{}/onnxruntime-ios", manifest_dir);
        
        // Check if building for simulator or device
        let target = std::env::var("TARGET").unwrap_or_default();
        let lib_path = if target.contains("sim") || target.contains("x86_64") {
            format!("{}/onnxruntime.xcframework/ios-arm64_x86_64-simulator", ort_dir)
        } else {
            format!("{}/onnxruntime.xcframework/ios-arm64", ort_dir)
        };
        
        // Tell cargo where to find the ONNX Runtime static library
        println!("cargo:rustc-link-search=native={}", lib_path);
        println!("cargo:rustc-link-lib=static=onnxruntime");
        
        // Link required iOS frameworks
        println!("cargo:rustc-link-lib=framework=Foundation");
        println!("cargo:rustc-link-lib=framework=Accelerate");
        
        // Set ORT_LIB_LOCATION for the ort crate
        println!("cargo:rustc-env=ORT_LIB_LOCATION={}", ort_dir);
        
        // Rerun if the onnxruntime directory changes
        println!("cargo:rerun-if-changed={}", ort_dir);
    }
    
    tauri_build::build()
}
