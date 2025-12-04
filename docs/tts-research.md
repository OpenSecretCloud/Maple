# TTS Research

## Gemini

Definitive Comparative Analysis of Local TTS Architectures for Cross-Platform Tauri Integration: Supertonic 2 vs. Chatterbox Turbo1. Executive Strategic Overview: The Local Inference Convergence in 2026The trajectory of computational linguistics and speech synthesis has undergone a profound transformation over the last half-decade, culminating in a distinct bifurcation of technology stacks in early 2026. For systems architects and developers leveraging the Tauri framework to build cross-platform applications—spanning the unrestricted desktop ecosystems of macOS and Linux, as well as the rigorously sandboxed mobile environments of iOS and Android—the selection of a Text-to-Speech (TTS) engine is no longer a mere feature choice. It has elevated to a fundamental architectural decision that dictates the entire build pipeline, runtime efficiency, and distribution strategy of the final application.The user’s query posits a choice between two leading contenders in the open-weight arena: Supertonic 2, released by Supertone Inc. in January 2026, and Chatterbox Turbo, developed by Resemble AI. This report provides an exhaustive technical due diligence of these two models. The core tension explored herein is between Architectural Agility—epitomized by Supertonic’s lightweight, ONNX-native design—and Expressive Density—represented by Chatterbox’s larger, Llama-based backbone.While cloud-based inference dominated the early 2020s, the current paradigm emphasizes "Edge AI" and "Local First" principles. This shift is driven by privacy mandates, the need for zero-latency interaction in conversational interfaces, and the desire to eliminate recurring API costs. However, achieving parity with cloud-grade TTS on consumer hardware requires navigating a labyrinth of constraints: binary size limitations, memory bandwidth bottlenecks on mobile SoCs (System on Chips), and the draconian process management restrictions of mobile operating systems.For a Tauri developer, who enjoys the luxury of Rust’s performance and the web’s ubiquity, the challenge is uniquely complex. Tauri’s promise of a "write once, deploy everywhere" codebase is severely tested when integrating deep learning models that rely on disparate runtimes. Supertonic 2 offers a path of least resistance through native compilation, while Chatterbox Turbo demands a hybrid architecture that may fracture the unified codebase ideal. This report rigorously dissects these trade-offs to provide a definitive integration roadmap.2. Architectural Deconstruction: The Lightweight vs. The Large Language BackboneTo understand the feasibility of these models within a constrained Tauri environment, one must first dismantle their internal architectures. The "black box" of AI often obscures dependency chains that can shatter a cross-platform build pipeline. The difference between 44 million parameters and 350 million parameters is not merely quantitative; it represents two divergent philosophies of engineering.2.1 Supertonic 2: The Principles of Architectural DistillationSupertonic 2, as of its January 2026 release 1, is an anomaly in the contemporary landscape of generative AI. While the broader industry trend has been to scale parameters upwards—moving from millions to billions to achieve nuanced reasoning—Supertone Inc. has focused on distillation and efficiency. The model is engineered explicitly for embedded and on-device usage, prioritizing the reduction of computational overhead to near-negligible levels.The 44 Million Parameter AdvantageThe model operates with approximately 44 million parameters.2 In the context of modern neural networks, where even "Small Language Models" (SLMs) typically range from 0.5B to 3B parameters, 44M is microscopic. This scale confers specific hardware advantages that are critical for mobile performance:Cache Residency: A model of this size (approx. 268 MB in FP32, significantly less if quantized) can often reside entirely within the System Level Cache (SLC) or high-speed RAM partitions of modern mobile processors like the Apple A-series or Qualcomm Snapdragon. This drastically minimizes memory bandwidth saturation, which is the primary source of heat and battery drain during inference.Initialization Speed: The "cold start" time—the duration from loading the model to the first audio sample—is imperceptible, measured in milliseconds. This allows the TTS engine to be instantiated on-demand rather than requiring a persistent background service, optimizing system resource usage.The ONNX-Native RuntimeCrucially for Tauri developers, Supertonic is built natively for the ONNX Runtime.1 This choice is not incidental; it is a strategic enablement of cross-platform portability. ONNX (Open Neural Network Exchange) provides a standardized inference engine that is completely decoupled from the training environment. It does not require a Python interpreter, the heavy PyTorch library, or complex CUDA drivers to execute. Instead, it runs via optimized C++ libraries.Because Tauri's backend is written in Rust, developers can utilize the ort crate to bind directly to these C++ libraries. This means the TTS engine is not an external dependency or a separate process; it becomes an intrinsic function within the application's binary. This "library-level" integration is the gold standard for mobile development, ensuring compliance with App Store policies regarding executable code and utilizing native platform capabilities.The January 2026 Evolution (v2)The user's query specifically highlights "Supertonic 2 (Jan 2026)." This version introduces pivotal upgrades that address previous limitations:Multilingual Unification: Prior versions were often language-specific. Supertonic 2 introduces a unified architecture supporting English, Korean, Spanish, Portuguese, and French.1 This implies that a single ONNX model file can handle dynamic language switching at runtime without the latency penalty of unloading and reloading different model weights.Voice Personas: The update adds distinct voice styles (e.g., Alex, Sarah, James).5 While not offering the infinite flexibility of voice cloning, these preset personas cover the vast majority of use cases for standard reading applications, navigation, and accessibility tools.2.2 Chatterbox Turbo: The Llama-Based HeavyweightChatterbox, developed by Resemble AI, represents the "Quality First" school of thought. It leverages the massive advancements in Large Language Models (LLMs) and generative flow matching to achieve state-of-the-art naturalness.The Llama BackboneChatterbox Turbo is built upon a Llama backbone 6, likely adapting the transformer architecture to process audio tokens alongside text. Even in its "Turbo" configuration, which is optimized for latency, the model retains a 350 million parameter structure. While efficient for a server-grade GPU, this is nearly an order of magnitude larger than Supertonic.Memory Pressure: The model weights alone exceed 4 GB.7 Loading a 4GB model into memory is a non-trivial operation on mobile devices. Most mid-range Android phones ship with 6GB or 8GB of total RAM, shared between the OS, the GPU, and all active apps. Allocating 4GB to a single background TTS process will almost certainly trigger the operating system's Low Memory Killer (LMK), terminating the application or other background services to preserve system stability.Storage Friction: Distributing a mobile application with a 4GB asset payload is highly problematic. It exceeds the initial download size limits of both the Apple App Store (which requires Over-the-Air downloads to be under a certain threshold, often 200MB-4GB depending on OS version) and the Google Play Store (150MB base limit). Developers would be forced to implement complex "On-Demand Resource" downloading or expansive expansion files (OBB), adding significant friction to the user's first-run experience.The Python-PyTorch Dependency ChainChatterbox is a PyTorch-native model.6 Its architecture utilizes complex operations—specifically paralinguistic tag handling and flow matching decoders—that are deeply entwined with the PyTorch runtime and the Python ecosystem (requiring libraries like numpy, scipy, and torchaudio).Lack of ONNX Export: Unlike simpler models, Chatterbox does not offer a first-party, fully functional ONNX export that retains all its features. The dynamic nature of its flow matching steps and custom tokenizers makes "freezing" the model into a static computation graph exceptionally difficult. Consequently, running Chatterbox requires a live Python environment, a requirement that introduces the "Sidecar Problem" on mobile platforms—a critical hurdle for Tauri integration that will be explored in depth in subsequent sections.Feature SuperiorityDespite these architectural weights, Chatterbox offers capabilities Supertonic cannot match:Paralinguistic Control: Developers can inject tags like [laugh], [sigh], or [cough] directly into the text stream.6 The model understands these non-verbal cues and generates appropriate audio artifacts, creating a level of "human" performance that is SOTA.Zero-Shot Cloning: The model can clone a target voice from a mere 5-second reference clip.9 This feature relies on the dense vector representations of the Llama backbone to capture and replicate timbre and prosody instantly.3. The Tauri Framework Context: Integration RealitiesThe user's choice of Tauri as the application framework is the defining constraint of this analysis. Tauri operates on a unique architecture distinct from Electron or Native development. A Tauri app consists of two distinct layers:The Core (Backend): Written in Rust. This layer handles system interactions, file I/O, and heavy computation. It compiles down to a native binary.The Webview (Frontend): Written in web technologies (HTML/JS/CSS). This layer handles the UI and communicates with the Core via an asynchronous IPC bridge.For a TTS engine to be "local," it must reside within or be managed by the Rust Core. The feasibility of this integration varies wildly between Desktop (macOS/Linux) and Mobile (iOS/Android).3.1 The "Sidecar Pattern" and Desktop SuccessOn desktop operating systems, Tauri supports a feature known as the Sidecar Pattern. This allows the Rust Core to bundle and spawn external binaries as subprocesses.Mechanism: The developer compiles a Python script (and its interpreter) into a standalone executable using tools like PyInstaller or Nuitka. The Rust Core then uses the Command::sidecar API to launch this executable. Communication occurs via stdin (sending text) and stdout (receiving audio data).Implication for Chatterbox: This pattern makes running Chatterbox on macOS and Linux entirely feasible. The massive Python dependency chain is encapsulated in the sidecar binary. While the installer size bloats to 4GB+, the application runs successfully.Implication for Supertonic: While Supertonic can be run this way (e.g., using a Python wrapper around ONNX Runtime), it is unnecessary. Supertonic's C++ roots allow it to be linked directly into the Rust Core, avoiding the IPC overhead of a sidecar.3.2 The "Mobile Wall": Why Sidecars Fail on iOS & AndroidThe user's requirement for iOS and Android support reveals the fundamental weakness of the Chatterbox architecture in a Tauri context. The "Sidecar Pattern" described above is functionally non-existent on mobile platforms due to strict OS security models.iOS Sandbox ConstraintsApple's iOS enforces a draconian sandbox. An application bundle cannot contain arbitrary executables that are spawned as independent processes. The fork() and exec() system calls—essential for spawning a sidecar—are restricted or forbidden for App Store applications.Furthermore, iOS prohibits Just-In-Time (JIT) compilation for most applications (exceptions exist for browser engines and debuggers, but not general apps). PyTorch and complex Python runtimes heavily rely on JIT for performance. Running them in "interpreter-only" mode results in a catastrophic performance degradation, rendering a 350M parameter model unusable.Android Sandbox ConstraintsAndroid's security model, while slightly more flexible regarding JIT, imposes similar restrictions on subprocesses. While it is theoretically possible to package a Python binary and execute it via the NDK, managing the lifecycle of that process, ensuring it isn't killed by the stringent Android memory manager, and handling the communication bridge is a task of immense complexity. It fights against the grain of the Android application lifecycle.The Dependency Hell of Embedded PythonThe alternative to a sidecar is embedding the Python interpreter directly into the Rust binary (using crates like pyo3). This allows Python code to run within the main application process, bypassing the subprocess restriction.However, this leads to "Dependency Hell." To run Chatterbox, one must embed not just Python, but numpy, scipy, and torch. These are not pure Python libraries; they are wrappers around massive C/C++ and Fortran codebases. Compiling scipy or torch from source for aarch64-linux-android or aarch64-apple-ios and linking them statically into a Rust binary is one of the most notoriously difficult tasks in cross-platform development. It involves resolving thousands of symbol conflicts, matching libc versions, and dealing with build system incompatibilities. For 99% of development teams, this is a non-starter.4. Platform-Specific Integration Analysis: Mobile Deep DiveGiven that Mobile is the "Great Filter" in this selection process, we must analyze the integration pathway for the surviving candidate—Supertonic—and the theoretical (but painful) path for Chatterbox.4.1 Supertonic 2 on Mobile: The Native RouteSupertonic's reliance on ONNX Runtime (ORT) is its superpower here. ORT is designed with mobile in mind.iOS Integration StrategyStatic Linking: The ORT library is distributed as an .xcframework. In Rust, the ort crate can be configured to link against this framework during the build process (cargo build --target aarch64-apple-ios).CoreML Acceleration: iOS devices feature the Apple Neural Engine (ANE). ONNX Runtime supports the CoreML Execution Provider. By enabling this provider in the Rust ort session options, Supertonic inference is offloaded from the CPU to the NPU. This results in faster generation and, critically, drastically lower battery consumption.Asset Management: The 268MB .onnx file is treated as a standard bundle resource. It is accessible to the Rust Core via the NSBundle API (wrapped by Tauri's resource path helpers).Android Integration StrategyJNI and Shared Libraries: Android requires native libraries to be .so files. The ort crate manages the inclusion of libonnxruntime.so into the jniLibs folder of the Android project structure generated by Tauri.NNAPI Acceleration: Similar to CoreML, Android offers the Neural Networks API (NNAPI). Supertonic can leverage this to run on the DSP or NPU of Qualcomm or MediaTek chips, ensuring performance across the fragmented Android hardware ecosystem.App Bundle Size: While 268MB exceeds the 150MB base APK limit, Tauri developers can utilize "Play Asset Delivery" (install-time delivery) to package the model. Since the model is a static file, this is a solved infrastructure problem.4.2 Chatterbox on Mobile: The Remote FallbackSince running Chatterbox locally on mobile is effectively blocked by the OS constraints discussed in Section 3.2, the only viable architecture for a Tauri app wanting to use Chatterbox is a Hybrid Approach.Desktop Users: Enjoy local inference via the Python Sidecar.Mobile Users: The app detects the platform and routes TTS requests to a remote API (hosted by the developer) running the Chatterbox engine.The Cost: This violates the user's "local" requirement. It introduces latency, server costs (GPU hosting for inference), and privacy concerns (data leaving the device). However, it is the only way to access Chatterbox's features on a phone.5. Performance and Resource Profiling: The Cost of QualityPerformance is the secondary selector after compatibility. The user's query mentions "architecture differences," and nowhere is this more visible than in the computational cost of running the models.5.1 Real-Time Factor (RTF) BenchmarksThe "Real-Time Factor" measures the speed of generation. RTF = Processing Time / Audio Duration. An RTF of 0.1 means generating 10 seconds of audio takes 1 second. Lower is better.Supertonic 2 PerformanceDesktop (M4 Pro): Benchmarks indicate an RTF of 0.006.10 This is ~166x faster than real-time. For the user, this means the audio starts playing instantly, with zero perceived latency.Mobile (A17 Pro / Snapdragon 8 Gen 3): Even on mobile silicon, the 44M parameter model flies. Estimations based on similar SLMs suggest an RTF of 0.01 - 0.05 when using NPU acceleration. This enables "streaming" capabilities where long paragraphs are synthesized faster than the user can read them.Chatterbox Turbo PerformanceDesktop (RTX 4090): The model is fast, achieving sub-0.1 RTF.Mobile CPU (Theoretical): If one could run it on a mobile CPU (bypassing the build issues), the 350M parameters would crush the processor. Without heavy quantization (e.g., 4-bit) and optimization, RTF would likely hover between 0.5 and 1.0. This means a 10-second sentence could take 5-10 seconds to generate, creating awkward pauses in conversation or UI interaction.5.2 Memory Footprint & System StabilitySupertonic: Requires ~300-500 MB of RAM. This is safe for almost all modern mobile devices, even low-end Android phones with 4GB RAM. It leaves plenty of room for the OS and the webview.Chatterbox: Requires ~4-5 GB of RAM/VRAM. On a PC, this is fine. On a mobile device, this is catastrophic. iOS aggressively kills background processes that consume excessive memory. An app attempting to allocate 4GB for TTS would likely be terminated immediately upon initialization on all but the most expensive "Pro" model iPhones and Android flagships.6. Technical Integration Guide: Supertonic 2 (Recommended)Based on the evidence, Supertonic 2 is the only viable candidate for a truly local, cross-platform Tauri application. This section details the integration roadmap.6.1 Rust Core ConfigurationThe integration avoids the sidecar pattern entirely. We utilize the ort crate to bind to ONNX Runtime directly within the Rust process.Step 1: Dependency ManagementIn src-tauri/Cargo.toml:Ini, TOML[dependencies]
tauri = { version = "2.0", features = }
# ORT: The interface to ONNX Runtime. 
# 'fetch-models' allows auto-downloading libs (mostly for dev).
# 'load-dynamic-lib' is crucial for mobile linking.
ort = { version = "2.0", features = ["fetch-models", "load-dynamic-lib", "ndarray"] }
# Rodio: For cross-platform audio playback
rodio = "0.19"
Step 2: Model Asset BundlingThe 268MB model file must be accessible to the binary at runtime.Place supertonic-v2.onnx and config.json in src-tauri/assets/.Update tauri.conf.json to include these assets:JSON"bundle": {
  "resources": ["assets/*"]
}
Step 3: The Inference Engine (Rust)In src-tauri/src/lib.rs, implement a command that the frontend can invoke. This command should:Tokenize: Convert the input string into the specific integer tokens expected by Supertonic. (Note: Check if Supertonic v2 includes a fused tokenizer in the ONNX graph; if not, a small Rust-based tokenizer matching the training data is required).Inference: Pass the tokens to the ort session.Rust// Conceptual Rust Code
let inputs = ort::inputs!["input_ids" => token_tensor]?;
let outputs = session.run(inputs)?;
let audio_data = outputs["audio"].extract_tensor::<f32>()?;
Playback: Feed the audio_data into a rodio Sink for immediate playback.6.2 Mobile-Specific Build FlagsAndroid: You must ensure the correct jniLibs are present. You can often rely on the ort crate's build script, but for production, manually downloading the onnxruntime-android AAR and extracting the .so files to your project's android/app/src/main/jniLibs is the most robust method.iOS: You must link the onnxruntime.xcframework. In your build.rs, you may need to emit linker flags:Rustprintln!("cargo:rustc-link-lib=framework=onnxruntime");
7. Technical Integration Guide: Chatterbox (The Desktop-Only Hybrid)For completeness, if the project demands Chatterbox's features, here is the implementation strategy. Note that this abandons local mobile inference.7.1 Desktop: The Python SidecarEnvironment Isolation: Create a standalone Python environment using uv or conda. Install chatterbox-tts and its heavy dependencies (torch).Freezing the Binary: Use PyInstaller to compile a server.py script into a single binary. This script should launch a local web server (e.g., FastAPI) to listen for TTS requests.Warning: The resulting binary will be 4GB+.Tauri Orchestration:Add the binary to externalBin in tauri.conf.json.On app launch, spawn it via Command::sidecar.Wait for the "ready" signal (monitor stdout).Send HTTP requests to localhost for generation.7.2 Mobile: The Remote API FallbackSince the sidecar cannot run on iOS/Android:Host a Server: Deploy the Chatterbox model to a cloud GPU provider (e.g., RunPod, Lambda Labs, or AWS).Conditional Logic: In your frontend JavaScript:JavaScriptimport { type } from '@tauri-apps/plugin-os';

async function generateSpeech(text) {
  if (type() === 'android' |

| type() === 'ios') {// Call Remote APIreturn await fetch('https://api.myapp.com/tts', { body: { text } });} else {// Call Local Sidecarreturn await fetch('http://localhost:8000/tts', { body: { text } });}}```8. Quality of Experience (QoE) AnalysisBeyond the binary "can it run" question lies the "how does it sound" question.8.1 Prosody and StabilitySupertonic 2: The model produces highly stable, intelligible speech. The prosody is consistent, making it ideal for reading long-form content (articles, ebooks). It rarely "hallucinates" or creates bizarre artifacts, a common trait of distilled models. However, it can sound "flatter" or less dynamic than larger models.Chatterbox Turbo: The "human" element is significantly higher. The model captures micro-tremors in pitch, breath intake, and varied pacing that signals high production value. It is better suited for narrative content (fiction, gaming) where emotional engagement is key.8.2 The "Uncanny Valley" of LatencySupertonic: The near-instant response (0.006 RTF) creates a seamless user experience. It feels like a native OS feature.Chatterbox: Even on desktop, the 200ms+ latency can create a "turn-taking" delay in conversational apps. On a slow connection (mobile remote fallback), this latency can spike to seconds, breaking the illusion of interactivity.9. Commercial and Operational Considerations9.1 Licensing and WatermarkingSupertonic 2: Released under the OpenRAIL-M license.5 This license permits commercial use but includes usage restrictions to prevent abuse (e.g., generating deepfakes for fraud). It does not mandate watermarking, though developers should be mindful of transparency.Chatterbox: Released under the MIT license 6, the most permissive option. However, Resemble AI includes PerTh Watermarking technology baked into the model.12 Every generated audio file contains an imperceptible watermark. This is a robust safety feature for a commercial app, allowing you to prove the provenance of the audio if challenged, but it incurs a small computational cost during inference.9.2 Update VelocitySupertone Inc.: The release of v2 in Jan 2026 suggests a committed roadmap. The shift to a unified multilingual architecture indicates a maturity in their R&D pipeline.Resemble AI: Chatterbox is an open-source offshoot of their core commercial product. Updates are frequent, but often prioritize their paid API services or newer, larger models that may drift further away from consumer hardware capability.10. Conclusion and Strategic RecommendationThe comparative analysis yields a definitive conclusion based on the user's specific constraint of running locally across iOS, Android, Mac, and Linux.The Recommendation: Supertonic 2 is the superior architectural choice.Mobile Feasibility: Supertonic 2 is the only candidate that offers a viable path to local inference on iOS and Android within a Tauri application. Its ONNX-native architecture allows for static linking and NPU acceleration, bypassing the OS restrictions that block Chatterbox's Python-based stack.Performance Profile: With an inference speed 166x faster than real-time and a memory footprint of under 500MB, Supertonic ensures the application remains responsive and stable on resource-constrained mobile devices. Chatterbox's 4GB requirement is a non-starter for mobile memory budgets.Integration Simplicity: While Rust requires a learning curve, the ort integration is cleaner and more robust than maintaining a fragile Python sidecar build chain.When to Consider Chatterbox:Chatterbox should only be selected if the application is Desktop-exclusive (Mac/Linux/Windows) or if the requirement for Zero-Shot Voice Cloning and Paralinguistic Tags (laughter, emotion) outweighs the requirement for "Local" execution on mobile. In that scenario, a hybrid architecture (Local Desktop + Remote Mobile API) is the only path forward.For the stated goal of a unified, local, cross-platform Tauri build, Supertonic 2 is not just the better option; it is practically the only option.


## Claude

# Local TTS for Tauri: Supertonic vs Chatterbox compared

**Supertonic emerges as the clear choice for cross-platform Tauri deployment**, offering native Rust integration, ~264 MB model size, and proven iOS/Android support out of the box. Chatterbox provides superior voice cloning and emotion control but at **10-12× the model size** and significantly higher deployment complexity. For a privacy-focused chat application prioritizing simplicity and bundle size, Supertonic's ONNX-based architecture delivers the most practical path to production.

## Model architecture and runtime requirements

**Supertonic** runs entirely on ONNX Runtime, making it deployment-friendly across all platforms. The architecture splits into four ONNX components: text encoder (28 MB), vector estimator (133 MB), vocoder (101 MB), and duration predictor (1.6 MB). With only **66 million parameters**, it's deliberately optimized for edge devices—proven to run on Raspberry Pi and e-readers at 0.3× real-time factor.

**Chatterbox** was built on PyTorch with a **0.5B Llama backbone**, requiring substantially more resources. Three model variants exist: the original 500M parameter model, Chatterbox-Multilingual (500M, 23 languages), and Chatterbox-Turbo (350M, optimized for speed). While native inference requires PyTorch with CUDA/MPS/ROCm backends, official ONNX exports now exist through `ResembleAI/chatterbox-turbo-ONNX`.

| Specification | Supertonic | Chatterbox |
|--------------|------------|------------|
| Parameters | 66M | 350M-500M |
| Native framework | ONNX Runtime | PyTorch |
| ONNX available | ✅ Primary | ✅ Exported |
| MLX support | ❌ | ✅ via mlx-audio |

## Model sizes shape deployment decisions

Supertonic's total ONNX bundle weighs approximately **264 MB** across all components, with OnnxSlim optimizations shaving a few megabytes. This size remains consistent since the architecture doesn't support quantization variants in the official release.

Chatterbox offers more flexibility through quantization but starts much larger. The full-precision Turbo ONNX export totals **~3.3 GB** across its four sessions (speech encoder, language model, conditional decoder, embed tokens). Quantized variants dramatically reduce this:

- **Q4F16** (4-bit with FP16): ~560 MB total
- **INT8 (Q8)**: ~1.1 GB total  
- **FP16**: ~1.7 GB total

For mobile deployment, the Q4F16 Chatterbox variant at 560 MB remains **roughly twice Supertonic's size**. Memory requirements diverge even more sharply: Supertonic runs comfortably in **250-500 MB RAM**, while Chatterbox ONNX peaks at **~3.2 GB RAM** on iOS based on real-world testing.

## Cross-platform deployment capabilities

Supertonic provides exceptional platform coverage with **official examples for every major platform** in its repository:

- **Desktop**: Windows, macOS, Linux via C++, Rust, Go, Python, Node.js, Java, C#
- **Mobile**: Native iOS (Swift/Xcode), Android (Java/Kotlin), Flutter
- **Web**: WebGPU/WASM (Chrome 121+, Edge 121+, Safari macOS 15+)
- **Embedded**: Proven on Raspberry Pi, Onyx Boox e-readers

Chatterbox's platform support depends heavily on your chosen runtime:

- **PyTorch native**: Linux (primary), macOS (MPS), Windows (CUDA/CPU only)
- **ONNX Runtime**: All platforms theoretically supported; iOS demonstrated working
- **MLX**: macOS 14.0+ and iOS 16.0+ only (Apple Silicon exclusive)
- **Android**: ONNX Runtime supports it, but not officially tested

## Rust integration and Tauri compatibility

**Supertonic offers native Rust support** directly in the repository's `rust/` directory. The implementation uses ONNX Runtime Rust bindings, making Tauri integration straightforward—you can call TTS directly from your Rust backend without spawning external processes.

```rust
// Supertonic approach: Native Rust in Tauri backend
// Uses ort crate (ONNX Runtime) directly
```

**Chatterbox lacks official Rust bindings**, creating three integration paths for Tauri:

1. **ONNX via `ort` crate**: Load quantized ONNX models directly from Rust—no Python required, works cross-platform
2. **Python sidecar**: Bundle PyInstaller-compiled Python with Tauri's `externalBin` feature
3. **Local HTTP server**: Run chatterbox-tts-api as subprocess with OpenAI-compatible endpoints

The Python sidecar approach has been documented for Chatterbox with mlx-audio. Configure `tauri.conf.json` with `"externalBin": ["binaries/tts-sidecar"]`, compile Python using PyInstaller with target-specific naming (`tts-sidecar-x86_64-apple-darwin`), and spawn via `app.shell().sidecar()`. Known issues include sidecars not terminating cleanly on app close and **50-200 MB additional bundle size** for the Python runtime.

## Voice quality and feature comparison

Both systems produce high-quality, natural speech—neither sounds robotic in typical usage.

**Supertonic** offers configurable inference steps trading speed for quality:
- 2-step inference: "Close to ElevenLabs Flash" quality, fastest
- 5-step inference: "Reaches much of ElevenLabs Prime tier"
- 10+ steps: Highest quality, slower

It includes **11 preset voices** (5 male, 5 female) and excels at text normalization—handling currencies ($5.2M), dates, phone numbers, and abbreviations without preprocessing. Supertonic 2, released January 6, 2026, added support for English, Korean, Spanish, Portuguese, and French.

**Chatterbox** won **63.75% preference over ElevenLabs** in blind evaluations and offers richer features:
- **Zero-shot voice cloning** from 5-10 seconds of reference audio
- **Emotion exaggeration control** (0 = monotone, 1 = normal, 2+ = dramatic)
- **Paralinguistic tags**: `[laugh]`, `[cough]`, `[sigh]`, `[groan]`
- **23 languages** in the multilingual model
- Built-in neural watermarking (PerTh) for provenance tracking

## Performance benchmarks reveal the gap

Supertonic's lightweight architecture delivers exceptional speed-to-quality ratios:

| Hardware | Supertonic RTF | Throughput |
|----------|---------------|------------|
| M4 Pro (CPU) | 0.015 | 1,263 chars/sec |
| M4 Pro (WebGPU) | 0.006 | 2,509 chars/sec |
| RTX 4090 | 0.001 | 12,164 chars/sec |
| Raspberry Pi | 0.3 | Real-time capable |

Chatterbox requires more compute but achieves competitive latency:
- **Streaming RTF**: 0.499 on RTX 4090
- **Latency**: Sub-200ms optimized, sub-300ms typical
- **Apple Silicon via MLX**: 2-3× faster than CPU
- **Mobile (iOS ONNX)**: Functional but ~3.2 GB peak RAM

## Licensing permits commercial use

Both projects use permissive licenses suitable for commercial applications:

| Aspect | Supertonic | Chatterbox |
|--------|------------|------------|
| Code license | MIT | MIT |
| Model license | OpenRAIL-M | MIT |
| Commercial use | ✅ Allowed | ✅ Allowed |
| Voice cloning | Not supported | Built-in |
| Watermarking | None | Optional neural watermark |

## Practical deployment recommendation

For your privacy-focused Tauri chat application, **Supertonic provides the most straightforward path**:

1. **Native Rust integration**: Add ONNX Runtime via `ort` crate, call TTS directly from Tauri backend
2. **Minimal bundle impact**: ~264 MB models + ~7-15 MB ONNX Runtime per platform
3. **True cross-platform**: iOS, Android, macOS, Linux work today with official examples
4. **No Python dependency**: Ships as pure native code

Choose Chatterbox only if you specifically need **voice cloning** or **emotion control**. In that case, use the Q4F16 ONNX variant (~560 MB) with the `ort` crate for maximum portability. Avoid the Python sidecar approach on mobile—it's impractical due to bundle size and process management complexity.

## Conclusion

The two systems serve different use cases despite both being local TTS solutions. Supertonic optimizes for **deployment simplicity and efficiency**—its 66M parameter model runs on constrained devices while maintaining quality competitive with cloud services. Chatterbox optimizes for **feature richness**—voice cloning and emotion control justify its 5-8× larger footprint for applications requiring those capabilities.

For a Tauri app shipping across iOS, Android, macOS, and Linux with privacy as a priority, Supertonic's architecture eliminates the primary pain points: no Python runtime, no platform-specific codepaths for inference, and proven mobile deployment. The January 2026 Supertonic 2 release with multilingual support further strengthens this recommendation for international applications.

---

## Grok

### Key Considerations for Choosing Between Supertonic and Chatterbox
- **Model Size and Resource Efficiency**: Research indicates Supertonic (66 million parameters) is significantly smaller and more lightweight than Chatterbox models (350-500 million parameters), making it better suited for resource-constrained devices across platforms.
- **Cross-Platform Compatibility**: Evidence leans toward Supertonic being more reliable and easier to deploy on iOS, Android, Mac, and Linux due to its ONNX Runtime integration, with native examples for mobile and desktop; Chatterbox, while feasible on Mac and potentially iOS via optimizations like MLX, lacks straightforward Android support and relies on Python, complicating Tauri integration.
- **Ease of Integration in Tauri Apps**: Supertonic appears more straightforward for Rust-based Tauri builds with direct Rust examples, while Chatterbox may require a Python sidecar (e.g., with MLX-audio on Apple devices), adding complexity but remaining viable for certain setups.
- **Reliability and Recent Developments**: Both are actively maintained, but Supertonic 2's January 2026 release introduces multilingual support and optimizations, suggesting it may offer fresher reliability for local runs; user feedback highlights its stability for long-form audio, though Chatterbox excels in voice cloning and expressiveness.
- **Recommendation**: For local TTS in Tauri across all specified platforms, Supertonic seems likely the better choice due to its smaller size, broader native compatibility, and simpler integration, though Chatterbox could suit scenarios prioritizing advanced features like zero-shot cloning if platform constraints are flexible.

#### Tech Stack Overview
Supertonic leverages ONNX Runtime for inference, enabling efficient, CPU-optimized execution without heavy dependencies—ideal for embedding in cross-platform apps. It supports batch processing and configurable inference steps (e.g., 2-5), with models hosted on Hugging Face (https://huggingface.co/Supertone/supertonic-2). Chatterbox, built on PyTorch, requires Python environments and supports CUDA, CPU, or MPS (Metal Performance Shaders) for Apple devices, but integration often involves additional tools like MLX-audio for optimization on Mac/iOS.

#### Performance and Features
Supertonic prioritizes speed, achieving real-time factors (RTF) as low as 0.006 on WebGPU and handling complex text (e.g., dates, currencies) natively. Chatterbox focuses on quality, with features like paralinguistic tags ([laugh], [chuckle]) for expressive speech and zero-shot voice cloning, though it may require more VRAM (around 5-6.5GB on GPUs). For local runs, Supertonic's lightweight design suggests lower latency on diverse hardware, while Chatterbox performs well on supported setups but could be slower on non-optimized platforms.

#### Platform-Specific Insights
- **iOS/Android**: Supertonic offers native iOS examples and Flutter support for cross-mobile deployment; ONNX Runtime ensures compatibility (https://onnxruntime.ai/docs/build/ios.html). Chatterbox has been adapted for iOS via apps like Chinny, but Android integration is less documented and may need custom work.
- **Mac/Linux**: Both run well—Supertonic via Swift/Rust on Mac and various examples on Linux; Chatterbox uses MPS on Mac for acceleration and is straightforward on Linux with CUDA/CPU.

If prioritizing minimal footprint and ease, start with Supertonic's Rust example for Tauri: clone the repo, build with Cargo, and integrate ONNX models directly.

---
### In-Depth Analysis of Supertonic and Chatterbox for Cross-Platform Local TTS in Tauri Applications

Text-to-speech (TTS) models like Supertonic and Chatterbox represent significant advancements in open-source AI, enabling privacy-focused, offline audio generation for applications such as voice assistants, accessibility tools, and content creation. As of early 2026, these models cater to developers building cross-platform apps with frameworks like Tauri, which allows Rust-based backends for web-like UIs on iOS, Android, Mac, and Linux. This analysis draws from repository details, user integrations, performance benchmarks, and community feedback to evaluate their suitability for local deployment. We prioritize factors like model size, tech stack, platform compatibility, reliability, and Tauri-specific integration, acknowledging that while both are capable, differences in architecture influence their ease of use across devices.

#### Model Architectures and Core Technologies
Supertonic, developed by Supertone Inc., is an ONNX-based TTS system optimized for on-device inference with minimal overhead. Its core relies on ONNX Runtime, a cross-platform engine that supports CPU-optimized execution (GPU untested in the repo but feasible). Models are slimmed using OnnxSlim, resulting in efficient, lightweight files. The system generates 16-bit WAV audio, supports batch processing for throughput, and handles natural text variations (e.g., phone numbers, units) without preprocessing. Supertonic 2, released on January 6, 2026 (v2.0.0), expands to multilingual support for English, Korean, Spanish, Portuguese, and French, with six new voice styles (M3-M5, F3-F5). It's licensed under MIT for code and OpenRAIL-M for models, allowing commercial use.

Chatterbox, from Resemble AI, is a PyTorch-based family of models: the original (500M parameters, English-only), Multilingual (500M, 23+ languages), and Turbo (350M, English with paralinguistic tags like [chuckle] or [cough]). It emphasizes high-fidelity, zero-shot voice cloning, and expressive speech via configurable parameters (e.g., CFG for guidance, exaggeration for emotion). All include Perth watermarking for ethical traceability. The Turbo variant distills the decoder to a single generation step, reducing latency and VRAM needs. It's MIT-licensed and installable via pip, with dependencies managed in pyproject.toml for Python 3.11 on Debian-like systems.

Key tech differences: Supertonic's ONNX focus enables broader runtime flexibility without Python, while Chatterbox's PyTorch ties it to Python environments, potentially requiring sidecars in non-Python apps like Tauri.

#### Model Sizes and Resource Requirements
Model size directly impacts local feasibility, especially on mobile devices with limited RAM/VRAM.

| Model | Variant | Parameters | Approximate Size | VRAM Usage (GPU) | Key Optimizations |
|-------|---------|------------|------------------|------------------|-------------------|
| Supertonic | Supertonic 2 | 66M | Ultra-lightweight (optimized ONNX files) | Minimal (CPU-focused; ~low GB if GPU) | OnnxSlim for compression; batch support |
| Chatterbox | Turbo | 350M | Medium | ~5GB (e.g., RTX 3060) | Distilled decoder; low-latency mode |
| Chatterbox | Multilingual/Original | 500M | Larger | ~6.5GB | Zero-shot cloning; expressive tuning |

Supertonic's 66M parameters make it the smallest, enabling runs on edge devices like Raspberry Pi or e-readers with RTF as low as 0.012 on CPU. Chatterbox models, at 350-500M, demand more resources but offer efficiencies like 1-step generation in Turbo, using ~5GB VRAM for faster output (e.g., 1.8x speed over original). For Tauri apps, Supertonic's footprint reduces bundling overhead, while Chatterbox may need quantized versions (e.g., 6-bit via MLX) for mobile.

#### Performance Benchmarks and Features
Performance varies by use case: speed vs. quality.

- **Speed and Latency**: Supertonic excels, processing up to 12,164 characters/second on RTX 4090 and 167x real-time on M4 Pro Mac, with RTF 0.006 on WebGPU. It's faster than Chatterbox on non-NVIDIA hardware. Chatterbox Turbo achieves sub-200ms latency, suitable for real-time agents, and handles long texts stably via chunking.
- **Audio Quality and Expressiveness**: Chatterbox leads in naturalness, with low word error rates, emotional carry-over, and tags for non-verbal cues; it outperforms paid services like ElevenLabs in cloning (7-11s reference audio). Supertonic provides stable, natural long-form narration but lacks cloning or advanced emotion tuning, focusing on clear, reliable output.
- **Multilingual Support**: Supertonic 2 adds five languages; Chatterbox Multilingual covers 23+.

In comparisons, Supertonic is praised for efficiency in resource-limited scenarios, while Chatterbox shines in expressive, cloned audio.

#### Cross-Platform Compatibility and Deployment
ONNX Runtime makes Supertonic highly portable: it supports iOS (native Xcode), Android (via Flutter), Mac (Swift/MPS), Linux (multiple languages), and even browsers (WebGPU/WASM). Installation involves cloning the repo, Git LFS for models, and language-specific builds (e.g., `cargo build` for Rust).

Chatterbox supports Mac (MPS), Linux (CUDA/CPU), and Windows (GPU), with iOS adaptations via apps like Chinny for offline runs. Android integration is not native; it may require embedding Python or API wrappers. MLX-audio optimizes for Apple Silicon, enabling faster inference on Mac/iOS.

For Tauri: Supertonic integrates directly via Rust examples, embedding ONNX in the backend. Chatterbox uses a Python sidecar (e.g., via tauri-plugin-shell), running scripts as external processes—feasible but adds overhead, especially with MLX-audio for Apple platforms.

#### Reliability, Maintenance, and Community Feedback
Both are reliable for local use, with active repos (Supertonic updated January 2026; Chatterbox last in mid-2025). Supertonic's on-device privacy and stability for long narration earn praise in integrations (e.g., browser extensions, e-book apps). Early Supertonic 2 reviews highlight its speed and natural voices on Windows/Mac. Chatterbox is lauded for beating paid TTS in quality but may face setup issues on non-GPU devices.

Pros/Cons Table:

| Aspect | Supertonic | Chatterbox |
|--------|------------|------------|
| **Pros** | Ultra-fast, small size, broad platforms, easy Rust/Tauri integration | Superior quality/cloning, expressive tags, multilingual options |
| **Cons** | Limited voices/cloning, recent multilingual addition | Larger size, Python-dependent, limited mobile native support |
| **Best For** | Speed-focused, cross-platform local apps | Quality-focused, voice agents with cloning |

#### Recommendations and Implementation Tips
For Tauri builds targeting all platforms, Supertonic is preferable: download models from Hugging Face, use Rust bindings for backend inference, and expose via Tauri commands. Test on devices like Raspberry Pi for edge cases. If voice cloning is essential, opt for Chatterbox with a Python sidecar—use MLX-audio for Mac/iOS acceleration and explore quantization for Android. Always verify with hardware benchmarks, as real-world performance varies.

### Key Citations
- [GitHub - supertone-inc/supertonic](https://github.com/supertone-inc/supertonic)
- [GitHub - resemble-ai/chatterbox](https://github.com/resemble-ai/chatterbox)
- [Lightning-Fast Open-Source TTS Model Is Here!](https://medium.com/@bytefer/12-164-characters-per-second-lightning-fast-open-source-tts-model-is-here-96452a3dd66f)
- [ONNX Runtime | Home](https://onnxruntime.ai/)
- [ONNX Runtime: Enabling Cross-Platform AI Model Inference](https://medium.com/aimonks/onnx-runtime-enabling-cross-platform-ai-model-inference-80f136ecbb2d)
- [This Local AI Voice Model Beats Paid TTS (Chatterbox Turbo Tested)](https://www.youtube.com/watch?v=3UN70ea4h9c)
- [A few insights from running Chatterbox fully offline on iPhone and Mac](https://huggingface.co/ResembleAI/chatterbox/discussions/42)
- [Blaizzy/mlx-audio](https://github.com/Blaizzy/mlx-audio/actions)
- [Supertonic TTS is incredible — sharing our integration results](https://huggingface.co/Supertone/supertonic/discussions/13)
- [Supertonic TTS: Ultra-Fast, Natural Voices on Windows & Mac](https://sonusahani.com/blogs/supertonic-tts)
- [Supertonic Review: A New, Ultra-Fast and Lightweight AI Model](https://www.youtube.com/watch?v=g2CuKBqCeK0)
- [The Top Open-Source Text to Speech (TTS) Models](https://modal.com/blog/open-source-tts)
- [The Best Open-Source Text-to-Speech Models in 2026](https://www.bentoml.com/blog/exploring-the-world-of-open-source-text-to-speech-models)
- [Embedding External Binaries](https://v2.tauri.app/develop/sidecar/)
- [How Chatterbox Is Redefining Open-Source AI Voice](https://medium.com/%40vinodpolinati/chatterbox-the-leading-family-of-open-source-ai-voice-models-thats-changing-the-game-150991d5cf8d)
