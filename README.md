# OpenSecret React SDK

This is a React SDK for the [OpenSecret](https://opensecret.cloud) platform.

🚧 We're currently in preview mode, please contact us at team@opensecret.cloud for the preview URL and getting started info 🚧

## Installation

```bash
npm install @opensecret/react
```

## Usage

Wrap your application in the `OpenSecretProvider` component and provide:
1. The URL of your OpenSecret backend
2. Your project's client ID (a UUID that identifies your project)

```tsx
import { OpenSecretProvider } from "@opensecret/react";

function App() {
  return (
    <OpenSecretProvider 
      apiUrl="{URL}"
      clientId="{PROJECT_UUID}"
    >
      <App />
    </OpenSecretProvider>
  );
}
```

Now import the `useOpenSecret` hook and use it to access the OpenSecret API:

```tsx
import { useOpenSecret } from "@opensecret/react";

function App() {
  const os = useOpenSecret();

  return (
    <div>
      <button onClick={() => os.signIn("email", "password")}>Sign In</button>
      <button onClick={() => os.signUp("name", "email", "password")}>Sign Up</button>
      <button onClick={() => os.signOut()}>Sign Out</button>
      <button onClick={() => os.get("key")}>Get Value</button>
      <button onClick={() => os.put("key", "value")}>Put Value</button>
      <button onClick={() => os.list()}>List Values</button>
      <button onClick={() => os.del("key")}>Delete Value</button>
    </div>
  );
}
```

## API Reference

### `OpenSecretProvider`

The `OpenSecretProvider` component is the main entry point for the SDK. It requires two props:
- `apiUrl`: The URL of your OpenSecret backend
- `clientId`: A UUID that identifies your project/tenant. This is used to scope user accounts and data to your specific project.

```tsx
<OpenSecretProvider 
  apiUrl="{URL}"
  clientId="{PROJECT_UUID}"
>
  <App />
</OpenSecretProvider>
```

### `useOpenSecret`

The `useOpenSecret` hook provides access to the OpenSecret API. It returns an object with the following methods:

#### Authentication Methods
- `signIn(email: string, password: string): Promise<void>`: Signs in a user with the provided email and password.
- `signUp(email: string, password: string, inviteCode: string, name?: string): Promise<void>`: Signs up a new user with the provided email, password, invite code, and optional name.
- `signInGuest(id: string, password: string): Promise<void>`: Signs in a guest user with their ID and password. Guest accounts are scoped to the project specified by `clientId`.
- `signUpGuest(password: string, inviteCode: string): Promise<LoginResponse>`: Creates a new guest account with just a password and invite code. Returns a response containing the guest's ID, access token, and refresh token. The guest account will be associated with the project specified by `clientId`.
- `signOut(): Promise<void>`: Signs out the current user.

#### Key-Value Storage Methods
- `get(key: string): Promise<string | undefined>`: Retrieves the value associated with the provided key.
- `put(key: string, value: string): Promise<string>`: Stores the provided value with the provided key.
- `list(): Promise<KVListItem[]>`: Retrieves all key-value pairs stored by the user.
- `del(key: string): Promise<void>`: Deletes the value associated with the provided key.

#### Account Management Methods
- `refetchUser(): Promise<void>`: Refreshes the user's authentication state.
- `changePassword(currentPassword: string, newPassword: string): Promise<void>`: Changes the user's password.
- `generateThirdPartyToken(audience?: string): Promise<{ token: string }>`: Generates a JWT token for use with third-party services. If an audience is provided, it can be any valid URL. If omitted, a token with no audience restriction will be generated.

#### Cryptographic Methods

##### Key Derivation Options

For cryptographic operations, the SDK supports a `KeyOptions` object with the following structure:

```typescript
type KeyOptions = {
  /** 
   * BIP-85 derivation path to derive a child mnemonic
   * Example: "m/83696968'/39'/0'/12'/0'"
   */
  seed_phrase_derivation_path?: string;
  
  /**
   * BIP-32 derivation path to derive a child key from the master (or BIP-85 derived) seed
   * Example: "m/44'/0'/0'/0/0"
   */
  private_key_derivation_path?: string;
};
```

All cryptographic methods accept this `KeyOptions` object as a parameter to specify derivation options.

##### Methods

- `getPrivateKey(key_options?: KeyOptions): Promise<{ mnemonic: string }>`: Retrieves the user's private key mnemonic phrase.
  - If no key_options are provided, returns the master mnemonic
  - If `seed_phrase_derivation_path` is provided, returns a BIP-85 derived child mnemonic
  - For BIP-85, the path format is typically `m/83696968'/39'/0'/12'/0'` where:
    - `83696968'` is the hardened BIP-85 application number (ASCII for "BIPS")
    - `39'` is the hardened BIP-39 application (for mnemonic derivation)
    - `0'` is the hardened coin type (0' for Bitcoin)
    - `12'` is the hardened entropy in words (12-word mnemonic)
    - `0'` is the hardened index (can be incremented to generate different phrases)

- `getPrivateKeyBytes(key_options?: KeyOptions): Promise<{ private_key: string }>`: Retrieves the private key bytes with flexible derivation options.
  - Supports multiple derivation approaches:
  
  1. Master key only (no parameters)
     - Returns the master private key bytes
  
  2. BIP-32 derivation only
     - Uses path format like `m/44'/0'/0'/0/0`
     - Supports both absolute (starting with "m/") and relative paths
     - Supports hardened derivation using either ' or h notation
  
  3. BIP-85 derivation only
     - Derives a child mnemonic from the master seed using BIP-85
     - Then returns the master private key of that derived seed
  
  4. Combined BIP-85 and BIP-32 derivation
     - First derives a child mnemonic via BIP-85
     - Then applies BIP-32 derivation to that derived seed

  Common BIP-32 paths:
  - BIP44 (Legacy): `m/44'/0'/0'/0/0`
  - BIP49 (SegWit): `m/49'/0'/0'/0/0`
  - BIP84 (Native SegWit): `m/84'/0'/0'/0/0`
  - BIP86 (Taproot): `m/86'/0'/0'/0/0`

- `getPublicKey(algorithm: 'schnorr' | 'ecdsa', key_options?: KeyOptions): Promise<PublicKeyResponse>`: Retrieves the user's public key for the specified signing algorithm and derivation options.
  
  The derivation paths determine which key is used to generate the public key:
  - Master key (no parameters)
  - BIP-32 derived key
  - BIP-85 derived key
  - Combined BIP-85 + BIP-32 derived key
  
  Supports two algorithms:
  - `'schnorr'`: For Schnorr signatures
  - `'ecdsa'`: For ECDSA signatures

- `signMessage(messageBytes: Uint8Array, algorithm: 'schnorr' | 'ecdsa', key_options?: KeyOptions): Promise<SignatureResponse>`: Signs a message using the specified algorithm and derivation options.
  
  Example message preparation:
  ```typescript
  // From string
  const messageBytes = new TextEncoder().encode("Hello, World!");
  
  // From hex
  const messageBytes = new Uint8Array(Buffer.from("deadbeef", "hex"));
  ```

- `encryptData(data: string, key_options?: KeyOptions): Promise<{ encrypted_data: string }>`: Encrypts arbitrary string data using the user's private key with flexible derivation options.
  
  Examples:
  ```typescript
  // Encrypt with master key
  const { encrypted_data } = await os.encryptData("Secret message");
  
  // Encrypt with BIP-32 derived key
  const { encrypted_data } = await os.encryptData("Secret message", {
    private_key_derivation_path: "m/44'/0'/0'/0/0"
  });
  
  // Encrypt with BIP-85 derived key
  const { encrypted_data } = await os.encryptData("Secret message", {
    seed_phrase_derivation_path: "m/83696968'/39'/0'/12'/0'"
  });
  
  // Encrypt with combined BIP-85 and BIP-32 derivation
  const { encrypted_data } = await os.encryptData("Secret message", {
    seed_phrase_derivation_path: "m/83696968'/39'/0'/12'/0'",
    private_key_derivation_path: "m/44'/0'/0'/0/0"
  });
  ```

- `decryptData(encryptedData: string, key_options?: KeyOptions): Promise<string>`: Decrypts data that was previously encrypted with the user's key.
  
  IMPORTANT: You must use the exact same derivation options for decryption that were used for encryption.
  
  Examples:
  ```typescript
  // Decrypt with master key
  const decrypted = await os.decryptData(encrypted_data);
  
  // Decrypt with BIP-32 derived key
  const decrypted = await os.decryptData(encrypted_data, {
    private_key_derivation_path: "m/44'/0'/0'/0/0"
  });
  
  // Decrypt with BIP-85 derived key
  const decrypted = await os.decryptData(encrypted_data, {
    seed_phrase_derivation_path: "m/83696968'/39'/0'/12'/0'"
  });
  
  // Decrypt with combined BIP-85 and BIP-32 derivation
  const decrypted = await os.decryptData(encrypted_data, {
    seed_phrase_derivation_path: "m/83696968'/39'/0'/12'/0'",
    private_key_derivation_path: "m/44'/0'/0'/0/0"
  });
  ```

##### Implementation Examples

1. Basic Usage with Default Master Key

```typescript
// Get the master mnemonic
const { mnemonic } = await os.getPrivateKey();

// Get the master private key bytes
const { private_key } = await os.getPrivateKeyBytes();

// Sign with the master key
const signature = await os.signMessage(messageBytes, 'ecdsa');
```

2. Using BIP-32 Derivation Only

```typescript
// Get private key bytes using BIP-32 derivation
const { private_key } = await os.getPrivateKeyBytes({
  private_key_derivation_path: "m/44'/0'/0'/0/0"
});

// Sign with a derived key
const signature = await os.signMessage(messageBytes, 'ecdsa', {
  private_key_derivation_path: "m/44'/0'/0'/0/0"
});
```

3. Using BIP-85 Derivation Only

```typescript
// Get a child mnemonic phrase derived via BIP-85
const { mnemonic } = await os.getPrivateKey({
  seed_phrase_derivation_path: "m/83696968'/39'/0'/12'/0'"
});

// Get master private key of a BIP-85 derived seed
const { private_key } = await os.getPrivateKeyBytes({
  seed_phrase_derivation_path: "m/83696968'/39'/0'/12'/0'"
});
```

4. Using Both BIP-85 and BIP-32 Derivation

```typescript
// Get private key bytes derived through BIP-85 and then BIP-32
const { private_key } = await os.getPrivateKeyBytes({
  seed_phrase_derivation_path: "m/83696968'/39'/0'/12'/0'",
  private_key_derivation_path: "m/44'/0'/0'/0/0"
});

// Sign a message with a key derived through both methods
const signature = await os.signMessage(messageBytes, 'schnorr', {
  seed_phrase_derivation_path: "m/83696968'/39'/0'/12'/0'",
  private_key_derivation_path: "m/44'/0'/0'/0/0"
});
```

5. Encryption/Decryption with Derived Keys

```typescript
// Encrypt with a BIP-85 derived key
const { encrypted_data } = await os.encryptData("Secret message", {
  seed_phrase_derivation_path: "m/83696968'/39'/0'/12'/0'"
});

// Decrypt using the same derivation path
const decrypted = await os.decryptData(encrypted_data, {
  seed_phrase_derivation_path: "m/83696968'/39'/0'/12'/0'"
});
```

### AI Integration

To get encrypted-to-the-gpu AI chat we provide a special version of `fetch` (`os.aiCustomFetch`) that handles all the encryption. Because we require the user to be logged in, and do the encryption client-side, this is safe to call from the client.

The easiest way to use this is through the OpenAI client:

```bash
npm install openai
```

```typescript
import OpenAI from "openai";
import { useOpenSecret } from "@opensecret/react";

//...

// In a component
const os = useOpenSecret();

const openai = new OpenAI({
  baseURL: `${os.apiUrl}/v1/`,
  dangerouslyAllowBrowser: true,
  apiKey: "api-key-doesnt-matter", // The actual API key is handled by OpenSecret
  defaultHeaders: {
    "Accept-Encoding": "identity",
    "Content-Type": "application/json",
  },
  fetch: os.aiCustomFetch, // Use OpenSecret's encrypted fetch
});

//...
```

You can now use the OpenAI client as normal. (Right now only streaming responses are supported.) See the example in `src/AI.tsx` in the SDK source code for a complete example.

For an alternative approach using custom fetch directly, see the implementation in `src/lib/ai.test.ts` in the SDK source code.

### Library development

This library uses Bun 1.3.5 for development. The Nix shell and CI pin the same
version. New Bun resolutions quarantine releases for seven days; routine
installs are frozen to the committed lockfile and do not run lifecycle scripts.
Age gates do not revalidate versions already present in either lock format.

The `bun.lockb` files are authoritative. The retained npm lockfiles are
compatibility artifacts and must not be used for dependency updates; npm's age
gate requires npm 11.10 or newer and does not revalidate locked versions.

Enter the development shell and install the locked dependencies:

```bash
nix develop
bun install
```

To build the library, run the following command:

```bash
bun run build
```

To test the library, run the following command:

```bash
bun test --env-file .env.local
```

To test a specific file or test case:

```bash
bun test --test-name-pattern="Developer login and token storage" src/lib/test/integration/developer.test.ts --env-file .env.local
```

The build invokes the installed Vite CLI with Node because of the historical
[`vite-plugin-dts` Bun incompatibility](https://github.com/OpenSecretCloud/OpenSecret-SDK/issues/16).
It never downloads a missing CLI during the build.

To pack the library (for publishing) run the following command:

```bash
bun run pack
```

To deploy: 

```bash
NPM_CONFIG_TOKEN=$NPM_CONFIG_TOKEN bun publish --access public
```

### Documentation Development

The SDK documentation is built using [Docusaurus](https://docusaurus.io/), a modern documentation framework. The documentation is automatically generated from TypeScript code comments and supplemented with manually written guides.

#### Getting Started with Documentation

To start the documentation development server:

```bash
bun run docs:dev
```

This will start the Docusaurus development server and open the documentation in your browser at http://localhost:3000/. The server supports hot-reloading, so any changes you make to the documentation will be immediately reflected in the browser.

#### Building Documentation

To build the documentation for production:

```bash
bun run docs:build
```

This will generate static HTML, JavaScript, and CSS files in the `website/build` directory.

To serve the built documentation locally:

```bash
bun run docs:serve
```

#### Documentation Structure

The documentation is organized into the following directories:

- `/website/docs/` - Contains all manual documentation files
  - `index.md` - The documentation landing page
  - `/guides/` - Step-by-step guides for using the SDK
  - `/api/` - API reference documentation (mostly auto-generated)

#### API Reference Documentation

The API reference documentation is automatically generated from TypeScript code comments using [TypeDoc](https://typedoc.org/). To update the API documentation:

1. Write proper JSDoc comments in the TypeScript source code
2. Run `bun run docs:build` to regenerate the documentation

Important notes for API documentation:

- Use standard JSDoc syntax for documenting parameters, return types, and descriptions
- For Markdown in JSDoc comments, be aware that backticks (`) must be properly escaped
- For code examples with apostrophes (e.g., BIP paths like `m/44'/0'/0'/0/0`), use backslash escaping: `m/44\'/0\'/0\'/0/0`

#### Adding New Guides

To add a new guide:

1. Create a new Markdown file in the `/website/docs/guides/` directory
2. Add frontmatter at the top of the file:
   ```md
   ---
   title: Your Guide Title
   sidebar_position: X  # Controls the order in the sidebar
   ---
   ```
3. Update the sidebar configuration in `/website/sidebars.ts` if needed

#### Customizing the Documentation

The main configuration files for Docusaurus are:

- `/website/docusaurus.config.ts` - Main Docusaurus configuration
- `/website/sidebars.ts` - Sidebar configuration
- `/website/typedoc.json` - TypeDoc configuration for API docs

To customize the appearance:

- Edit `/website/src/css/custom.css` for global styles
- Create or modify components in `/website/src/components/`

#### Deployment

The documentation can be deployed to various platforms like GitHub Pages, Netlify, or Vercel. For CloudFlare Pages deployment, as mentioned in our guideline:

1. In CloudFlare Pages, create a new project connected to your GitHub repo
2. Set `BUN_VERSION=1.3.5` to pin the available Bun binary.
3. Set `SKIP_DEPENDENCY_INSTALL=true` to disable automatic package-manager
   selection.
4. Use these build settings:
   - Build command: `cd website && bun install --frozen-lockfile --ignore-scripts && bun run build`
   - Build output directory: `website/build`
5. Set up a custom domain through CloudFlare's dashboard

#### Troubleshooting

Common issues:

- If TypeDoc fails to generate documentation, check the JSDoc comments for syntax errors
- If you see "Could not parse expression with acorn" errors, there are likely unescaped characters in code examples
- If links are broken, check that the referenced pages exist and paths are correct
- For sidebar issues, verify that the sidebar configuration in `sidebars.ts` is correct

## License

This project is licensed under the MIT License.
