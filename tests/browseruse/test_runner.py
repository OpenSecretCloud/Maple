#!/usr/bin/env python3
"""
Test runner for all BrowserUse tests.
"""

import asyncio
import sys
from test_signin_simple import test_invalid_email_format, test_invalid_credentials_valid_email

# List of all tests to run
TESTS = [
    ("invalid_email_format", test_invalid_email_format),
    ("invalid_credentials_valid_email", test_invalid_credentials_valid_email),
]

async def run_all_tests():
    """Run all registered tests."""
    print("🚀 Starting BrowserUse test suite...")
    
    passed = 0
    failed = 0
    
    for test_name, test_func in TESTS:
        print(f"\n{'='*60}")
        print(f"Running test: {test_name}")
        print('='*60)
        
        try:
            success = await test_func()
            if success:
                passed += 1
            else:
                failed += 1
        except Exception as e:
            print(f"❌ Test {test_name} failed with exception: {e}")
            failed += 1
        
        print(f"Test {test_name} completed.")
    
    print(f"\n{'='*60}")
    print(f"Test Summary: {passed} passed, {failed} failed")
    print('='*60)
    
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