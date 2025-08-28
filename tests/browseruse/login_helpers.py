#!/usr/bin/env python3
"""
Common login helper functions for browseruse tests.
These helpers provide reusable login functionality across all tests that require authentication.
"""

import os
from browser_use import Controller, ActionResult
from browser_use import BrowserSession
from playwright.async_api import Page


def create_login_controller():
    """
    Creates a browser controller with secure login actions.
    
    Returns:
        Controller: Configured controller with email and password input actions
    """
    controller = Controller()
    
    @controller.action('Input the email for Maple login')
    async def input_email_securely(browser_session: BrowserSession, page: Page) -> ActionResult:
        """Securely input email from environment variable."""
        email = os.environ.get('BROWSERUSE_TEST_EMAIL', '')
        if not email:
            return ActionResult(
                success=False, 
                extracted_content="BROWSERUSE_TEST_EMAIL environment variable not set"
            )
        
        # Clear any existing content and input email
        await page.keyboard.press('Control+a')
        await page.keyboard.type(email)
        return ActionResult(success=True, extracted_content="Email entered securely")
    
    @controller.action('Input the password for Maple login')
    async def input_password_securely(browser_session: BrowserSession, page: Page) -> ActionResult:
        """Securely input password from environment variable."""
        password = os.environ.get('BROWSERUSE_TEST_PASSWORD', '')
        if not password:
            return ActionResult(
                success=False,
                extracted_content="BROWSERUSE_TEST_PASSWORD environment variable not set"
            )
        
        # Clear any existing content and input password
        await page.keyboard.press('Control+a')
        await page.keyboard.type(password)
        return ActionResult(success=True, extracted_content="Password entered securely")
    
    @controller.action('wait_2_seconds')
    async def wait_2_seconds(browser_session: BrowserSession, page: Page) -> ActionResult:
        """Wait for 2 seconds to let the page stabilize."""
        import asyncio
        await asyncio.sleep(2)
        return ActionResult(success=True, extracted_content="Waited 2 seconds")
    
    @controller.action('Click model selector button')
    async def click_model_selector(browser_session: BrowserSession, page: Page) -> ActionResult:
        """Click the model selector button using its data-testid attribute."""
        try:
            # Wait for the element to be available and click it
            await page.wait_for_selector('[data-testid="model-selector-button"]', timeout=5000)
            await page.click('[data-testid="model-selector-button"]')
            
            # Wait a moment for the dropdown to appear
            import asyncio
            await asyncio.sleep(1)
            
            return ActionResult(success=True, extracted_content="Successfully clicked model selector button")
        except Exception as e:
            return ActionResult(success=False, extracted_content=f"Failed to click model selector: {str(e)}")
    
    return controller


def get_login_task_prefix():
    """
    Gets the standard login task instructions that can be prefixed to any test.
    
    Returns:
        str: Standard login task instructions
    """
    return """
PREREQUISITE: Complete login process if not already on the main logged in chat dashboard
1. Go to http://localhost:5173 and wait for the page to load completely
1a. If you're on the marketing page and see a "Log in" button, then continue
1b. IMPORTANT: If you're inside the main app, with a chatbox available, then you're already logged in. Skip the log in steps and go straight to step 10.
2. Look for and click the "Log in" or "Sign in" button/link to access the login form
3. Find the email input field and click on it to focus it
4. Use the 'Input the email for Maple login' action to enter the email securely
5. Find the password input field and click on it to focus it  
6. Use the 'Input the password for Maple login' action to enter the password securely
7. Click the login/sign in button to submit the form
8. Wait for successful login and navigation to the main dashboard
9. Verify you see the main chat interface with input field and model selector
"""


def validate_credentials():
    """
    Validates that required environment variables are set for login.
    
    Returns:
        tuple: (bool, str) - (is_valid, error_message)
    """
    email = os.environ.get('BROWSERUSE_TEST_EMAIL')
    password = os.environ.get('BROWSERUSE_TEST_PASSWORD')
    
    if not email:
        return False, "BROWSERUSE_TEST_EMAIL environment variable is not set"
    
    if not password:
        return False, "BROWSERUSE_TEST_PASSWORD environment variable is not set"
    
    if '@' not in email:
        return False, "BROWSERUSE_TEST_EMAIL does not appear to be a valid email address"
    
    return True, "Credentials are valid"


def get_full_login_task(post_login_task: str, test_name: str, expected_outcome: str):
    """
    Creates a complete test task that includes login followed by the actual test.
    
    Args:
        post_login_task (str): The test steps to perform after successful login
        test_name (str): Name of the test case for error reporting
        expected_outcome (str): Expected outcome description
        
    Returns:
        str: Complete task with login and test steps
    """
    return f"""
TEST CASE: {test_name}
EXPECTED OUTCOME: {expected_outcome}

STEPS:
{get_login_task_prefix()}

POST-LOGIN TESTING:
{post_login_task}

IMPORTANT: If login fails at any point, immediately return with test_case_passed: false and explain the failure including that login failed.
If login succeeds but the test case fails, return test_case_passed: false with details about what went wrong in the test.

When complete, you MUST provide your result in this EXACT JSON format (no other text before or after):
{{
  "test_case_passed": true/false,
  "explanation": "Detailed explanation of what happened during the test",
  "final_page_url": "Current URL after all steps"
}}

DO NOT include any text outside of the JSON object above!
"""
