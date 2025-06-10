#!/usr/bin/env python3
"""
All login-related tests for BrowserUse.
"""

import asyncio
import os
import json
from base_test import BrowserTestBase
from login_helpers import create_login_controller


def parse_json_result(final_result: str):
    """Helper to parse JSON from LLM result."""
    try:
        # Extract JSON from the result - it might be wrapped in other text
        json_start = final_result.find('{')
        json_end = final_result.rfind('}') + 1
        
        if json_start != -1 and json_end > json_start:
            json_str = final_result[json_start:json_end]
            return json.loads(json_str)
        else:
            return None
    except json.JSONDecodeError:
        return None

async def test_invalid_email_format():
    """Test login with invalid email format (missing @)."""
    print("🧪 Testing invalid email format...")
    
    base = BrowserTestBase("invalid_email_format")
    
    try:
        task = """
        TEST CASE: Invalid Email Format
        EXPECTED OUTCOME: Login should fail with an error about missing @ symbol in email
        
        STEPS:
        1. Go to http://localhost:5173 and wait for the page to load
        2. Find and click the login button
        3. Try to log in with:
           - Email: notanemail (this is intentionally invalid - missing @)
           - Password: Test123!
        4. Submit the form
        
        EXPECTED RESULT: You should see an error message about the email format being invalid (missing @ symbol)
        
        IMPORTANT: When complete, provide your result as JSON:
        {
          "test_case_passed": true/false (true if you got the expected email format error),
          "actual_error": "The exact error message shown",
          "explanation": "What actually happened"
        }
        """
        
        result = await base.run_task(task)
        
        if result and result.is_done():
            final_result = result.final_result()
            
            result_data = parse_json_result(final_result)
            
            if result_data:
                test_case_passed = result_data.get('test_case_passed', False)
                actual_error = result_data.get('actual_error', '')
                explanation = result_data.get('explanation', '')
                
                if test_case_passed:
                    print(f"✅ Test passed: {explanation}")
                    print(f"   Error message: {actual_error}")
                    return True
                else:
                    print(f"❌ Test failed: {explanation}")
                    print(f"   Error message: {actual_error}")
                    return False
            else:
                print(f"❌ Test failed: Could not parse JSON from result")
                print(f"   Raw result: {final_result}")
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
        TEST CASE: Invalid Credentials (Valid Email Format)
        EXPECTED OUTCOME: Login should fail with an error about invalid credentials/password, NOT email format
        
        STEPS:
        1. Go to http://localhost:5173 and wait for the page to load
        2. Find and click the login button
        3. Try to log in with:
           - Email: john.doe@example.com (valid email format)
           - Password: WrongPassword123! (incorrect password)
        4. Submit the form
        
        EXPECTED RESULT: You should see an error about invalid credentials, wrong password, or authentication failure.
        IMPORTANT: The error should NOT be about email format since the email is properly formatted.
        
        IMPORTANT: When complete, provide your result as JSON:
        {
          "test_case_passed": true/false (true if you got a credential/password error, false if you got an email format error),
          "actual_error": "The exact error message shown",
          "explanation": "What actually happened"
        }
        """
        
        result = await base.run_task(task)
        
        if result and result.is_done():
            final_result = result.final_result()
            
            result_data = parse_json_result(final_result)
            
            if result_data:
                test_case_passed = result_data.get('test_case_passed', False)
                actual_error = result_data.get('actual_error', '')
                explanation = result_data.get('explanation', '')
                
                if test_case_passed:
                    print(f"✅ Test passed: {explanation}")
                    print(f"   Error message: {actual_error}")
                    return True
                else:
                    print(f"❌ Test failed: {explanation}")
                    print(f"   Error message: {actual_error}")
                    return False
            else:
                print(f"❌ Test failed: Could not parse JSON from result")
                print(f"   Raw result: {final_result}")
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
        # Use the reusable login controller from login_helpers
        controller = create_login_controller()
    
        # Task that doesn't include the actual email or password
        task = """
        TEST CASE: Successful Login
        EXPECTED OUTCOME: Login should succeed and redirect to the chat/dashboard page
        
        STEPS:
        1. Go to http://localhost:5173 and wait for the page to load
        2. Find and click the login button
        3. Fill in the login form (IMPORTANT: follow these steps exactly):
           - First, click on the email input field to focus it
           - Then use the 'Input the email for Maple login' action (this will clear and type the email)
           - Next, click on the password input field to focus it  
           - Then use the 'Input the password for Maple login' action (this will clear and type the password)
        4. Submit the form
        5. Wait for the page to load after submission
        
        EXPECTED RESULT: Login should succeed and you should be redirected to the chat interface or dashboard
        
        IMPORTANT: When complete, provide your result as JSON:
        {
          "test_case_passed": true/false (true if login succeeded and you reached the chat/dashboard),
          "final_page_url": "The URL you ended up on",
          "explanation": "What actually happened"
        }
        """
        
        result = await base.run_task(task, max_steps=15, controller=controller)
        
        if result and result.is_done():
            final_result = result.final_result()
            
            result_data = parse_json_result(final_result)
            
            if result_data:
                test_case_passed = result_data.get('test_case_passed', False)
                final_page_url = result_data.get('final_page_url', 'Unknown')
                explanation = result_data.get('explanation', '')
                
                if test_case_passed:
                    print(f"✅ Test passed: {explanation}")
                    print(f"   Final page: {final_page_url}")
                    return True
                else:
                    print(f"❌ Test failed: {explanation}")
                    print(f"   Final page: {final_page_url}")
                    return False
            else:
                print(f"❌ Test failed: Could not parse JSON from result")
                print(f"   Raw result: {final_result}")
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
