# Windows Research

## Gemini

Comprehensive Guide to Windows Distribution Architecture for Tauri v2 Applications1. Introduction: The Windows Distribution ParadigmThe transition from UNIX-based operating systems (macOS, Linux) to the Windows platform represents one of the most significant architectural shifts in the lifecycle of a desktop application. While modern cross-platform frameworks like Tauri abstract the majority of the development complexity—allowing Rust backends and web frontends to operate seamlessly across environments—the distribution pipeline remains stubbornly distinct. Windows operates on a fundamentally different trust model, utilizing a unique rendering engine distribution strategy and a code signing infrastructure that has undergone a radical transformation as of 2025.For a production-grade application already shipping to iOS, Android, macOS, and Linux, the integration of Windows support is not merely a matter of adding a windows-latest runner to a CI/CD pipeline. It requires a strategic navigation of Microsoft's Authenticode system, the Windows Defender SmartScreen reputation engine, and the dependency management of WebView2.This report establishes a definitive architectural blueprint for shipping Tauri v2 applications to Windows in 2025. It specifically addresses the needs of individual developers who operate outside the Microsoft Store ecosystem, prioritizing direct distribution via verified installers. The analysis focuses on leveraging Azure Trusted Signing—a cloud-native cryptographic service that renders legacy hardware-based signing obsolete—and implementing a fully automated, keyless CI/CD pipeline using GitHub Actions and OpenID Connect (OIDC).1.1 The Evolution of Trust: From Hardware Tokens to Cloud HSMsHistorically, the primary barrier to entry for independent Windows developers was the code signing certificate. Prior to 2023, acquiring an Organization Validated (OV) or Extended Validation (EV) certificate required purchasing a physical hardware token (USB HSM) from a Certificate Authority (CA) such as Sectigo or DigiCert.1 This token had to be physically connected to a machine to sign binaries, making cloud-based automation (CI/CD) prohibitively difficult or insecure. Developers often resorted to "redirecting" USB ports over networks or manually signing builds on local machines, breaking the chain of automated delivery.3In 2025, this legacy architecture is deprecated in favor of Azure Trusted Signing (formerly Azure Code Signing). This managed service eliminates the handling of private keys entirely. Instead of possessing a .pfx file or a physical token, the build pipeline authenticates with Azure, submits a hash of the binary, and receives a signature signed by a Microsoft-managed root certificate.4 This shift democratizes access to professional-grade code signing, reducing costs from hundreds of dollars per year to a usage-based model starting at approximately $9.99 per month.61.2 Core Components of the Windows PipelineThe proposed architecture relies on three integrated pillars:Identity Verification (Azure Trusted Signing): A cloud service where Microsoft verifies the developer's identity (individual or organization) and manages the PKI infrastructure. This satisfies the operating system's requirement for a trusted publisher.7Build Orchestration (GitHub Actions + OIDC): A CI pipeline that builds the Rust/Frontend artifacts and authenticates with Azure using ephemeral tokens rather than static secrets, ensuring high security.8Application Packaging (Tauri Bundler + NSIS): The generation of a robust installer (.exe) that manages the WebView2 runtime dependency, application placement, and update logic.92. Windows Trust Architecture and Identity ValidationThe prerequisite for any executable to run on modern Windows systems without triggering severe security warnings is a valid digital signature. Windows employs a technology called Authenticode to verify the integrity and origin of binaries.2.1 The SmartScreen BarrierWhen a user downloads an application from the internet, Windows applies a "Mark of the Web" (MotW) to the file. Upon execution, Windows Defender SmartScreen checks the file's reputation. Unsigned applications are blocked immediately. Signed applications are evaluated based on the reputation of their signing certificate.2In the legacy model, Extended Validation (EV) certificates provided "instant reputation," bypassing SmartScreen immediately. However, industry analysis indicates that as of late 2024, EV certificates no longer guarantee instant reputation.11 Reputation is now built organically through download volume and integrity history. While Azure Trusted Signing provides the highest level of cryptographic trust (Public Trust), developers must anticipate a "warm-up" period where SmartScreen warnings may persist until the application establishes a download history.2.2 Azure Trusted Signing for IndividualsCrucially for individual developers, Microsoft expanded the Trusted Signing service to support Individual Identity Validation in late 2024 (previously restricted to organizations with 3+ years of tax history).122.2.1 The Validation WorkflowThe validation process for individuals is rigorous but fully digital, leveraging the Microsoft Entra Verified ID system.Azure Account Creation: The developer requires an Azure account with an active subscription. A "Pay-As-You-Go" subscription is recommended to avoid upfront costs, as the service bills monthly.4Resource Provider Registration: The Microsoft.CodeSigning resource provider must be manually registered within the Azure subscription settings to unlock the capabilities.14Identity Submission: Within the Trusted Signing resource, the developer selects "Identity Validation" and chooses "Individual".Verification Steps:Document Upload: A government-issued photo ID (Passport or Driver's License) is required.Biometric Check: A "liveness" check (selfie) is performed via the Microsoft Authenticator app on a mobile device to match the ID holder to the applicant.12Address Verification: If the ID does not contain a current address, supplemental documentation (utility bill) may be required.Processing Time: Validation typically completes within 1 to 3 business days.142.2.2 Certificate Profiles: Public vs. PrivateOnce identity is validated, a Certificate Profile must be created. This profile dictates the type of certificate Azure will generate on-the-fly during the signing process.Public Trust (Mandatory): This profile chains to the Microsoft Public Root Certificate. It is trusted by Windows by default and is required for distributing applications to the public.Private Trust: This chains to a private root and is only trusted within organizations that have explicitly installed that root. It is unsuitable for general distribution.7Table 1: Comparison of Signing ArchitecturesFeatureLegacy Code Signing (Sectigo/DigiCert)Azure Trusted Signing (Recommended)Key StoragePhysical USB Token (HSM)Azure Cloud HSM (FIPS 140-2 Level 3)Cost$500 - $800 / year~$9.99 / month + usage 6CI/CD IntegrationDifficult (Requires physical hardware access)Native (Azure CLI / GitHub Actions)Certificate TypeLong-lived .pfx fileShort-lived (3-day) ephemeral certs 4RevocationDifficult (CRL updates)Instant (Cloud-managed)Individual SupportYes (often expensive)Yes (via Verified ID) 122.3 Pricing and Operational CostsThe "Basic" tier of Azure Trusted Signing is priced at $9.99 per month. This tier includes:Identity validation maintenance.Certificate lifecycle management.A quota of signatures (typically sufficient for daily builds of a single application). Overage is charged per signature, but for standard release cycles, the base tier is comprehensive.63. Tauri v2 Configuration and ArchitectureTauri v2 introduces a plugin-centric architecture and a capability-based security model that significantly alters how Windows integrations are configured compared to v1.3.1 The Rendering Engine: WebView2 StrategyUnlike macOS, which guarantees the presence of WebKit, Windows relies on WebView2 (based on Microsoft Edge Chromium). While WebView2 is pre-installed on Windows 11 and modern Windows 10 updates, it is not guaranteed on every target machine (e.g., Windows Server or unpatched Windows 10 LTSC).15Tauri provides several strategies for handling this dependency in tauri.conf.json:downloadBootstrapper (Recommended): The installer includes a lightweight shim. If the runtime is missing, it downloads and installs the "Evergreen" runtime from Microsoft.Advantage: Keeps the installer size small (~2-5 MB added). Ensures the user receives the latest security patches.Disadvantage: Fails if the user is offline during installation.9offlineInstaller: Embeds the full WebView2 runtime installer.Advantage: Works without internet access.Disadvantage: Increases installer size by approximately 150 MB.9fixedVersion: Bundles a specific binary version of the WebView2 binaries.Advantage: Prevents breaking changes from browser updates.Disadvantage: Shifts security patching responsibility to the developer. Highly discouraged for general internet-facing apps.9For a general-purpose production app distributed via the web, downloadBootstrapper is the industry standard balance of size and reliability.3.2 Installer Technology: NSIS vs. WiX (MSI)Tauri supports building .msi (via WiX Toolset) and .exe installers (via NSIS - Nullsoft Scriptable Install System).Recommendation: NSIS (.exe)NSIS is the preferred format for Tauri v2 for several reasons:Update Reliability: The NSIS installer logic is more resilient for the "overwrite-update" mechanism used by Tauri's auto-updater. MSI updates can encounter "repair loop" issues if component GUIDs are not managed perfectly.16User Experience: NSIS supports a "Per User" install mode which does not require Administrator privileges, reducing friction for users. MSI often defaults to machine-wide installs requiring UAC elevation.16Customization: NSIS scripting allows for easier UI customization and logic hooks compared to the rigid declarative structure of WiX.Configuration (tauri.conf.json):JSON{
  "bundle": {
    "active": true,
    "targets": ["nsis"],
    "windows": {
      "webviewInstallMode": {
        "type": "downloadBootstrapper"
      },
      "nsis": {
        "installMode": "perUser",
        "languages":
      }
    }
  }
}
3.3 Visual Assets: IconographyWindows requires strict adherence to the .ico file format. A simple PNG rename is insufficient. The .ico file must contain multiple bitmap layers (16x16, 32x32, 48x48, 64x64, 256x256) to render correctly across the taskbar, file explorer, and window title bars.18Implementation:The Tauri CLI includes an image generation tool. Run the following command with a high-resolution source image (1024x1024 PNG recommended) to generate the compliant assets:Bashnpm run tauri icon./src/assets/app-icon.png
This command populates src-tauri/icons/icon.ico specifically for the Windows build target.4. Implementation of Cloud Code SigningIntegrating Azure Trusted Signing into the Tauri build process requires a departure from standard signtool usage. Since the signing certificates are ephemeral and exist only in the cloud, the local build environment acts as a client that hashes the binary and requests a signature.4.1 The signCommand HookTauri v2 allows developers to override the default signing logic via the signCommand configuration in tauri.conf.json. This hook is executed by the bundler immediately after the executable is compiled but before it is packaged into the installer (NSIS).19Because the Azure signing tool (trusted-signing-cli or Azure.CodeSigning.Tool) requires multiple arguments—specifically the Endpoint, Account Name, and Certificate Profile Name—it is best practice to wrap this invocation in a script. This script acts as an adapter between Tauri (which passes the file path as an argument) and the Azure tool.tauri.conf.json Configuration:JSON{
  "bundle": {
    "windows": {
      "signCommand": "filesign %1"
    }
  }
}
Note: %1 is the variable where Tauri injects the absolute path to the binary.4.2 Installing the Signing ClientThe recommended tool for 2025 is the Azure Code Signing Tool (installed via dotnet tool install). This tool handles the authentication handshake and the signing request. In the CI environment, this tool must be installed on the runner before the build step initiates.204.3 Authentication: OpenID Connect (OIDC)Authenticating the GitHub Actions runner with Azure should never be done using long-lived Client Secrets (username/password pairs). These are security risks and require rotation.OpenID Connect (OIDC) allows GitHub and Azure to establish a trust relationship.Azure App Registration: Create an application in Azure Entra ID representing the GitHub Action.Federated Credential: Configure the App Registration to trust tokens issued by token.actions.githubusercontent.com for the specific repository and branch (e.g., main or release tags).8Role Assignment: Grant this Service Principal the "Trusted Signing Certificate Profile Signer" role on the Trusted Signing Account resource.21When the GitHub Action runs, it requests a JWT from GitHub, exchanges it for an Azure access token, and uses that token to authorize the signing request—all without storing a single password in the repository secrets.5. CI/CD Pipeline Architecture: GitHub ActionsThis section details the construction of the .github/workflows/release.yml file. This pipeline is the engine of the distribution system.5.1 Pipeline StrategyThe workflow must perform the following sequence:Environment Setup: Provision a Windows runner, install Node.js (frontend), and Rust (backend).Authentication: Perform the OIDC login to Azure.Tooling: Install the Azure Code Signing CLI tool.Adapter Script: Generate a temporary PowerShell script (filesign.ps1) to inject the Azure configuration parameters into the signing command.Build & Bundle: Execute tauri build. The bundler calls the adapter script, which signs the binaries.Release: Upload the signed NSIS installer (.exe) and the update signature (.sig) to a GitHub Release.5.2 The Workflow ConfigurationBelow is the complete, annotated YAML configuration. It relies on several repository secrets that must be configured:AZURE_CLIENT_ID: Application ID of the Service Principal.AZURE_TENANT_ID: Directory ID of the Azure Entra tenant.AZURE_SUBSCRIPTION_ID: ID of the billing subscription.AZURE_ENDPOINT: The region-specific signing URL (e.g., https://wus.codesigning.azure.net).AZURE_ACCOUNT: The Trusted Signing Account name.AZURE_PROFILE: The Certificate Profile name.TAURI_SIGNING_PRIVATE_KEY: The Ed25519 private key for the updater (discussed in Section 6).TAURI_SIGNING_PRIVATE_KEY_PASSWORD: Password for the updater key (optional).YAMLname: Release Windows
on:
  push:
    tags:
      - 'v*' # Triggers on version tags (e.g., v1.0.0)

permissions:
  id-token: write # Mandatory for OIDC authentication
  contents: write # Mandatory for creating Releases

jobs:
  release:
    runs-on: windows-latest
    steps:
      - uses: actions/checkout@v4

      # Setup Frontend Environment
      - name: Setup Node
        uses: actions/setup-node@v4
        with:
          node-version: 20
          cache: 'npm' # or yarn/pnpm

      - name: Install Frontend Dependencies
        run: npm ci

      # Setup Backend Environment
      - name: Setup Rust
        uses: dtolnay/rust-toolchain@stable
        with:
          targets: x86_64-pc-windows-msvc

      # 1. Authenticate to Azure using OIDC
      - name: Azure Login
        uses: azure/login@v2
        with:
          client-id: ${{ secrets.AZURE_CLIENT_ID }}
          tenant-id: ${{ secrets.AZURE_TENANT_ID }}
          subscription-id: ${{ secrets.AZURE_SUBSCRIPTION_ID }}

      # 2. Install the Azure Signing Tool
      - name: Install Azure Signing Tool
        shell: powershell
        run: dotnet tool install --global Azure.CodeSigning.Tool

      # 3. Create the Signing Wrapper Script
      # This creates a 'filesign.ps1' that calls the Azure tool with your secrets.
      # Tauri will call 'filesign <path>' during the bundle step.
      - name: Create Signing Adapter
        shell: powershell
        run: |
          $content = @"
          acs tool sign -v `
            -u "${{ secrets.AZURE_ENDPOINT }}" `
            -a "${{ secrets.AZURE_ACCOUNT }}" `
            -p "${{ secrets.AZURE_PROFILE }}" `
            -i "`$args"
          "@
          Set-Content -Path filesign.ps1 -Value $content
          # Add current directory to PATH so 'filesign' is discoverable
          echo "$PWD" | Out-File -FilePath $env:GITHUB_PATH -Encoding utf8 -Append

      # 4. Build and Bundle
      # The hook in tauri.conf.json ("signCommand": "filesign %1") triggers here.
      - name: Build Tauri App
        uses: tauri-apps/tauri-action@v0
        env:
          GITHUB_TOKEN: ${{ secrets.GITHUB_TOKEN }}
          TAURI_SIGNING_PRIVATE_KEY: ${{ secrets.TAURI_SIGNING_PRIVATE_KEY }}
          TAURI_SIGNING_PRIVATE_KEY_PASSWORD: ${{ secrets.TAURI_SIGNING_PRIVATE_KEY_PASSWORD }}
        with:
          tagName: app-v__VERSION__ 
          releaseName: 'App v__VERSION__'
          releaseBody: 'See the assets to download this version and install.'
          releaseDraft: true
          prerelease: false
6. The Auto-Update SystemFor applications distributed outside the Microsoft Store, a robust auto-update mechanism is critical for security and feature delivery. Tauri v2 provides a built-in updater system that requires specific cryptographic configurations separate from the Windows OS code signing.6.1 Dual-Layer Security ModelIt is crucial to distinguish between the two layers of signing involved in a Windows release:OS Layer (Outer Shell): The .exe binary is signed by Azure Trusted Signing. This satisfies Windows SmartScreen and the OS security policies.App Layer (Inner Shell): The update manifest and the update package are signed/verified using a private Ed25519 key generated by Tauri. This satisfies the Tauri updater plugin, ensuring that the update files retrieved from your server have not been tampered with or spoofed.226.2 Generating Updater KeysYou must generate a keypair specifically for the updater. This is a one-time process.Bashnpm run tauri signer generate -w ~/.tauri/myapp.key
This command generates:Private Key: Saved to your local machine (and used in CI secrets).Public Key: Displayed in the terminal. This key is safe to distribute and must be hardcoded into your tauri.conf.json.6.3 Update Manifest (Static JSON)Tauri v2 supports a "Static JSON" update strategy, which is ideal for simple website hosting (S3, GitHub Pages, Cloudflare R2). You do not need a complex backend server; a simple JSON file hosted at a stable URL is sufficient.JSON Schema:The update server (or static file) must return a JSON object adhering to the specific v2 schema. Note that the signature field requires the content of the .sig file, not a path.JSON{
  "version": "1.2.0",
  "notes": "Critical security update and Windows support.",
  "pub_date": "2025-01-07T12:00:00Z",
  "platforms": {
    "windows-x86_64": {
      "signature": "dW50cnVzdGVkIGNvbW1lbnQ6IHNpZ25hdHVyZSBmcm9tIHRhdXJpIHNlY3JldCBrZXkK...",
      "url": "https://your-domain.com/downloads/v1.2.0/app-setup.exe"
    },
    "linux-x86_64": {
      "signature": "...",
      "url": "..."
    }
  }
}
6.4 Tauri Configuration for UpdaterIn tauri.conf.json, you must enable the updater plugin, provide the public key, and set the endpoint URL.JSON{
  "plugins": {
    "updater": {
      "active": true,
      "endpoints": [
        "https://your-domain.com/updates/latest.json"
      ],
      "dialog": true,
      "pubkey": "YOUR_GENERATED_PUBLIC_KEY_HERE"
    }
  }
}
Note: The endpoints array allows for redundancy. Tauri will try them in order.7. Operational "Gotchas" and Windows SpecificsDeveloping for Windows introduces several platform-specific quirks that are distinct from the UNIX-like environments of macOS and Linux.7.1 The Console Window FlashBy default, Rust applications on Windows compiled in debug mode—and sometimes release mode if misconfigured—will spawn a command prompt window alongside the GUI application. This is unprofessional and confusing to users.Solution:Ensure the entry point file (src-tauri/src/main.rs) includes the windows_subsystem attribute. This directive tells the Windows linker to treat the executable as a GUI application, suppressing the standard output console.23Rust#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
  tauri::Builder::default()
   .run(tauri::generate_context!())
   .expect("error while running tauri application");
}
7.2 Antivirus False PositivesRust binaries are frequently flagged by heuristic analysis engines in antivirus software (including Windows Defender) as potential malware. This occurs because Rust binaries are statically linked and "packed" in a way that resembles some malware obfuscation techniques, and they often lack the extensive API call history of older C++ applications.Mitigation Strategy:Code Signing: This is the primary defense. Signed binaries are trusted significantly more than unsigned ones.Submission: Immediately after your first release, submit the signed .exe to the(https://www.microsoft.com/en-us/wdsi/filesubmission) portal. Select "Software Developer" and upload the file. This manually trains the Defender ML model that your file is safe.Wait: It typically takes 24-72 hours for the "cloud protection" reputation to propagate.7.3 File System Permissions and PathsWindows enforces a strict permission model and a legacy path length limit (MAX_PATH = 260 characters).Path Length: Deeply nested dependencies in node_modules or Rust build artifacts can exceed the 260-character limit, causing build failures. While Windows 10/11 supports long paths, not all tools respect this setting. Recommendation: Keep your project root directory close to the drive root (e.g., C:\dev\project) on your local machine to avoid issues.Capabilities: In Tauri v2, file system access is denied by default. You must explicitly configure capabilities in src-tauri/capabilities/default.json to allow access to specific directories like $APPDATA or $DOWNLOADS. Do not use absolute paths like C:/Users/... in configuration; always use Tauri's path variables.248. Conclusion and Strategic OutlookThe landscape for shipping independent desktop applications on Windows has matured significantly. The introduction of Azure Trusted Signing in 2024 removed the single largest friction point—hardware-based code signing—allowing for the creation of truly automated, cloud-native release pipelines.For a developer experienced in the Apple and Linux ecosystems, the Windows path is now convergent with modern DevOps practices. By adopting the NSIS installer format, leveraging Azure OIDC for keyless security, and utilizing WebView2's bootstrapper for efficient delivery, you can establish a Windows delivery pipeline that matches the reliability and automation of your existing platforms. The result is a secure, verifiable, and auto-updating application that respects the user's trust and the operating system's integrity requirements.

--- 

## Claude Research

# Shipping Tauri v2 to Windows: The Complete 2025 Production Guide

**Microsoft fundamentally changed SmartScreen in March 2024—EV certificates no longer bypass reputation warnings.** This levels the playing field between EV and OV certificates, making the cheaper OV path more attractive for indie developers. For a developer with zero Windows experience adding Windows to an existing cross-platform Tauri v2 app, the critical path involves: choosing a certificate authority with cloud HSM support, configuring Tauri's `signCommand` for CI/CD signing, selecting NSIS over WiX for cross-compilation compatibility, and understanding WebView2 runtime requirements. This guide provides step-by-step configuration with real YAML workflows, config files, and solutions for the gotchas that will otherwise cost you days.

## Understanding Windows code signing certificates in 2025

The Windows code signing landscape changed dramatically with two industry shifts. First, since **June 1, 2023**, all code signing certificates (not just EV) must store private keys on FIPS 140-2 Level 2 hardware—meaning USB tokens or cloud HSMs. Second, Microsoft's **March 2024 SmartScreen change** removed the OID that gave EV certificates instant reputation, making both certificate types build reputation organically through downloads.

**EV (Extended Validation)** certificates cost **$249-700/year**, require registered business status with typically 3+ years of verifiable history, and remain mandatory only for Windows kernel-mode drivers. **OV (Organization Validation)** certificates cost **$65-400/year**, are available to individuals, and now provide equivalent SmartScreen treatment to EV. For most Tauri applications distributed via direct download, OV certificates offer the best value.

The hardware token requirement creates a CI/CD problem—you cannot plug a USB token into GitHub Actions. Cloud HSM solutions solve this:

| Solution | Annual Cost | Setup Complexity | Best For |
|----------|-------------|------------------|----------|
| Microsoft Trusted Signing | ~$120 | Low | US/Canada businesses with 3+ years history |
| GlobalSign + Azure Key Vault | ~$320 | Medium | International or newer businesses |
| SSL.com eSigner | ~$1,265+ | Low | Developers wanting turnkey CI/CD |
| DigiCert KeyLocker | ~$500+ | Medium | Enterprise with existing DigiCert relationship |

**Microsoft Trusted Signing** at **$9.99/month** is the cheapest option but has strict eligibility: only US/Canada organizations with 3+ years of verifiable tax history, or US/Canada individual developers. For everyone else, the **GlobalSign + Azure Key Vault** combination provides EV-quality certificates for approximately **$320/year** total.

## Certificate authority selection and verification process

Start the certificate application **2-3 weeks before your planned release**—verification takes time, especially for EV certificates requiring organizational vetting.

**For individuals or new businesses**, SSL.com offers the smoothest path with OV certificates starting at **$65/year** and their eSigner cloud signing service. Required documents: government-issued ID, proof of physical address, and a verifiable phone number. Expect 1-3 business days for approval.

**For established businesses**, GlobalSign provides excellent Azure Key Vault integration at approximately €709 for a 3-year EV certificate (~$260/year). Required documents: business registration, government-issued ID, proof of physical address, verified phone callback, and for companies under 3 years, a lawyer/accountant opinion letter. Expect 1-2 weeks for approval.

**Dramatically speed up verification** by obtaining a D-U-N-S number from Dun & Bradstreet before applying—CAs use this for instant business verification, reducing approval time from weeks to days.

Common rejection reasons include: phone numbers not publicly listed in trusted directories, business information mismatching government records, and documents requiring notarized English translation. Prepare these before applying.

## Configuring Tauri v2 for Windows builds

Create a Windows-specific configuration file at `src-tauri/tauri.windows.conf.json` that merges with your main config:

```json
{
  "bundle": {
    "windows": {
      "webviewInstallMode": {
        "type": "embedBootstrapper",
        "silent": true
      },
      "nsis": {
        "installMode": "currentUser",
        "displayLanguageSelector": false
      },
      "digestAlgorithm": "sha256",
      "timestampUrl": "http://timestamp.digicert.com"
    }
  }
}
```

**Choose NSIS over WiX** for your installer format. NSIS produces `-setup.exe` files that support cross-compilation from Linux/macOS, ARM64 builds, and multi-language installers. WiX produces `.msi` files but only builds on Windows and lacks ARM64 support. Build NSIS installers with `cargo tauri build --bundles nsis`.

The `installMode` setting determines elevation requirements. Use **`currentUser`** (the default) to install to `%LOCALAPPDATA%` without requiring administrator privileges—this provides the smoothest user experience. Use `perMachine` only if you need system-wide installation in `C:\Program Files`.

For WebView2, **`embedBootstrapper`** is recommended over the default `downloadBootstrapper` because it adds only 1.8MB to your installer while providing better Windows 7 compatibility and handling TLS 1.2 edge cases. The `offlineInstaller` option bundles the full 127MB runtime for air-gapped environments.

Configure the Windows application manifest in `src-tauri/build.rs` for DPI awareness and proper UI rendering:

```rust
fn main() {
    let mut windows = tauri_build::WindowsAttributes::new();
    windows = windows.app_manifest(r#"
        <assembly xmlns="urn:schemas-microsoft-com:asm.v1" manifestVersion="1.0">
            <application xmlns="urn:schemas-microsoft-com:asm.v3">
                <windowsSettings>
                    <dpiAware xmlns="http://schemas.microsoft.com/SMI/2005/WindowsSettings">true/pm</dpiAware>
                    <dpiAwareness xmlns="http://schemas.microsoft.com/SMI/2016/WindowsSettings">PerMonitorV2, PerMonitor</dpiAwareness>
                </windowsSettings>
            </application>
            <trustInfo xmlns="urn:schemas-microsoft-com:asm.v3">
                <security>
                    <requestedPrivileges>
                        <requestedExecutionLevel level="asInvoker" uiAccess="false" />
                    </requestedPrivileges>
                </security>
            </trustInfo>
        </assembly>
    "#);
    
    let attrs = tauri_build::Attributes::new().windows_attributes(windows);
    tauri_build::try_build(attrs).expect("failed to run build script");
}
```

## GitHub Actions workflow for signed Windows builds

The complete production workflow handles certificate signing via Azure Key Vault and produces both x64 and ARM64 builds:

```yaml
name: 'Build and Release'
on:
  push:
    tags:
      - 'v*'

jobs:
  build-windows:
    runs-on: windows-latest
    permissions:
      contents: write
    strategy:
      fail-fast: false
      matrix:
        include:
          - target: x86_64-pc-windows-msvc
            args: ''
          - target: aarch64-pc-windows-msvc
            args: '--target aarch64-pc-windows-msvc'
    
    steps:
      - uses: actions/checkout@v4
      
      - name: Setup Node.js
        uses: actions/setup-node@v4
        with:
          node-version: 'lts/*'
          cache: 'npm'
      
      - name: Install Rust stable
        uses: dtolnay/rust-toolchain@stable
        with:
          targets: ${{ matrix.target }}
      
      - name: Rust cache
        uses: swatinem/rust-cache@v2
        with:
          workspaces: './src-tauri -> target'
          shared-key: ${{ matrix.target }}
      
      - name: Install AzureSignTool
        run: dotnet tool install --global AzureSignTool
      
      - name: Install frontend dependencies
        run: npm ci
      
      - name: Build Tauri app
        uses: tauri-apps/tauri-action@v0
        env:
          GITHUB_TOKEN: ${{ secrets.GITHUB_TOKEN }}
          AZURE_KEY_VAULT_URI: ${{ secrets.AZURE_KEY_VAULT_URI }}
          AZURE_CLIENT_ID: ${{ secrets.AZURE_CLIENT_ID }}
          AZURE_CLIENT_SECRET: ${{ secrets.AZURE_CLIENT_SECRET }}
          AZURE_TENANT_ID: ${{ secrets.AZURE_TENANT_ID }}
          AZURE_CERT_NAME: ${{ secrets.AZURE_CERT_NAME }}
        with:
          tagName: v__VERSION__
          releaseName: 'v__VERSION__'
          releaseBody: 'See assets for downloads.'
          releaseDraft: true
          args: ${{ matrix.args }} --bundles nsis
```

For Azure Key Vault signing, configure `signCommand` in your `tauri.conf.json`:

```json
{
  "bundle": {
    "windows": {
      "signCommand": "AzureSignTool sign -kvu %AZURE_KEY_VAULT_URI% -kvi %AZURE_CLIENT_ID% -kvt %AZURE_TENANT_ID% -kvs %AZURE_CLIENT_SECRET% -kvc %AZURE_CERT_NAME% -tr http://timestamp.digicert.com -td sha256 %1"
    }
  }
}
```

**Required GitHub Secrets** for Azure Key Vault integration:
- `AZURE_KEY_VAULT_URI`: Your Key Vault URL (e.g., `https://your-vault.vault.azure.net`)
- `AZURE_CLIENT_ID`: App Registration client ID
- `AZURE_CLIENT_SECRET`: App Registration secret
- `AZURE_TENANT_ID`: Azure AD tenant ID
- `AZURE_CERT_NAME`: Certificate name in Key Vault

For **SSL.com eSigner** as an alternative, use their GitHub Action:

```yaml
- name: Sign with SSL.com eSigner
  uses: sslcom/esigner-codesign@develop
  with:
    command: sign
    username: ${{ secrets.ES_USERNAME }}
    password: ${{ secrets.ES_PASSWORD }}
    credential_id: ${{ secrets.CREDENTIAL_ID }}
    totp_secret: ${{ secrets.ES_TOTP_SECRET }}
    file_path: src-tauri/target/release/bundle/nsis/*.exe
```

## Setting up Azure Key Vault for EV signing

Create an Azure Key Vault with **Premium tier** (required for HSM-backed keys meeting FIPS 140-2 Level 2):

1. In Azure Portal, create a Key Vault with Premium pricing tier
2. Under "Certificates," select "Generate/Import" → "Generate"
3. Choose **RSA-HSM** key type (not RSA) for compliance
4. Complete the certificate signing request (CSR) generation
5. Download the CSR and submit to your Certificate Authority
6. Once CA issues your certificate, merge it back into Key Vault

Create an **App Registration** for GitHub Actions authentication:

1. In Azure AD, create a new App Registration
2. Generate a client secret under "Certificates & secrets"
3. In Key Vault, assign the app these roles via Access Control (IAM):
   - **Key Vault Crypto User** (for signing operations)
   - **Key Vault Certificate User** (for certificate access)

This setup typically takes 1-2 hours once you have your certificate from the CA.

## Navigating SmartScreen and Windows Defender

**SmartScreen reputation builds gradually** regardless of certificate type. Newly signed applications will display warnings like "Windows protected your PC" with your company name visible (unlike unsigned apps showing "Unknown publisher"). Users must click "More info" → "Run anyway" to proceed.

To accelerate reputation building, submit your application to Microsoft's malware analysis at [microsoft.com/wdsi/filesubmission](https://www.microsoft.com/en-us/wdsi/filesubmission). This doesn't guarantee approval but helps Microsoft's systems recognize your software faster. After approximately **several thousand downloads** without incident reports, warnings typically disappear.

**Windows Defender false positives** plague NSIS installers specifically because malware authors frequently use NSIS. Common false detections include `Trojan:Script/Wacatac.B!ml` and `Trojan:Win32/CryptInject!ml`. To minimize false positives:

- Code sign your application—this significantly reduces false positive rates
- Consider MSI over NSIS if false positives persist (MSI raises fewer flags)
- Submit false positives to Microsoft and other AV vendors through their official channels
- Verify your dependencies haven't been compromised using `npm audit` and `cargo audit`

If users report AV blocks, document the issue and provide whitelisting instructions. Upload your installer to VirusTotal to identify which engines flag it—if major vendors (Norton, ESET, Kaspersky) mark it clean, the flags are likely false positives from heuristic engines.

## Windows platform gotchas that will bite you

**WebView2 is mandatory**—your app will not launch without it. Windows 11 and Windows 10 (April 2018+) include it pre-installed. For older systems, your installer handles WebView2 installation based on your `webviewInstallMode` setting. The `embedBootstrapper` option provides the most reliable cross-version behavior.

**Path length limits** remain relevant on Windows. The traditional 260-character MAX_PATH limit can break builds when `node_modules` creates deeply nested directories. Keep your project path short (e.g., `C:\dev\myapp` not `C:\Users\Username\Documents\Projects\Company\Application\Development\myapp`).

**DPI scaling issues** cause blurry rendering on high-DPI displays, especially at 125%, 150%, or 200% scaling. The manifest configuration in `build.rs` shown earlier enables Per-Monitor V2 DPI awareness. Test your application on high-DPI displays and multi-monitor setups with different scaling factors—windows can resize unexpectedly when dragged between monitors.

**Deep linking behaves differently than macOS**. On Windows, custom protocol handlers spawn a new application instance with the URL as a command-line argument rather than sending an event to the running instance. Use Tauri's `single-instance` plugin combined with `deep-link` to handle this:

```rust
// During development only, register deep link schemes
#[cfg(all(debug_assertions, windows))]
app.deep_link().register_all()?;
```

**Auto-updates require elevation** if installed with `perMachine` mode. Updates fail silently with error code 740 ("The requested operation requires elevation") unless the app runs as administrator. Use `currentUser` installation mode to avoid this problem entirely.

**File associations** don't trigger events—they spawn new instances with file paths as CLI arguments. Handle this by checking `std::env::args()` at startup rather than relying on event listeners.

## Verifying your signed builds

After building, verify signatures using PowerShell:

```powershell
# Check signature details
Get-AuthenticodeSignature ".\target\release\bundle\nsis\MyApp_1.0.0_x64-setup.exe"

# Detailed verification
signtool verify /pa /v ".\target\release\bundle\nsis\MyApp_1.0.0_x64-setup.exe"
```

A properly signed binary shows `Status: Valid` with your organization name under `SignerCertificate`. If verification fails, common causes include: incorrect certificate thumbprint, missing timestampUrl (signatures expire with the certificate), or certificate not properly imported to the Windows certificate store.

**Test on multiple Windows versions** before release. Windows 11, Windows 10 (latest), and Windows 10 (1803) represent the critical test matrix for WebView2 compatibility. For Windows 7 testing, use the `windows7-compat` feature flag with notification plugins and ensure you're using `embedBootstrapper` for WebView2 installation.

## Critical migration note for v1 users

If migrating from Tauri v1, add this setting immediately to preserve user data:

```json
{
  "app": {
    "windows": [{
      "useHttpsScheme": true
    }]
  }
}
```

Tauri v2 changed the Windows frontend origin from `https://tauri.localhost` to `http://tauri.localhost`. Without this setting, **IndexedDB, LocalStorage, and cookies are wiped** because browsers treat these as different origins. Run `npx @tauri-apps/cli migrate` to automate most migration steps.

## Conclusion

Shipping Tauri v2 to Windows requires navigating code signing infrastructure, installer configuration, and platform-specific behaviors that differ significantly from macOS and Linux. The most impactful decisions are: **choosing Azure Key Vault or SSL.com eSigner** for CI/CD-compatible code signing, **using NSIS with `currentUser` installation** for the smoothest user experience, and **embedding the WebView2 bootstrapper** for reliable cross-version compatibility.

Start the certificate application process early—it's the longest lead time item. Configure signing in your CI/CD pipeline before your first release to avoid last-minute scrambling. And expect SmartScreen warnings initially regardless of certificate type; build reputation through consistent releases and user downloads rather than searching for a technical bypass that no longer exists.

----

## Grok Research

### Key Points
- Adding Windows support to an existing Tauri v2 app is straightforward: enable Windows-specific bundle targets (e.g., `nsi`, `msi`, or `msix`) in `tauri.conf.json`—no major code changes needed since your app already runs on other platforms.
- Production-grade distribution requires **code signing** with a valid certificate (EV recommended for instant Microsoft SmartScreen reputation) to avoid security warnings.
- Builds for Windows installers (especially MSI) are easiest on a Windows machine, but NSIS can be cross-compiled; use GitHub Actions with `windows-latest` runners for reliable CI/CD.
- Distribution options include direct downloads (NSIS/MSI installers) or submission to the Microsoft Store (via MSIX packaging), with the Store offering broader reach but stricter requirements.
- Common gotchas include handling WebView2 runtime installation, potential Windows Defender flags on unsigned apps, and architecture-specific builds (x64, ARM64, or 32-bit).

### Adding Windows Target
Since your Tauri v2 project is already set up, Windows support is enabled by default if the bundler detects the platform. To explicitly include Windows bundles, update `tauri.conf.json`:

```json
{
  "bundle": {
    "targets": ["nsis", "msi", "msix", "all"],  // "all" includes Windows options
    "windows": {
      // Optional customizations here
    }
  }
}
```

No frontend or Rust code changes are typically needed, as your existing web assets will render via WebView2 on Windows.

### Prerequisites for Windows
For local development/building on Windows:
- Install Microsoft C++ Build Tools (via Visual Studio Installer, select "Desktop development with C++").
- Install Microsoft Edge WebView2 Evergreen Bootstrapper.
- For MSI bundles: Enable the "VBSCRIPT" optional Windows feature.

Add Rust targets if needed:
```bash
rustup target add x86_64-pc-windows-msvc  # Default x64
rustup target add aarch64-pc-windows-msvc  # ARM64
rustup target add i686-pc-windows-msvc     # 32-bit
```

### Building the App
Run `cargo tauri build` (or `tauri build`) on a Windows machine for full support:
- Produces NSIS (.exe setup) and/or MSI installers.
- MSI requires Windows (WiX toolset limitation).
- NSIS supports cross-compilation from Linux/macOS with tools like `cargo-xwin`.

For specific targets:
```bash
tauri build --target x86_64-pc-windows-msvc
```

WebView2 handling is critical—configure in `tauri.conf.json` under `bundle.windows.webviewInstallMode` to avoid runtime failures (details below).

### Code Signing for Production
Unsigned apps trigger SmartScreen/Defender warnings. Use `signtool` (built into Tauri bundler).

1. Acquire a code signing certificate:
   - OV (basic) or EV (Extended Validation, ~$300–500/year, instant reputation).
   - Recommended providers: DigiCert, Sectigo.

2. Prepare .pfx file and configure in `tauri.conf.json`:
```json
{
  "bundle": {
    "windows": {
      "certificateThumbprint": "YOUR_THUMBPRINT",
      "digestAlgorithm": "sha256",
      "timestampUrl": "http://timestamp.example.com"
    }
  }
}
```

Tauri signs automatically during build. EV certificates eliminate initial warnings.

### CI/CD with GitHub Actions
Use the official `tauri-apps/tauri-action@v1` for automated builds/releases.

Example workflow snippet for Windows:
```yaml
jobs:
  build-windows:
    runs-on: windows-latest
    steps:
      - uses: actions/checkout@v4
      - uses: actions/setup-node@v4
        with: { node-version: lts/* }
      - uses: dtolnay/rust-toolchain@stable
      - run: yarn install  # Or your frontend setup
      - uses: tauri-apps/tauri-action@v1
        env:
          GITHUB_TOKEN: ${{ secrets.GITHUB_TOKEN }}
        with:
          args: ''  # Add --target if needed
```

For signing in CI: Store .pfx as base64 secret, decode/import before build. Trigger on tags for releases—artifacts upload automatically.

### Distribution Options
- **Direct Download**: Bundle NSIS (.exe) or MSI. Host on GitHub Releases or your site. Preferred for full control.
- **Microsoft Store**: Bundle as MSIX (`targets: ["msix"]`), sign, enroll in Microsoft Partner Center (~$19 individual/$99 company), reserve app name, submit via dashboard. Offers auto-updates and discovery but requires policy compliance (e.g., no external updaters unless configured).

### Common Gotchas
- WebView2 runtime: Most users have it, but embed bootstrapper to avoid failures.
- Unsigned apps often flagged by Defender/SmartScreen initially.
- MSI only builds on Windows; use NSIS for cross-platform CI.
- Test on multiple architectures and older Windows versions if needed.
- Per-user vs. per-machine installs affect paths and privileges.

---

### Comprehensive Guide to Shipping Tauri v2 Apps to Windows

Tauri v2 (stable as of late 2024, with ongoing updates through 2025) provides robust Windows support, leveraging WebView2 for rendering and native bundlers for installers. Since your app already targets mobile/desktop/web, adding Windows focuses on bundling, signing, and distribution. This guide covers a complete production workflow based on official documentation as of late 2025.

#### Enabling and Configuring Windows Bundles
Tauri v2's bundler supports multiple Windows targets out of the box. In `tauri.conf.json`:

```json
{
  "bundle": {
    "active": true,
    "targets": ["nsis", "msi", "msix"],  // Or "all"
    "identifier": "com.yourapp.dev",     // Required for MSIX/Store
    "windows": {
      "installMode": "currentUser",      // Or "perMachine"/"both"
      "webviewInstallMode": {            // Critical for reliability
        "type": "embedBootstrapper"      // Recommended over download
      }
    }
  }
}
```

Bundle types comparison:

| Bundle Type | File Output       | Cross-Compile? | Size Impact      | Best For                  | Notes                          |
|-------------|-------------------|----------------|------------------|---------------------------|--------------------------------|
| NSIS       | .exe setup       | Yes           | Minimal         | Direct downloads         | Customizable hooks/scripts    |
| MSI        | .msi installer   | No (Windows only) | Minimal      | Enterprise/per-machine   | WiX-based, multi-language     |
| MSIX       | .msix package    | Yes           | Minimal         | Microsoft Store          | Requires package identity     |

#### Development and Build Prerequisites
On Windows machines:
- Microsoft C++ Build Tools.
- WebView2 Evergreen Bootstrapper.
- VBScript feature (for MSI).

Rust targets for non-default architectures must be added manually.

#### Local Building Process
```bash
cargo tauri build
```
Outputs to `target/release/bundle/`. Test installers thoroughly—simulate fresh Windows installs to verify WebView2 handling.

WebView2 modes (configure under `bundle.windows.webviewInstallMode`):

| Mode                | Internet Required | Size Increase | Offline Support | Recommendation                  |
|---------------------|-------------------|---------------|-----------------|---------------------------------|
| downloadBootstrapper| Yes              | ~0MB         | No             | Default, but avoid for Win7    |
| embedBootstrapper   | No               | ~1.8MB       | Partial        | Best balance                   |
| offlineInstaller    | No               | ~127MB       | Yes            | Full offline                   |
| fixedRuntime        | No               | ~180MB       | Yes            | Specific version control       |
| skip                | No               | ~0MB         | No             | Avoid—app crashes if missing   |

For fixed runtime: Download CAB from Microsoft, extract to `src-tauri/resources/core/`, and set `webviewRuntimePath`.

#### Production Code Signing
Essential for trust:
- Standard OV cert: Basic validation.
- EV cert: Hardware token often required, immediate SmartScreen trust.

Process:
1. Purchase cert → Export as .pfx.
2. Import to Windows store (local) or use Azure Key Vault for CI.
3. Config thumbprint/digest/timestamp in `tauri.conf.json`.
4. Build—Tauri invokes `signtool`.

In CI: Base64-encode .pfx, store as secret, decode/import in workflow.

#### GitHub Actions CI/CD Setup
Full multi-platform example (focus on Windows job):

```yaml
name: Release
on:
  push:
    tags: [ "v*.*.*" ]

jobs:
  build:
    strategy:
      matrix:
        platform: [windows-latest]  # Add macos/linux as needed
    runs-on: ${{ matrix.platform }}
    steps:
      - uses: actions/checkout@v4
      - uses: actions/setup-node@v4
        with: { node-version: lts/* }
      - uses: dtolnay/rust-toolchain@stable
      - run: yarn install  # Frontend deps
      # Signing prep (if needed)
      - name: Import Certificate
        if: matrix.platform == 'windows-latest'
        run: certutil -decode cert.txt cert.pfx && Import-PfxCertificate -FilePath cert.pfx -Password ...
        env:
          CERT_BASE64: ${{ secrets.WINDOWS_CERT }}
      - uses: tauri-apps/tauri-action@v1
        with:
          tagName: ${{ github.ref_name }}
          releaseName: "App ${{ github.ref_name }}"
          args: --target x86_64-pc-windows-msvc  # If specific
        env:
          GITHUB_TOKEN: ${{ secrets.GITHUB_TOKEN }}
```

Outputs include release IDs and artifact paths for further automation.

#### Distribution and Submission
- **Direct**: Upload NSIS/MSI to GitHub Releases. Pair with Tauri's updater plugin for auto-updates.
- **Microsoft Store**:
  1. Enroll at Partner Center (developer account required).
  2. Reserve app name.
  3. Bundle as MSIX (`targets: ["msix"]`).
  4. Sign with valid cert.
  5. Submit package via dashboard—includes age ratings, screenshots, compliance checks.
  6. Store handles updates/distribution.

Store pros: Visibility, trusted installation. Cons: Approval time, no external updaters.

#### Platform-Specific Gotchas and Best Practices
- Defender/SmartScreen: Unsigned → high false positives; EV cert mitigates.
- Architecture support: Test x64 (default), ARM64 (growing), 32-bit (legacy).
- Windows 7: Use `embedBootstrapper` + `windows7-compat` feature.
- Installer customizations: Use WiX fragments or NSIS hooks for extras (e.g., VC++ redist).
- Internationalization: Configure languages for multi-locale installers.
- Testing: Verify on clean VMs; check UAC prompts, paths (%LOCALAPPDATA% vs. Program Files).

This workflow ensures a polished, production-ready Windows release matching your existing cross-platform quality.

**Key Citations**
- https://v2.tauri.app/distribute/windows-installer/
- https://v2.tauri.app/distribute/sign/windows/
- https://v2.tauri.app/distribute/pipelines/github/
- https://v2.tauri.app/start/prerequisites/
- https://github.com/tauri-apps/tauri-action
- https://v2.tauri.app/distribute/microsoft-store/
