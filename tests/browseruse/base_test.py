#!/usr/bin/env python3
"""
Base class for BrowserUse tests - handles browser setup only.
"""

import os
import json
from pathlib import Path
from browser_use import Agent, BrowserProfile, BrowserSession
from browser_use.browser.context import BrowserContextConfig
from langchain_openai import ChatOpenAI

class BrowserTestBase:
    """Base class that handles browser setup and teardown."""
    
    def __init__(self, test_name: str):
        self.test_name = test_name
        self.logs_dir = Path("./logs") / test_name
        self.logs_dir.mkdir(parents=True, exist_ok=True)
        
        # Check for API key
        if not os.getenv("OPENAI_API_KEY"):
            raise ValueError("OPENAI_API_KEY not set")
        
        # Initialize LLM
        self.llm = ChatOpenAI(model="gpt-4o", temperature=0)
        
        # Configure browser context for SPA loading
        self.context_config = BrowserContextConfig(
            wait_for_network_idle_page_load_time=5.0,
            minimum_wait_page_load_time=3.0,
            maximum_wait_page_load_time=15.0,
            wait_between_actions=3.0,
            slow_mo=500,
        )
        
        # Configure browser - check for local vs CI environment
        is_ci = os.getenv("CI") == "true" or os.getenv("GITHUB_ACTIONS") == "true"
        headless = is_ci or os.getenv("BROWSERUSE_HEADLESS", "false").lower() == "true"
        
        if not headless:
            print("🖥️  Running in headed mode - browser will be visible")
        
        self.browser_profile = BrowserProfile(
            args=[
                "--no-sandbox", 
                "--disable-setuid-sandbox",
                "--disable-gpu",
                "--disable-dev-shm-usage",
                "--disable-web-security",
                "--allow-insecure-localhost"
            ],
            headless=headless,
            new_context_config=self.context_config,
            viewport={"width": 1920, "height": 1080}  # Standard HD viewport
        )
        
        self.browser_session = BrowserSession(browser_profile=self.browser_profile)
    
    async def run_task(self, task: str, max_steps: int = 10, controller=None):
        """Run a browser task and return the result."""
        agent_args = {
            "task": task,
            "llm": self.llm,
            "browser_session": self.browser_session,
            "save_conversation_path": str(self.logs_dir / "conversation.json")
        }
        
        # Only add controller if it's provided
        if controller is not None:
            agent_args["controller"] = controller
            
        agent = Agent(**agent_args)
        
        try:
            result = await agent.run(max_steps=max_steps)
            
            # Save history
            history_path = self.logs_dir / "test_history.json"
            with open(history_path, "w", encoding="utf-8") as f:
                json.dump(result.to_dict() if hasattr(result, 'to_dict') else str(result), f, indent=2)
            
            return result
        except Exception as e:
            raise e
    
    
    async def close(self):
        """Clean up resources."""
        if hasattr(self, 'browser_session'):
            await self.browser_session.close()
