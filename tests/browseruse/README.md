# BrowserUse Tests

## Overview
This directory contains automated browser tests using BrowserUse - an AI-powered browser automation library.

## Tests Available

1. **Invalid Email Format Test** (`test_invalid_email_format`)
   - Tests login with invalid email (missing @ symbol)
   - Expects: Email format error message

2. **Invalid Credentials Test** (`test_invalid_credentials_valid_email`)
   - Tests login with valid email format but wrong password
   - Expects: Invalid credentials error (NOT email format error)

3. **Successful Login Test** (`test_successful_login`)
   - Tests login with real credentials from environment variables
   - Expects: Successful login and redirect to chat/dashboard

## Secure Credential Handling

### Best Practices Implemented

1. **Environment Variables**: Credentials are stored in environment variables, never hardcoded
2. **Custom Controller Actions**: Passwords are typed directly via controller actions, preventing them from being exposed to the LLM
3. **No Logging**: The password is never included in task instructions or logs

### Setting Up Credentials

For the successful login test, set these environment variables:

```bash
export BROWSERUSE_TEST_EMAIL="your-test-email@example.com"
export BROWSERUSE_TEST_PASSWORD="your-test-password"
```

### How It Works

The `test_successful_login()` test uses custom controller actions for both email and password:

```python
@controller.action('Input the email for Maple login')
async def input_email_securely(page: Page) -> ActionResult:
    """Securely input email without LLM interpretation."""
    email = os.environ.get('BROWSERUSE_TEST_EMAIL', '')
    await page.keyboard.press('Control+a')  # Clear field first
    await page.keyboard.type(email)
    return ActionResult(success=True, extracted_content="Email entered securely")

@controller.action('Input the password for Maple login')
async def input_password_securely(page: Page) -> ActionResult:
    """Securely input password without exposing it to the LLM."""
    password = os.environ.get('BROWSERUSE_TEST_PASSWORD', '')
    await page.keyboard.press('Control+a')  # Clear field first
    await page.keyboard.type(password)
    return ActionResult(success=True, extracted_content="Password entered securely")
```

This approach ensures:
- The LLM never sees the actual email or password values
- Credentials are typed directly into the page after clearing the fields
- No sensitive data appears in logs or conversation history
- Special characters in emails (like +) are handled correctly without LLM interpretation

### Running Tests

1. **All tests** (including successful login if credentials are set):
   ```bash
   python test_runner.py
   ```

2. **GitHub Actions**: Tests run automatically on push/PR to master

   - Successful login test is skipped if credentials aren't configured
   - To enable the successful login test in CI:
     1. Go to your repository Settings → Secrets and variables → Actions
     2. Add these repository secrets:
        - `BROWSERUSE_TEST_EMAIL`: A valid test account email (can include special characters like +)
        - `BROWSERUSE_TEST_PASSWORD`: The test account password
   - The workflow is already configured to pass these secrets as environment variables

## Test Structure

- `base_test.py`: Base class handling browser setup and teardown
- `test_signin_simple.py`: All login-related tests with structured JSON output
- `test_runner.py`: Runs all tests and reports results
- `requirements-ci.txt`: Minimal dependencies for CI environment

## Test Output Format

All tests use structured JSON output for clear pass/fail determination:

```json
{
  "test_case_passed": true/false,
  "actual_error": "Error message if applicable",
  "explanation": "What actually happened",
  "final_page_url": "URL for successful login test"
}
```

## Security Notes

- Never commit real credentials to the repository
- Use test accounts specifically created for automation
- Rotate test credentials regularly
- In CI/CD, use encrypted secrets (GitHub Actions secrets, etc.)
- Email addresses with special characters (like +) are supported and handled securely

## CI/CD Optimizations

- Python dependencies are cached between runs
- Playwright browsers are cached when possible
- System dependencies are only installed on first run or cache miss