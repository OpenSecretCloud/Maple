# BrowserUse Tests

## Overview
This directory contains automated browser tests using BrowserUse - an AI-powered browser automation library.

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

The `test_successful_login()` test uses a custom controller action:

```python
@controller.action('Input the password for Maple login')
async def input_password_securely(browser: BrowserContext):
    """Securely input password without exposing it to the LLM."""
    page = await browser.get_current_page()
    password = os.environ.get('BROWSERUSE_TEST_PASSWORD', '')
    await page.keyboard.type(password)
    return ActionResult(success=True, extracted_content="Password entered securely")
```

This approach ensures:
- The LLM never sees the actual password
- The password is typed directly into the page
- No sensitive data appears in logs or conversation history

### Running Tests

1. **All tests** (including successful login if credentials are set):
   ```bash
   python test_runner.py
   ```

2. **Individual tests**:
   ```bash
   python run_test.py invalid_email_format
   python run_test.py successful_login
   ```

3. **GitHub Actions**: Tests run automatically on push/PR
   - Successful login test is skipped if credentials aren't configured
   - To enable the successful login test in CI:
     1. Go to your repository Settings → Secrets and variables → Actions
     2. Add these repository secrets:
        - `BROWSERUSE_TEST_EMAIL`: A valid test account email
        - `BROWSERUSE_TEST_PASSWORD`: The test account password
   - The workflow is already configured to pass these secrets as environment variables

## Test Structure

- `base_test.py`: Base class handling browser setup and teardown
- `test_signin_simple.py`: All login-related tests
- `test_runner.py`: Runs all tests and reports results

## Security Notes

- Never commit real credentials to the repository
- Use test accounts specifically created for automation
- Rotate test credentials regularly
- In CI/CD, use encrypted secrets (GitHub Actions secrets, etc.)