#!/usr/bin/env python3
"""
Simple BrowserUse test for invalid sign-in attempts.
Designed to run in GitHub Actions.
"""

import asyncio
import os
import json
from pathlib import Path
from browser_use import Agent, BrowserProfile, BrowserSession
from browser_use.browser.context import BrowserContextConfig
from langchain_openai import ChatOpenAI

async def test_invalid_email():
    """Test login with invalid email format."""
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
    
    llm = ChatOpenAI(model="gpt-4o", temperature=0)
    
    # Configure browser context for SPA loading
    context_config = BrowserContextConfig(
        wait_for_network_idle_page_load_time=5.0,  # Wait 5 seconds for network idle
        minimum_wait_page_load_time=3.0,  # Minimum 3 seconds wait
        maximum_wait_page_load_time=15.0,  # Maximum 15 seconds wait
        wait_between_actions=2.0,  # 2 seconds between actions
    )
    
    # Configure browser for GitHub Actions (disable sandbox)
    # Try with additional args that might help with rendering
    browser_profile = BrowserProfile(
        args=[
            "--no-sandbox", 
            "--disable-setuid-sandbox",
            "--disable-gpu",
            "--disable-dev-shm-usage",
            "--disable-web-security",
            "--allow-insecure-localhost"
        ],
        headless=True,
        new_context_config=context_config
    )
    
    browser_session = BrowserSession(browser_profile=browser_profile)
    
    # Create logs directory
    logs_dir = Path("./logs")
    logs_dir.mkdir(exist_ok=True)
    
    # Configure agent with logging
    agent = Agent(
        task=task, 
        llm=llm, 
        browser_session=browser_session,
        save_conversation_path=str(logs_dir / "conversation.json")
    )
    
    try:
        result = await agent.run(max_steps=10)
        print(f"Test result: {result}")
        
        # Save the final screenshot if available
        if hasattr(result, 'screenshot') and result.screenshot:
            screenshot_path = logs_dir / "final_screenshot.png"
            with open(screenshot_path, "wb") as f:
                import base64
                f.write(base64.b64decode(result.screenshot))
            print(f"Screenshot saved to: {screenshot_path}")
        
        # Save the full history
        history_path = logs_dir / "test_history.json"
        with open(history_path, "w") as f:
            json.dump(result.to_dict() if hasattr(result, 'to_dict') else str(result), f, indent=2)
        print(f"History saved to: {history_path}")
        
        # Check if the agent successfully completed the task
        if result and result.is_done():
            # Check if we found the error message about invalid email format
            final_result = result.final_result()
            if final_result and "error" in final_result.lower() and "email" in final_result.lower():
                print("✅ Successfully found error message about invalid email format")
                return True
            else:
                print("❌ Did not find expected error message about invalid email format")
                return False
        else:
            print("❌ Agent failed to complete the task")
            return False
            
    except Exception as e:
        print(f"❌ Test failed with exception: {e}")
        return False

async def main():
    """Run the test."""
    print("🧪 Running invalid sign-in test...")
    
    # Check for API key
    if not os.getenv("OPENAI_API_KEY"):
        print("❌ Error: OPENAI_API_KEY not set")
        exit(1)
    
    # Run test
    success = await test_invalid_email()
    
    if success:
        print("✅ Test completed")
        exit(0)
    else:
        print("❌ Test failed")
        exit(1)

if __name__ == "__main__":
    asyncio.run(main())