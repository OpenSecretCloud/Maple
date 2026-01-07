# Windows Code Signing Certificate Setup Guide

This guide helps you choose and set up a code signing certificate for distributing Maple on Windows. Written for decision-makers who need to understand the options, costs, and tradeoffs before filling out forms.

---

## Table of Contents

1. [Why Do We Need This?](#why-do-we-need-this)
2. [What Users Will See](#what-users-will-see)
3. [Our Situation](#our-situation)
4. [The Three Options](#the-three-options)
5. [Recommendation](#recommendation)
6. [Detailed Setup Instructions](#detailed-setup-instructions)

---

## Why Do We Need This?

### The Problem: Windows Blocks Unsigned Apps

When a Windows user downloads and runs an unsigned application, they see this scary warning:

```
┌─────────────────────────────────────────────────────────┐
│  Windows protected your PC                              │
│                                                         │
│  Microsoft Defender SmartScreen prevented an            │
│  unrecognized app from starting. Running this app       │
│  might put your PC at risk.                             │
│                                                         │
│  App: Maple-setup.exe                                   │
│  Publisher: Unknown publisher                           │
│                                                         │
│  [Don't run]                                            │
│                                                         │
│  ▼ More info                                            │
└─────────────────────────────────────────────────────────┘
```

Most users will NOT click "More info" and "Run anyway". They'll assume the app is malware and leave.

### The Solution: Code Signing

With a valid code signing certificate, users see a much friendlier prompt:

```
┌─────────────────────────────────────────────────────────┐
│  Do you want to allow this app from an unknown          │
│  publisher to make changes to your device?              │
│                                                         │
│  Maple-setup.exe                                        │
│                                                         │
│  Publisher: OpenSecret LLC                              │
│             (or "John Smith" for individual certs)      │
│                                                         │
│  [Yes]  [No]                                            │
└─────────────────────────────────────────────────────────┘
```

The key differences:
- Shows a verified publisher name (not "Unknown publisher")
- Simple Yes/No prompt instead of scary "protected your PC" warning
- Users can see WHO made the app and make an informed decision

### SmartScreen Reputation (Important!)

Even with a valid certificate, **brand new certificates start with low reputation**. The first few hundred/thousand downloads may still trigger some warnings. This is normal. As more users download and run the app without issues, Windows learns to trust it more.

As of March 2024, Microsoft changed how this works:
- Previously, expensive "EV" (Extended Validation) certificates got instant trust
- Now, ALL certificates build reputation the same way - through successful downloads
- This means there's less reason to pay extra for EV certificates

---

## What Users Will See

The **publisher name** shown in Windows prompts comes directly from your certificate. You cannot choose an arbitrary name - it must be a verified legal identity.

### If We Use Individual Validation:
```
Publisher: John Smith
```
(Your personal legal name as shown on government ID)

### If We Use Organization Validation:
```
Publisher: [Your Legal Business Name LLC/Inc]
```
(Exactly as registered with the state - NOT a product name like "Maple")

### What About "Maple" as the Publisher?

**This is not possible** unless you have a legal entity named "Maple" or similar. Code signing certificates require verification of a real legal identity (person or registered business).

However, users will still see "Maple" in many places:
- The installer window title: "Maple - Private AI Chat Setup"
- Windows Add/Remove Programs: "Maple"
- The app itself: "Maple"
- Desktop shortcut: "Maple"

The legal entity name only appears in the security prompt during installation.

**Industry standard:** Many apps work this way. Discord shows "Discord Inc.", Slack shows "Slack Technologies, LLC", etc. Users understand that companies publish apps under product names.

---

## Our Situation

### Company Details
- **Company incorporated:** January 28, 2023
- **Current date:** January 7, 2026
- **Time since incorporation:** ~2 years, 11 months (3 weeks short of 3 years)

### The 3-Year Requirement

Microsoft's Azure Trusted Signing service (our cheapest/best option) has different requirements:

| Validation Type | Requirements | Publisher Name |
|-----------------|--------------|----------------|
| **Individual** | Government photo ID + selfie verification | Your personal name |
| **Organization** | 3+ years of verifiable business history | Your legal business name |

**"Verifiable business history"** typically means:
- Business registration records (state filings)
- EIN documentation
- Optionally: D-U-N-S number (speeds up verification)
- Tax filings help but aren't the primary check

Since we're only 3 weeks away from the 3-year mark, we have options (see Recommendation below).

---

## The Three Options

### Quick Comparison

| Aspect | Azure Individual | Azure Organization | SSL.com eSigner |
|--------|------------------|-------------------|-----------------|
| **Monthly Cost** | ~$10/month | ~$10/month | ~$20-42/month |
| **Annual Cost** | ~$120/year | ~$120/year | ~$239-499/year |
| **Publisher Name** | Personal name | Business name | Either |
| **Approval Time** | 1-3 business days | 1-3 business days | 1-5 business days |
| **Requirements** | Photo ID + selfie | 3+ years business | Flexible |
| **Available Now?** | Yes | After Jan 28, 2026 | Yes |
| **CI/CD Security** | Best (OIDC/keyless) | Best (OIDC/keyless) | Good (stored secrets) |

### Option 1: Azure Trusted Signing - Individual

**What it is:** Microsoft's cloud signing service using your personal identity.

**Publisher shows:** "Published by: [Your Legal Name]"

**Pros:**
- Cheapest option (~$120/year)
- Best CI/CD integration (no passwords/secrets to manage, uses modern "OIDC" authentication)
- Available immediately - no waiting
- Fast approval (1-3 business days)
- Microsoft manages all the security infrastructure

**Cons:**
- Shows personal name, not company name
- Must be US or Canada resident

**Good for:** Indie developers, small teams, getting started quickly

**Required for setup:**
- Azure account (free to create, pay-as-you-go billing)
- Government-issued photo ID (passport or driver's license)
- Smartphone with Microsoft Authenticator app
- ~30 minutes to complete the forms
- 1-3 business days for Microsoft to verify

---

### Option 2: Azure Trusted Signing - Organization

**What it is:** Same Microsoft service, but verified as a business entity.

**Publisher shows:** "Published by: [Your Legal Business Name]"

**Pros:**
- Professional appearance with company name
- Same low cost as Individual (~$120/year)
- Same excellent CI/CD integration
- Best long-term solution

**Cons:**
- **Requires 3+ years of verifiable business history**
- We can't apply until January 28, 2026 (3 weeks from now)
- Slightly more documentation required

**Good for:** Established companies wanting professional branding

**Required for setup:**
- Everything from Individual, plus:
- Legal business name (exactly as registered with state)
- Business address
- EIN (Employer Identification Number)
- D-U-N-S number (optional but speeds up verification significantly)
- ~45 minutes to complete the forms
- 1-3 business days for Microsoft to verify (longer without D-U-N-S)

**What is a D-U-N-S number?**
A unique 9-digit identifier for businesses, issued by Dun & Bradstreet. Many large organizations use it to verify businesses. You can request one for free at [dnb.com](https://www.dnb.com/duns-number.html), but it takes 30+ days. If you already have one, use it - it dramatically speeds up verification.

---

### Option 3: SSL.com eSigner (Backup)

**What it is:** A traditional certificate authority with cloud signing capabilities.

**Publisher shows:** Business name or personal name (depending on certificate type)

**Pricing:**
- OV (Organization Validated): ~$239/year
- EV (Extended Validation): ~$499/year
- Note: EV no longer provides SmartScreen advantages as of 2024

**Pros:**
- More flexible requirements (may accept businesses under 3 years)
- Works internationally (not limited to US/Canada)
- Well-established company
- Can potentially use company name sooner

**Cons:**
- 2-4x more expensive than Azure
- Less secure CI/CD integration (requires storing passwords as secrets)
- More complex setup process
- May still require extensive business documentation

**Good for:** International companies, those rejected by Azure, backup option

**Required for setup:**
- Credit card for payment
- Government-issued photo ID
- Business registration documents
- Domain ownership verification
- Phone verification callback
- For businesses under 3 years: possibly a lawyer/accountant opinion letter

---

## Recommendation

### If Company Branding Matters (Recommended Path)

**Wait until January 28, 2026, then use Azure Organization Validation.**

You're only 3 weeks away from the 3-year mark. The benefits:
- Professional "Published by: [Company Name]" appearance
- Cheapest option long-term
- Best CI/CD security
- No need to redo anything later

**Use the 3 weeks to prepare:**
1. Create Azure account and subscription
2. Gather required documents (EIN, business registration)
3. Check if you have a D-U-N-S number (or start requesting one)
4. Developer can set up all the non-certificate parts of Windows support

### If Speed Matters More

**Use Azure Individual Validation now, upgrade later.**

Get Windows builds shipping immediately with your personal name as publisher. When the company hits 3 years, you can switch to Organization Validation. The signing process is identical - just different identity verification.

Downsides:
- Initial users see personal name, not company
- Extra work to switch later (re-do validation)
- But: builds ship immediately

### If Azure Doesn't Work Out

**Fall back to SSL.com eSigner.**

If Microsoft rejects the Organization Validation for any reason, SSL.com is a reliable backup. More expensive and slightly less convenient, but well-established and functional.

---

## Detailed Setup Instructions

### Azure Trusted Signing Setup (Individual or Organization)

These instructions work for both Individual and Organization validation. The only difference is step 4.

#### Prerequisites

Before starting, you'll need:
- A credit card for Azure billing (you won't be charged until you use the service)
- Government-issued photo ID
- Smartphone with Microsoft Authenticator app installed
- For Organization: business registration documents and EIN

#### Step 1: Create an Azure Account

1. Go to [portal.azure.com](https://portal.azure.com)
2. Click "Start free" or sign in if you have an existing Microsoft account
3. Complete the signup process
4. When asked about a subscription, choose **"Pay-As-You-Go"**
   - This means you only pay for what you use
   - No upfront commitment
   - The signing service costs ~$10/month

#### Step 2: Register the Code Signing Feature

Azure needs to know you want to use the code signing feature. This is a one-time setup.

1. In Azure Portal, click on **"Subscriptions"** in the left sidebar (or search for it)
2. Click on your subscription name
3. In the left sidebar, scroll down to **"Settings"** → **"Resource providers"**
4. In the search box, type **"CodeSigning"**
5. Find **"Microsoft.CodeSigning"** in the list
6. Click on it, then click **"Register"** at the top
7. Wait for the status to change from "Registering" to **"Registered"** (refresh the page if needed)

#### Step 3: Create a Trusted Signing Account

This is where your certificates will live.

1. In the Azure Portal search bar at the top, search for **"Trusted Signing"**
2. Click on **"Trusted Signing Accounts"** in the results
3. Click **"+ Create"**
4. Fill in the form:
   - **Subscription:** Select your Pay-As-You-Go subscription
   - **Resource group:** Click "Create new" and name it something like "maple-signing"
   - **Account name:** Something like "maple-codesigning" (must be unique)
   - **Region:** Choose one close to you. For US, "West US 2" is a good choice
     - Note the region - you'll need the endpoint URL later:
       - West US: `https://wus.codesigning.azure.net`
       - East US: `https://eus.codesigning.azure.net`
       - West Europe: `https://weu.codesigning.azure.net`
   - **SKU:** Select **"Basic"** ($9.99/month)
5. Click **"Review + create"**
6. Click **"Create"**
7. Wait for deployment to complete (1-2 minutes)

#### Step 4: Complete Identity Validation

This is where Individual and Organization differ.

**For Individual Validation:**

1. Go to your new Trusted Signing Account resource
2. In the left sidebar, click **"Identity validation"**
3. Click **"+ Add"**
4. Select **"Individual"**
5. You'll be guided through:
   - **Photo ID upload:** Take a clear photo of your passport or driver's license
   - **Selfie/liveness check:** The Microsoft Authenticator app will ask you to take a selfie and may ask you to turn your head or blink
   - **Address verification:** Confirm your current address. If your ID has an old address, you may need to upload a utility bill
6. Submit and wait 1-3 business days for approval
7. You'll receive an email when approved

**For Organization Validation:**

1. Go to your new Trusted Signing Account resource
2. In the left sidebar, click **"Identity validation"**
3. Click **"+ Add"**
4. Select **"Organization"**
5. Fill in your business details **exactly as registered:**
   - Legal business name (must match state registration exactly)
   - Business address
   - EIN (Employer Identification Number)
   - D-U-N-S number (if you have one - highly recommended)
   - Primary contact name, email, phone
6. Submit and wait 1-3 business days for approval
7. Microsoft will verify against public business records
8. You'll receive an email when approved

**If Organization Validation is Rejected:**
- Most common reason: business history less than 3 years
- You can immediately switch to Individual Validation as a fallback
- Try again after your company reaches the 3-year mark

#### Step 5: Create a Certificate Profile

After identity validation is approved:

1. In your Trusted Signing Account, go to **"Certificate profiles"** in the left sidebar
2. Click **"+ Add"**
3. Fill in:
   - **Profile name:** Something like "maple-signing-profile"
   - **Certificate type:** Select **"Public Trust"** (this is required for public distribution)
   - **Identity validation:** Select the validation you just completed
4. Click **"Create"**

The certificate itself is generated automatically each time you sign something. You don't need to download or manage certificate files.

#### Step 6: Set Up CI/CD Authentication (Technical - Can Be Done by Developer)

This section creates the connection between GitHub Actions and Azure. The developer can complete this part.

**Create an App Registration:**

1. In Azure Portal, search for **"Microsoft Entra ID"** (formerly Azure Active Directory)
2. In the left sidebar, click **"App registrations"**
3. Click **"+ New registration"**
4. Fill in:
   - **Name:** "GitHub Actions - Maple Signing"
   - **Supported account types:** "Accounts in this organizational directory only"
   - **Redirect URI:** Leave blank
5. Click **"Register"**
6. On the app's overview page, note down:
   - **Application (client) ID** - you'll need this later
   - **Directory (tenant) ID** - you'll need this later

**Configure Federated Credentials (for secure GitHub connection):**

1. In your App Registration, go to **"Certificates & secrets"** in the left sidebar
2. Click the **"Federated credentials"** tab
3. Click **"+ Add credential"**
4. Select **"GitHub Actions deploying Azure resources"**
5. Fill in:
   - **Organization:** OpenSecretCloud
   - **Repository:** Maple
   - **Entity type:** Tag
   - **Tag:** v*
   - **Name:** "github-releases"
6. Click **"Add"**

This creates a secure, passwordless connection. GitHub Actions can authenticate with Azure without storing any passwords.

**Assign Signing Permissions:**

1. Go back to your Trusted Signing Account resource
2. Click **"Access control (IAM)"** in the left sidebar
3. Click **"+ Add"** → **"Add role assignment"**
4. Search for and select: **"Trusted Signing Certificate Profile Signer"**
5. Click **"Next"**
6. Click **"+ Select members"**
7. Search for "GitHub Actions - Maple Signing" (the app you just created)
8. Select it and click **"Select"**
9. Click **"Review + assign"** twice

#### Step 7: Record the Values for Developer

After completing setup, provide these values to the developer to add as GitHub Secrets:

| Secret Name | Where to Find It | Example |
|-------------|------------------|---------|
| AZURE_CLIENT_ID | App Registration → Overview → Application (client) ID | a1b2c3d4-e5f6-... |
| AZURE_TENANT_ID | App Registration → Overview → Directory (tenant) ID | f1e2d3c4-b5a6-... |
| AZURE_SUBSCRIPTION_ID | Subscriptions → Your subscription → Subscription ID | 12345678-... |
| AZURE_ENDPOINT | Based on region you chose | https://wus.codesigning.azure.net |
| AZURE_ACCOUNT | Trusted Signing Account name | maple-codesigning |
| AZURE_PROFILE | Certificate Profile name | maple-signing-profile |

---

### SSL.com eSigner Setup (Backup Option)

Only use this if Azure doesn't work out.

#### Step 1: Purchase a Certificate

1. Go to [ssl.com/certificates/code-signing](https://www.ssl.com/certificates/code-signing/)
2. Choose:
   - **"Code Signing Certificates"** for OV (~$239/year)
   - **"EV Code Signing"** for extended validation (~$499/year)
3. Select your term (1, 2, or 3 years)
4. **Important:** During checkout, make sure to select **"eSigner"** as the signing method
   - This enables cloud signing (required for automated CI/CD)
   - Without this, you'd need a physical USB token

#### Step 2: Complete Validation

After purchase, SSL.com will guide you through validation:

**For Individual/OV:**
- Upload government-issued photo ID
- Complete phone verification

**For Business/OV:**
- Upload business registration documents
- Verify domain ownership (if applicable)
- Complete phone callback verification

**For EV:**
- All of the above, plus:
- More extensive business verification
- May require lawyer/accountant opinion letter for newer businesses

This process typically takes 1-5 business days.

#### Step 3: Set Up eSigner for CI/CD

After validation is complete:

1. Log into your SSL.com account dashboard
2. Go to your certificate → **"eSigner"** settings
3. Generate a **TOTP secret** (this is like a 2FA backup code for automation)
4. Note your **Credential ID**

#### Step 4: Record Values for Developer

| Secret Name | Where to Find It |
|-------------|------------------|
| ES_USERNAME | Your SSL.com account email |
| ES_PASSWORD | Your SSL.com account password |
| CREDENTIAL_ID | eSigner settings → Credential ID |
| ES_TOTP_SECRET | eSigner settings → TOTP secret |

---

## Glossary

**Certificate:** A digital document that proves your identity. Like a notarized signature for software.

**Code Signing:** The process of digitally signing software to prove it came from you and hasn't been tampered with.

**SmartScreen:** Microsoft's reputation system that decides whether to warn users about downloaded software.

**OIDC (OpenID Connect):** A secure authentication method that doesn't require storing passwords. Azure uses this.

**TOTP:** Time-based One-Time Password. The same technology as Google Authenticator codes.

**OV (Organization Validated):** A certificate that proves a business exists. Shows company name.

**EV (Extended Validation):** A more thoroughly verified certificate. As of 2024, provides no SmartScreen advantage over OV.

**D-U-N-S Number:** A unique business identifier from Dun & Bradstreet. Speeds up business verification.

**HSM (Hardware Security Module):** Secure hardware that stores cryptographic keys. Azure manages this for you in the cloud.

---

## Timeline Summary

| Date | Action |
|------|--------|
| Now | Create Azure account, gather documents, developer prepares Windows build config |
| January 28, 2026 | Company reaches 3-year mark, submit Organization Validation |
| ~January 31, 2026 | Expected approval (1-3 business days) |
| Early February | First signed Windows builds shipping |

**Alternative timeline (if using Individual Validation now):**

| Date | Action |
|------|--------|
| Now | Submit Individual Validation with personal ID |
| ~January 10, 2026 | Expected approval, start shipping Windows builds |
| January 28, 2026 | (Optional) Switch to Organization Validation for company branding |
