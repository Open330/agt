#!/usr/bin/env node

const { execFileSync } = require("child_process");
const path = require("path");
const { PLATFORM_PACKAGES } = require("../lib/platforms");

const platform = `${process.platform}-${process.arch}`;
const binName = process.platform === "win32" ? "agt.exe" : "agt";

let binary;

// Try platform-specific optional dependency
const pkg = PLATFORM_PACKAGES[platform];
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
  if (typeof e.status === "number") {
    process.exit(e.status);
  }
  if (e.signal) {
    try {
      process.kill(process.pid, e.signal);
      process.exitCode = 1;
      return;
    } catch (signalError) {
      console.error(
        `Failed to propagate ${e.signal} after agt terminated: ${signalError.message}`
      );
      process.exit(1);
    }
  }
  console.error(`Failed to run agt: ${e.message}`);
  process.exit(1);
}
