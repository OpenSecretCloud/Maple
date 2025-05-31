#!/usr/bin/env python3
"""
Test runner for all BrowserUse tests.
"""

import asyncio
import sys
import traceback
from test_signin_simple import test_invalid_email_format, test_invalid_credentials_valid_email, test_successful_login
from test_dashboard import test_model_selector

# List of all tests to run
TESTS = [
    ("invalid_email_format", test_invalid_email_format),
    ("invalid_credentials_valid_email", test_invalid_credentials_valid_email),
    ("successful_login", test_successful_login),
    ("model_selector", test_model_selector),
]

async def run_all_tests():
    """Run all registered tests."""
    print("🚀 Starting BrowserUse test suite...")
    
    passed = 0
    failed = 0
    failed_tests = []

    for test_name, test_func in TESTS:
        print(f"\n{'='*60}")
        print(f"Running test: {test_name}")
        print('='*60)
        # Retry logic - attempt up to 3 times
        max_attempts = 3

        for attempt in range(1, max_attempts + 1):
            try:
                if attempt > 1:
                    print(f"🔄 Retrying test {test_name} (attempt {attempt}/{max_attempts})...")

                result = await test_func()

                if result is None:  # Test was skipped
                    print(f"⏩ Test {test_name} was skipped")
                    break
                if result:
                    passed += 1
                    print(f"✅ Test {test_name} PASSED")
                    break
                else:
                    if attempt < max_attempts:
                        print(f"⚠️  Test {test_name} failed, will retry...")
                    else:
                        failed += 1
                        failed_tests.append(test_name)
                        print(f"❌ Test {test_name} FAILED after {max_attempts} attempts")
            except Exception as e:
                print(f"❌ Test {test_name} failed with exception: {e}")
                traceback.print_exc()
                if attempt < max_attempts:
                    print(f"⚠️  Will retry test {test_name}...")
                    # Small delay between retries
                    await asyncio.sleep(2)
                else:
                    failed += 1
                    failed_tests.append(test_name)
                    print(f"❌ Test {test_name} FAILED after {max_attempts} attempts")

        print(f"Test {test_name} completed.")
    print(f"\n{'='*60}")
    print(f"Test Summary: {passed} passed, {failed} failed")
    if failed_tests:
        print(f"Failed tests: {', '.join(failed_tests)}")
    print('='*60)

    # Create a summary file for GitHub Actions
    with open("test_summary.txt", "w", encoding="utf-8") as f:
        f.write(f"Total tests: {len(TESTS)}\n")
        f.write(f"Passed: {passed}\n")
        f.write(f"Failed: {failed}\n")
        if failed_tests:
            f.write(f"Failed tests: {', '.join(failed_tests)}\n")

    return failed == 0

async def main():
    """Main entry point."""
    success = await run_all_tests()
    
    if success:
        print("\n✅ All tests passed!")
        sys.exit(0)
    else:
        print("\n❌ Some tests failed!")
        sys.exit(1)

if __name__ == "__main__":
    asyncio.run(main())
