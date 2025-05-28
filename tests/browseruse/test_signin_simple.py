#!/usr/bin/env python3
"""
All login-related tests for BrowserUse.
"""

import asyncio
import os
from base_test import BrowserTestBase
from browser_use import Controller, ActionResult
from playwright.async_api import Page

async def test_invalid_email_format():
    """Test login with invalid email format (missing @)."""
    print("🧪 Testing invalid email format...")
    
    base = BrowserTestBase("invalid_email_format")
    
    try:
        task = """
        Go to http://localhost:5173 and wait 5 seconds for the page to fully load. 
        Take a screenshot to see what's on the page.
        Look for ANY text or buttons on the page. If you see "Maple AI" or any navigation links, describe them.
        If you can find a login button or link (it might say "Login", "Sign In", "Log in", or be in the navigation), click on it.
        If you're on a login page, try to log in with:
        - Email: notanemail
        - Password: Test123!
        
        Tell me what you see on the page and if you see any error messages.
        """
        
        result = await base.run_task(task)
        
        if result and result.is_done():
            final_result = result.final_result()
            if final_result and "error" in final_result.lower() and "email" in final_result.lower():
                print("✅ Successfully found error message about invalid email format")
                return True
        
        print("❌ Did not find expected error message about invalid email format")
        return False
        
    except Exception as e:
        print(f"❌ Test failed with exception: {e}")
        return False
    finally:
        await base.close()

async def test_invalid_credentials_valid_email():
    """Test login with valid email format but wrong password."""
    print("🧪 Testing invalid credentials with valid email...")
    
    base = BrowserTestBase("invalid_credentials_valid_email")
    
    try:
        task = """
        Go to http://localhost:5173 and wait for the page to load.
        Find and click on the login button or link to go to the login page.
        Once on the login page, try to log in with these credentials:
        - Email: john.doe@example.com (this is a valid email format)
        - Password: WrongPassword123!
        
        Submit the form and tell me what error message appears.
        I expect to see an error about invalid credentials or incorrect password,
        NOT about email format since the email is properly formatted.
        """
        
        result = await base.run_task(task)
        
        if result and result.is_done():
            final_result = result.final_result()
            result_lower = final_result.lower() if final_result else ""
            
            # Check for credential error, not email format error
            if ("invalid" in result_lower or "incorrect" in result_lower) and "credentials" in result_lower:
                print("✅ Successfully found error message about invalid credentials")
                return True
        
        print("❌ Did not find expected error message about invalid credentials")
        return False
        
    except Exception as e:
        print(f"❌ Test failed with exception: {e}")
        return False
    finally:
        await base.close()


async def test_successful_login():
    """Test successful login with real credentials from environment variables."""
    print("🧪 Testing successful login with real credentials...")
    
    # Get credentials from environment variables
    email = os.environ.get('BROWSERUSE_TEST_EMAIL')
    password = os.environ.get('BROWSERUSE_TEST_PASSWORD')
    
    if not email or not password:
        print("⚠️  Test skipped: BROWSERUSE_TEST_EMAIL and BROWSERUSE_TEST_PASSWORD environment variables not set")
        return None  # Skip test, not a failure
    
    base = BrowserTestBase("successful_login")
    
    try:
        # Create controller with custom action for secure password input
        controller = Controller()
        
        @controller.action('Input the password for Maple login')
        async def input_password_securely(page: Page) -> ActionResult:
            """Securely input password without exposing it to the LLM."""
            # page parameter is automatically provided by browser-use
            
            # Get password from environment variable
            password = os.environ.get('BROWSERUSE_TEST_PASSWORD', '')
            
            # Type password directly without logging
            await page.keyboard.type(password)
            
            return ActionResult(
                success=True,
                extracted_content="Password entered securely"
            )
    
        # Task that uses email but NOT password in the instruction
        task = f"""
        Go to http://localhost:5173 and wait for the page to load.
        Find and click on the login button or link to go to the login page.
        Once on the login page, fill in the login form:
        - Email: {email}
        - Password: Use the 'Input the password for Maple login' action to enter the password securely
        
        After entering credentials, click the sign in button.
        
        Wait for the page to load after submission.
        If login is successful, you should be redirected to the chat interface or dashboard.
        Report whether login was successful and what page you ended up on.
        """
        
        result = await base.run_task(task, max_steps=15, controller=controller)
        
        if result and result.is_done():
            final_result = result.final_result()
            result_lower = final_result.lower() if final_result else ""
            
            # Check if login was successful (look for chat, dashboard, or success indicators)
            if ("success" in result_lower or "chat" in result_lower or 
                "dashboard" in result_lower or "logged in" in result_lower):
                print("✅ Successfully logged in with real credentials")
                return True
            else:
                print(f"❌ Login failed or unclear result: {final_result}")
                return False
        else:
            print("❌ Test did not complete successfully")
            return False
        
    except Exception as e:
        print(f"❌ Test failed with exception: {e}")
        return False
    finally:
        await base.close()


# For backward compatibility - original test
async def test_invalid_email():
    """Original test function for backward compatibility."""
    return await test_invalid_email_format()

async def main():
    """Run the original test for backward compatibility."""
    print("🧪 Running invalid sign-in test...")
    
    if not os.getenv("OPENAI_API_KEY"):
        print("❌ Error: OPENAI_API_KEY not set")
        exit(1)
    
    success = await test_invalid_email()
    
    if success:
        print("✅ Test completed")
        exit(0)
    else:
        print("❌ Test failed")
        exit(1)

if __name__ == "__main__":
    asyncio.run(main())
