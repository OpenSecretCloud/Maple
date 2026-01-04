# Maple Support FAQ

Common user questions and suggested responses.

---

## Billing & Payments

### Q: How do I cancel my subscription?

**Answer:**
1. Log in to Maple at trymaple.ai
2. Click on your account menu in the sidebar (bottom left)
3. Select "Manage Subscription"
4. This opens the Stripe billing portal where you can cancel

**Note:** Only available for Stripe subscriptions. Bitcoin/Zaprite yearly plans are prepaid and non-refundable.

**References:**
- Account menu: `/frontend/src/components/AccountMenu.tsx`

---

### Q: Can I pay with Bitcoin monthly instead of yearly?

**Answer:** No, Bitcoin payments are yearly only since there's no way to auto-renew. We don't offer refunds for unused months on prepaid annual plans.

**References:**
- Pricing page: `/frontend/src/routes/pricing.tsx`
- Bitcoin toggle uses Zaprite for yearly billing

---

### Q: Can I get a refund for unused months on my Bitcoin/yearly plan?

**Answer:** No, we don't offer refunds for unused months on prepaid annual plans.

---

### Q: What's the difference between the plans?

**Answer:**
- **Free:** 25 messages per week (resets Sunday 00:00 UTC), max length on individual messages
- **Pro ($20/mo):** Generous usage for power users with a high monthly cap
- **Max ($100/mo):** 20x more usage than Pro for maximum power users
- **Team ($30/user/mo):** 2x more usage than Pro per team member with unified billing
- **Enterprise:** Contact team@opensecret.cloud

**References:**
- Pricing config: `/frontend/src/config/pricingConfig.tsx`
- Pricing FAQ in: `/frontend/src/routes/pricing.tsx`

---

## Features & File Uploads

### Q: What file types are supported for upload?

**Answer:**
- **Images:** .jpg, .png, .webp
- **Documents (Desktop/Mobile apps only):** .pdf, .txt, .md
- **Limits:** 1 file per prompt, 10MB max per file
- **Not supported:** .docx, .csv, .xls (no ETA)

**Note:** PDF extraction is text-based only. Scanned PDFs (image-based) won't work since there's no OCR.

**References:**
- PDF extractor: `/frontend/src-tauri/src/pdf_extractor.rs`
- Issue #359: docx support (idea)

---

### Q: Why isn't my PDF uploading / working?

**Troubleshooting steps:**
1. **Is the PDF text-based or scanned?** We extract text directly from PDFs, so scanned documents (images of pages) won't work. If you can highlight/copy text in the PDF, it should work.
2. **File size:** Must be under 10MB.
3. **Workaround:** Copy/paste the text content directly into the chat.

**References:**
- PDF extractor uses `pdf_extract` crate (text extraction, no OCR)

---

### Q: Do you support RAG / vector databases?

**Answer:** We support embeddings via the API, but we don't have built-in RAG or vector database functionality. No ETA on this currently.

---

### Q: How do I get the AI to provide reference links / citations?

**Answer:** Enable the web search toggle (globe icon) before sending a message. The AI will search the web and include sources with its response.

**References:**
- Web search in UnifiedChat: `/frontend/src/components/UnifiedChat.tsx`

---

## Chat History & Data

### Q: Can I export / download my chat history?

**Answer:** This feature isn't currently available. Chat history syncs automatically across all devices when logged in.

**References:**
- Issue #353: Export/download chat history (idea)

---

### Q: How do you sync my chat history across devices?

**Answer:** We use a secure synchronization protocol that ensures your encrypted chat history is synced across all your devices. Start a conversation on one device and pick it up on another without compromising security or privacy.

---

### Q: Can I tag or organize my conversations?

**Answer:** Not currently available. We're planning to add pinned chats.

**References:**
- Issue #348: Pinned chats (planned)
- Issue #354: Tagging/organizing conversations (idea)

---

### Q: Can I edit and resubmit a previous prompt?

**Answer:** Not currently available.

**References:**
- Issue #356: Edit and resubmit prompts (idea)
- Issue #357: Conversation branches / forking (idea)

---

### Q: Can I auto-delete old conversations?

**Answer:** Not currently available.

**References:**
- Issue #355: Auto-delete conversations (idea)

---

## Security & Privacy

### Q: Do you support MFA / 2FA?

**Answer:** Not currently, but it's on our roadmap.

**References:**
- Issue #358: Multi-factor authentication (planned, security)

---

### Q: How private is Maple?

**Answer:** Encrypted end-to-end. Maple uses confidential computing to secure user data and LLM data. Your account has its own private key that encrypts your chats and AI responses. Every user has their own personal data vault that can't be read by anyone else, not even us.

**References:**
- Proof page: `/frontend/src/routes/proof.tsx`
- Marketing page security section: `/frontend/src/components/Marketing.tsx`

---

### Q: Can companies use my data to train AI models?

**Answer:** No. When you chat with AI in Maple, nobody knows what is being said. Data cannot be used for training by any company.

---

### Q: Is this safe for confidential company information?

**Answer:** The service is encrypted end-to-end, so confidential information is private between you and the AI. Recommend consulting your company's security policy.

---

## Platform & Downloads

### Q: Is Maple open source?

**Answer:** Yes! The code is available at: https://github.com/OpenSecretCloud/Maple

We keep it open so anyone can review and verify our security claims.

---

### Q: What platforms is Maple available on?

**Answer:**
- **Desktop:** macOS, Linux (Windows coming soon)
- **Mobile:** iOS (App Store), Android (Play Store)
- **Web:** trymaple.ai

**References:**
- Downloads page: `/frontend/src/routes/downloads.tsx`
- App Store: https://apps.apple.com/us/app/id6743764835
- Play Store: https://play.google.com/store/apps/details?id=cloud.opensecret.maple

---

### Q: Is there a TestFlight / beta program?

**Answer:**
- **iOS TestFlight:** https://testflight.apple.com/join/zjgtyAeD
- **Android Beta:** https://play.google.com/apps/testing/cloud.opensecret.maple

---

## AI Models

### Q: What AI models are available?

**Answer:**
- **Free tier:** Llama 3.3 70B
- **Starter+ tier:** Gemma 3 27B (vision), Qwen3-VL 30B (vision)
- **Pro+ tier:** DeepSeek R1 671B (reasoning), Kimi K2 (reasoning), GPT-OSS 120B, Qwen3 Coder 480B

None of your data is transmitted to model providers - everything stays within secure enclaves.

**References:**
- Model config: `/frontend/src/components/ModelSelector.tsx`

---

### Q: Can you add Grok / ChatGPT / Claude / [other closed-source model]?

**Answer:** No. We only support open-source models that run entirely within our secure enclaves. Adding closed-source models (Grok, ChatGPT, Claude, etc.) would mean routing your data through third-party servers where those companies could see your conversations. That would break our end-to-end encryption guarantee.

This is fundamental to how Maple works - your data never leaves our private infrastructure.

---

### Q: Do you support image generation?

**Answer:** No, we don't support image generation and have no plans to add it.

---

## API

### Q: Do you have an API?

**Answer:** Yes, API access is available on Pro, Max, and Team plans. You can use system prompts to guide model behavior for your use case.

---

### Q: Can I build my own app using Maple's API?

**Answer:** Yes, Maple's API lets you integrate our privacy-focused AI models into your own applications. However, any app features (UI, user management, etc.) would need to be built on your end.

For beginners, tools like Cursor or Replit can help get started.

---

## Response Guidelines

1. Keep responses concise and friendly
2. Don't over-promise on features or timelines
3. For feature requests without an ETA, say "no ETA" rather than making up timelines
4. For unclear requests, ask clarifying questions before committing to answers
