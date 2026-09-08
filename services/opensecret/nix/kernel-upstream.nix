# The enclave kernel is pinned independently of the Nixpkgs snapshot so Linux
# stable fixes do not wait for the NixOS package-update cadence. Update all four
# fields together after reviewing the upstream 6.12 LTS changelog.
{
  branch = "6.12";
  version = "6.12.101";
  url = "https://cdn.kernel.org/pub/linux/kernel/v6.x/linux-6.12.101.tar.xz";
  hash = "sha256-DSHNEZM/SfcVG3ydu4zD/dyMir5QZDS4UP7s9B/CinY=";
}
