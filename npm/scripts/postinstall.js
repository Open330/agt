const { PLATFORM_PACKAGES } = require("../lib/platforms");

const platform = `${process.platform}-${process.arch}`;
const pkg = PLATFORM_PACKAGES[platform];

if (!pkg) {
  console.warn(`[agt] Unsupported platform: ${platform}`);
  process.exit(0);
}

try {
  require.resolve(`${pkg}/bin/agt`);
  process.exit(0);
} catch {
  console.error(`[agt] Required platform package is unavailable: ${pkg}`);
  console.error("[agt] Reinstall with optional dependencies enabled.");
  process.exit(1);
}
