#!/usr/bin/env node

const { execFileSync } = require("child_process");
const path = require("path");

const PLATFORM_MAP = {
  "darwin-arm64": "@open330/agt-darwin-arm64",
  "linux-x64": "@open330/agt-linux-x64",
};

const platform = `${process.platform}-${process.arch}`;
const binName = process.platform === "win32" ? "agt.exe" : "agt";

let binary;

// Try platform-specific optional dependency
const pkg = PLATFORM_MAP[platform];
if (pkg) {
  try {
    binary = require.resolve(`${pkg}/bin/${binName}`);
  } catch {}
}

// Fallback: binary downloaded by postinstall
if (!binary) {
  binary = path.join(__dirname, binName);
}

if (!binary || !require("fs").existsSync(binary)) {
  console.error(
    `agt: native binary not found for ${platform}.\n` +
    `Try reinstalling: npm install -g @open330/agt`
  );
  process.exit(1);
}

try {
  execFileSync(binary, process.argv.slice(2), { stdio: "inherit" });
} catch (e) {
  if (e.status !== undefined) {
    process.exit(e.status);
  }
  console.error(`Failed to run agt: ${e.message}`);
  process.exit(1);
}
