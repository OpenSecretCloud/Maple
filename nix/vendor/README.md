Vendored Nix inputs
===================

These files are binary build inputs for Linux AppImage packaging. They are
checked into the flake source because the upstream `continuous` release assets
are mutable, which broke fixed-output hash verification on clean CI runners.

Nix still verifies each vendored file with the hashes in `flake.nix`.

- `linuxdeploy-plugin-appimage/linuxdeploy-plugin-appimage-x86_64.AppImage`
  - Source: `linuxdeploy/linuxdeploy-plugin-appimage` Actions run `25200823721`,
    artifact `6744621518` (`AppImage-x86_64`)
  - Hash: `sha256-Egjmp7HiZG4/sAbeqQC3K9hI7IYyS5l5t8lD8hHGacg=`
- `linuxdeploy-plugin-appimage/linuxdeploy-plugin-appimage-aarch64.AppImage`
  - Source: `linuxdeploy/linuxdeploy-plugin-appimage` Actions run `25200823721`,
    artifact `6744622485` (`AppImage-aarch64`)
  - Hash: `sha256-Ak4f3LJchgv9hSN5I6lO0VubrAwCqQ/xCpCcsIdmxfU=`
- `appimage-type2-runtime/runtime-x86_64`
  - Source: `AppImage/type2-runtime` continuous release asset
  - Hash: `sha256-okGdzkdWg5WuecAf+ppaNB3TOVgTUv8QTQc1J1Qxd+U=`
- `appimage-type2-runtime/runtime-aarch64`
  - Source: `AppImage/type2-runtime` continuous release asset
  - Hash: `sha256-fyeowVvyCi5GNC6kqXcEemnYtNZKEj/gteI7IP0pDIU=`
