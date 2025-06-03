#!/usr/bin/env python3
"""
Dashboard functionality tests for Maple application.
These tests verify core dashboard features after successful authentication.
"""

import asyncio
import os
import json
from base_test import BrowserTestBase
from login_helpers import create_login_controller, get_full_login_task, validate_credentials


def parse_json_result(final_result: str):
    """Helper to parse JSON from LLM result."""
    try:
        json_start = final_result.find('{')
        json_end = final_result.rfind('}') + 1
        if json_start != -1 and json_end > json_start:
            json_str = final_result[json_start:json_end]
            return json.loads(json_str)
        else:
            return None
    except json.JSONDecodeError:
        return None


async def test_model_selector():
    """Test model selector visibility and interaction functionality."""
    print("🧪 Testing model selector functionality...")
    
    # Validate credentials before starting test
    creds_valid, error_msg = validate_credentials()
    if not creds_valid:
        print(f"❌ Test failed: {error_msg}")
        return False
    
    base = BrowserTestBase("model_selector")
    
    try:
        controller = create_login_controller()
        
        post_login_task = """
10. Once logged in, wait for the page to fully load with the `wait_2_seconds` action
11. Look at the chat interface and verify you can see text that says "Model:" at the bottom of the chatbox component.
12. Click on the Model button dropdown.
13. Once the dropdown opens, you should see a list of available models
14. Click on a different model name in the dropdown (e.g., if "Llama 3.3 70B" is selected, click on "Gemma 3 27B" or vice versa). There should be at least 2 models available. Refresh if you have to. If it never shows up then fail the test case.
15. Verify the dropdown closes and the selected model name is now shown in the button

IMPORTANT: Your final response MUST be in the exact JSON format specified in the task instructions above!
"""
        
        task = get_full_login_task(
            post_login_task,
            "Model Selector Functionality",
            "The model selector should be visible in the chat interface and allow selecting different models via dropdown"
        )
        
        result = await base.run_task(task, max_steps=20, controller=controller)
        
        if result and result.is_done():
            final_result = result.final_result()
            result_data = parse_json_result(final_result)
            
            if result_data:
                test_case_passed = result_data.get('test_case_passed', False)
                explanation = result_data.get('explanation', '')
                
                if test_case_passed:
                    print(f"✅ Test passed: {explanation}")
                    return True
                else:
                    print(f"❌ Test failed: {explanation}")
                    return False
            else:
                print("❌ Test failed: Could not parse JSON from result")
                print(f"   Raw result: {final_result[:500]}..." if final_result else "   No result returned")
                return False
        else:
            print("❌ Test failed: Task did not complete successfully")
            print(f"   Result done: {result.is_done() if result else 'No result'}")
            return False
                
    except Exception as e:
        print(f"❌ Test failed with exception: {e}")
        import traceback
        print("   Stack trace:")
        traceback.print_exc()
        return False
    finally:
        await base.close()



if __name__ == "__main__":
    async def run_dashboard_tests():
        """Run all dashboard tests."""
        print("🚀 Starting dashboard tests...")
        
        tests = [
            ("model_selector", test_model_selector),
        ]
        
        results = []
        for test_name, test_func in tests:
            print(f"\n--- Running {test_name} ---")
            try:
                result = await test_func()
                results.append((test_name, result))
            except Exception as e:
                print(f"❌ {test_name} failed with exception: {e}")
                results.append((test_name, False))
        
        # Summary
        print("\n" + "="*50)
        print("DASHBOARD TEST RESULTS:")
        print("="*50)
        
        passed = 0
        total = len(results)
        
        for test_name, result in results:
            status = "✅ PASSED" if result else "❌ FAILED"
            print(f"{test_name}: {status}")
            if result:
                passed += 1
        
        print(f"\nOverall: {passed}/{total} tests passed")
        
        if passed == total:
            print("🎉 All dashboard tests passed!")
            return True
        else:
            print(f"⚠️  {total - passed} dashboard tests failed")
            return False
    
    asyncio.run(run_dashboard_tests())
