# iOS Build Troubleshooting

## arm64-sim Architecture Error

### Problem
When building for iOS simulator, you may encounter this error:
```
clang: error: version '-sim' in target triple 'arm64-apple-ios13.0-simulator-sim' is invalid
```

This happens when the Xcode project file incorrectly lists `arm64-sim` as an architecture, causing a duplicate `-sim` suffix in the target triple.

### Solution

1. **Edit the Xcode project file** (`frontend/src-tauri/gen/apple/maple.xcodeproj/project.pbxproj`):

   Find and replace all occurrences of:
   ```
   ARCHS = (
       arm64,
       "arm64-sim",
   );
   ```
   
   With:
   ```
   ARCHS = (
       arm64,
       x86_64,
   );
   ```

2. **Update VALID_ARCHS**:
   
   Replace:
   ```
   VALID_ARCHS = "arm64  arm64-sim";
   ```
   
   With:
   ```
   VALID_ARCHS = "arm64 x86_64";
   ```

3. **Update EXCLUDED_ARCHS**:
   
   Replace:
   ```
   "EXCLUDED_ARCHS[sdk=iphoneos*]" = "arm64-sim x86_64";
   ```
   
   With:
   ```
   "EXCLUDED_ARCHS[sdk=iphoneos*]" = x86_64;
   ```

### Important Notes

- Keep the `arm64-sim` references in library search paths and output paths - these refer to directory names, not architectures
- This issue can reoccur if the Xcode project is regenerated
- Related to Xcode 16.3+ behavior changes with simulator architectures

### Prevention

To prevent this issue from recurring:

1. Avoid regenerating the iOS project unless necessary
2. If you must regenerate, check the project.pbxproj file for incorrect `arm64-sim` architecture entries
3. Consider adding a post-generation script to automatically fix these entries

### Reference

This issue is tracked in [tauri-apps/tauri#12882](https://github.com/tauri-apps/tauri/issues/12882)