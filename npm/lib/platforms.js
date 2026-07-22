const PLATFORM_PACKAGES = {
  "darwin-arm64": "@open330/agt-darwin-arm64",
  "linux-x64": "@open330/agt-linux-x64",
  "linux-arm64": "@open330/agt-linux-arm64",
};

const RUST_TARGETS = {
  "darwin-arm64": "aarch64-apple-darwin",
  "linux-x64": "x86_64-unknown-linux-musl",
  "linux-arm64": "aarch64-unknown-linux-musl",
};

module.exports = { PLATFORM_PACKAGES, RUST_TARGETS };
