---
title: Maple AI Integration
sidebar_position: 1
---

# Maple AI Integration

Integrate Maple's TEE-secured LLM inference directly into your application using API key authentication. This is the simplest way to add private, end-to-end encrypted AI to your app.

**Choose your SDK:**
- [TypeScript/JavaScript](#typescript-sdk)
- [Rust](#rust-sdk)

## Overview

Maple runs all LLM inference inside Trusted Execution Environments (TEEs), providing hardware-level security and privacy. Your prompts and responses are **encrypted end-to-end** and never accessible to anyone—not even Maple.

This guide covers **direct API integration** for applications serving multiple users, where each user authenticates with their own Maple API key.

### When to Use This Approach

**Direct SDK Integration (this guide)** is best when:
- Building apps that serve multiple end-users
- Each user brings their own Maple account and API credits
- You want direct API calls without running additional infrastructure

**Alternative: Maple Proxy** is best when:
- You need a local OpenAI-compatible server
- Building internal tools for yourself or your business
- You want to use existing OpenAI-compatible tools (LangChain, Goose, etc.)

For the proxy approach, see the [Maple Proxy Documentation](https://blog.trymaple.ai/maple-proxy-documentation/) or use the built-in proxy in the [Maple Desktop App](https://trymaple.ai/downloads).

## Prerequisites

Each user of your application will need:

1. A **paid Maple account** at [trymaple.ai](https://trymaple.ai) (Pro, Team, or Max plan)
2. An **API key** created from the [Maple dashboard](https://trymaple.ai)

---

## TypeScript SDK

### Installation

Install the OpenSecret React SDK and OpenAI client:

```bash
npm install @opensecret/react openai
```

### Quick Start

Here's the minimal code to make a completion request with an API key:

```typescript
import OpenAI from "openai";
import { createCustomFetch } from "@opensecret/react";

const MAPLE_API_URL = "https://enclave.trymaple.ai";

async function chat(apiKey: string, message: string) {
  const openai = new OpenAI({
    baseURL: `${MAPLE_API_URL}/v1/`,
    apiKey: apiKey,
    dangerouslyAllowBrowser: true,
    fetch: createCustomFetch({ apiKey, apiUrl: MAPLE_API_URL })
  });

  const stream = await openai.chat.completions.create({
    model: "gpt-oss-120b",
    messages: [{ role: "user", content: message }],
    stream: true
  });

  let response = "";
  for await (const chunk of stream) {
    const content = chunk.choices[0]?.delta?.content || "";
    response += content;
    process.stdout.write(content);
  }
  
  return response;
}

// Usage
chat("your-maple-api-key", "Hello, world!");
```

### How It Works

The `createCustomFetch({ apiKey, apiUrl })` function from the SDK handles:

1. **TEE Attestation** - Verifies you're talking to genuine secure hardware
2. **End-to-End Encryption** - Encrypts your prompts before transmission
3. **Secure Key Exchange** - Negotiates encryption keys with the TEE
4. **Response Decryption** - Decrypts the AI response client-side

Your application just uses the standard OpenAI client interface.

### Fetching Available Models

You can list available models using the OpenAI client:

```typescript
const openai = new OpenAI({
  baseURL: `${MAPLE_API_URL}/v1/`,
  apiKey: apiKey,
  dangerouslyAllowBrowser: true,
  fetch: createCustomFetch({ apiKey, apiUrl: MAPLE_API_URL })
});

const models = await openai.models.list();
console.log(models.data.map(m => m.id));
```

### Available Models

| Model | Best For | Price (per M tokens) |
|-------|----------|----------------------|
| `gpt-oss-120b` | Quick responses, creative writing | $1.50 input / $2.50 output |
| `kimi-k2-6` | Reasoning, coding, image analysis | $3 input / $10.50 output |
| `deepseek-v4-pro` | Deep reasoning, research, math | $3 input / $10.50 output |
| `llama3-3-70b` | General reasoning, daily tasks | $3.50 input / $5.50 output |
| `qwen3-vl-30b` | Image analysis, vision tasks | $2.50 input / $8 output |
| `gemma4-31b` | Fast image analysis | $10 input / $10 output |

For detailed model capabilities and example prompts, see the [Maple Model Guide](https://blog.trymaple.ai/maple-ai-model-guide-with-example-prompts/).

### React Component Example

Here's a complete React component for a chat interface:

```tsx
import { useState } from "react";
import OpenAI from "openai";
import { createCustomFetch } from "@opensecret/react";

const MAPLE_API_URL = "https://enclave.trymaple.ai";

type Message = {
  role: "user" | "assistant";
  content: string;
};

interface MapleChatProps {
  apiKey: string;
}

export function MapleChat({ apiKey }: MapleChatProps) {
  const [messages, setMessages] = useState<Message[]>([]);
  const [input, setInput] = useState("");
  const [loading, setLoading] = useState(false);

  async function handleSubmit(e: React.FormEvent) {
    e.preventDefault();
    if (!input.trim() || loading) return;

    const userMessage: Message = { role: "user", content: input };
    setMessages(prev => [...prev, userMessage]);
    setInput("");
    setLoading(true);

    try {
      const openai = new OpenAI({
        baseURL: `${MAPLE_API_URL}/v1/`,
        apiKey: apiKey,
        dangerouslyAllowBrowser: true,
        fetch: createCustomFetch({ apiKey, apiUrl: MAPLE_API_URL })
      });

      const stream = await openai.chat.completions.create({
        model: "gpt-oss-120b",
        messages: [...messages, userMessage],
        stream: true
      });

      let assistantContent = "";
      const assistantMessage: Message = { role: "assistant", content: "" };
      setMessages(prev => [...prev, assistantMessage]);

      for await (const chunk of stream) {
        const content = chunk.choices[0]?.delta?.content || "";
        assistantContent += content;
        setMessages(prev => [
          ...prev.slice(0, -1),
          { role: "assistant", content: assistantContent }
        ]);
      }
    } catch (error) {
      console.error("Chat error:", error);
    } finally {
      setLoading(false);
    }
  }

  return (
    <div>
      <div className="messages">
        {messages.map((msg, i) => (
          <div key={i} className={msg.role}>
            <strong>{msg.role}:</strong> {msg.content}
          </div>
        ))}
      </div>
      <form onSubmit={handleSubmit}>
        <input
          value={input}
          onChange={e => setInput(e.target.value)}
          placeholder="Type a message..."
          disabled={loading}
        />
        <button type="submit" disabled={loading}>
          {loading ? "Sending..." : "Send"}
        </button>
      </form>
    </div>
  );
}
```

### Using Without React

The `createCustomFetch` function is vanilla JavaScript and works outside React:

```typescript
import { createCustomFetch } from "@opensecret/react";

const MAPLE_API_URL = "https://enclave.trymaple.ai";

async function completion(apiKey: string, prompt: string) {
  const customFetch = createCustomFetch({ apiKey, apiUrl: MAPLE_API_URL });

  const response = await customFetch(`${MAPLE_API_URL}/v1/chat/completions`, {
    method: "POST",
    headers: {
      "Content-Type": "application/json",
      "Accept": "text/event-stream"
    },
    body: JSON.stringify({
      model: "gpt-oss-120b",
      messages: [{ role: "user", content: prompt }],
      stream: true
    })
  });

  const reader = response.body?.getReader();
  const decoder = new TextDecoder();
  let result = "";

  while (reader) {
    const { done, value } = await reader.read();
    if (done) break;

    const chunk = decoder.decode(value);
    const lines = chunk.split("\n");
    
    for (const line of lines) {
      if (line.startsWith("data: ") && line !== "data: [DONE]") {
        const data = JSON.parse(line.slice(6));
        const content = data.choices[0]?.delta?.content || "";
        result += content;
      }
    }
  }

  return result;
}
```

### Error Handling

Handle common error cases:

```typescript
try {
  const stream = await openai.chat.completions.create({
    model: "gpt-oss-120b",
    messages: [{ role: "user", content: "Hello" }],
    stream: true
  });
  // ... process stream
} catch (error) {
  if (error instanceof Error) {
    if (error.message.includes("401")) {
      console.error("Invalid API key");
    } else if (error.message.includes("402")) {
      console.error("Insufficient API credits");
    } else if (error.message.includes("429")) {
      console.error("Rate limited - try again later");
    } else {
      console.error("API error:", error.message);
    }
  }
}
```

---

## Rust SDK

### Installation

Add to your `Cargo.toml`:

```toml
[dependencies]
opensecret = "3.5.0"
bytes = "1"
futures = "0.3"
http = "1"
tokio = { version = "1", features = ["full"] }
```

### Quick Start

```rust
use bytes::Bytes;
use futures::StreamExt;
use http::Request;
use opensecret::{OpenSecretClient, Result};

#[tokio::main]
async fn main() -> Result<()> {
    let api_key = "your-maple-api-key".to_string();
    
    // Create client with API key
    let client = OpenSecretClient::new_with_api_key(
        "https://enclave.trymaple.ai",
        api_key
    )?;
    
    // Perform attestation handshake (required)
    client.perform_attestation_handshake().await?;
    
    // The lossless transport preserves provider-specific parameters and
    // returns decrypted response bytes for the caller to parse or forward.
    let request = Request::post("/v1/chat/completions")
        .body(Bytes::from_static(br#"{
            "model": "gemma4-31b",
            "messages": [{"role": "user", "content": "Hello, world!"}],
            "stream": true,
            "include_reasoning": false,
            "chat_template_kwargs": {"enable_thinking": false}
        }"#))
        .expect("valid request");

    let response = client.send_inference_request(request).await?;
    let mut body = response.into_body();
    while let Some(chunk) = body.next().await {
        let decrypted_bytes = chunk?;
        // Parse or forward the SSE bytes in the calling application.
        print!("{}", String::from_utf8_lossy(&decrypted_bytes));
    }
    
    Ok(())
}
```

### Fetching Available Models

```rust
use opensecret::{OpenSecretClient, Result};

async fn list_models(api_key: String) -> Result<()> {
    let client = OpenSecretClient::new_with_api_key(
        "https://enclave.trymaple.ai",
        api_key
    )?;
    
    client.perform_attestation_handshake().await?;
    
    let models = client.get_models().await?;
    
    for model in models.data {
        println!("{}", model.id);
    }
    
    Ok(())
}
```

### Error Handling

```rust
use opensecret::Error;

match client.get_models().await {
    Ok(models) => println!("Found {} models", models.data.len()),
    Err(Error::Authentication(msg)) => eprintln!("Auth failed: {}", msg),
    Err(Error::Api { status, message }) => {
        match status {
            401 => eprintln!("Invalid API key"),
            402 => eprintln!("Insufficient API credits"),
            429 => eprintln!("Rate limited"),
            _ => eprintln!("API error {}: {}", status, message),
        }
    }
    Err(e) => eprintln!("Error: {}", e),
}
```

### Key Differences from TypeScript

| Aspect | TypeScript | Rust |
|--------|------------|------|
| Client creation | `createCustomFetch({ apiKey })` | `OpenSecretClient::new_with_api_key(url, key)` |
| Attestation | Automatic (in fetch) | Manual: `client.perform_attestation_handshake().await?` |
| Streaming | `for await (const chunk of stream)` | `while let Some(chunk) = stream.next().await` |
| Inference request | Uses `openai` npm package | Lossless encrypted HTTP transport; typed compatibility helpers also available |

---

## Security Best Practices

1. **Never hardcode API keys** - Use environment variables or secure storage
2. **Each user needs their own key** - Don't share a single API key across users
3. **API keys are billed to the user** - Usage is charged to the key owner's account
4. **Rotate keys periodically** - Users can manage keys in the [Maple dashboard](https://trymaple.ai)

## Getting Help

- **Discord**: [Join the community](https://discord.gg/ch2gjZAMGy)
- **GitHub**: [OpenSecret SDK](https://github.com/OpenSecretCloud/OpenSecret-SDK)
- **Maple Support**: [trymaple.ai](https://trymaple.ai)
