const { execFileSync } = require("child_process");
const fs = require("fs");
const path = require("path");
const { PLATFORM_PACKAGES } = require("../lib/platforms");

const rootPackage = JSON.parse(
  fs.readFileSync(path.join(__dirname, "..", "package.json"), "utf8")
);
const platformsDir = path.join(__dirname, "..", "platforms");

function verifyPublishedVersion(name, version) {
  execFileSync("npm", ["view", `${name}@${version}`, "version", "--json"], {
    stdio: "pipe",
  });
}

function readLocalPlatformVersions() {
  const versions = new Map();

  for (const entry of fs.readdirSync(platformsDir, { withFileTypes: true })) {
    if (!entry.isDirectory()) {
      continue;
    }

    const manifestPath = path.join(platformsDir, entry.name, "package.json");
    if (!fs.existsSync(manifestPath)) {
      continue;
    }

    const manifest = JSON.parse(fs.readFileSync(manifestPath, "utf8"));
    versions.set(manifest.name, manifest.version);
  }

  return versions;
}

function main() {
  const optionalDependencies = rootPackage.optionalDependencies || {};
  const localVersions = readLocalPlatformVersions();
  const failures = [];

  for (const [platform, name] of Object.entries(PLATFORM_PACKAGES)) {
    if (!Object.hasOwn(optionalDependencies, name)) {
      failures.push(
        `Wrapper platform ${platform} maps to ${name}, but it is missing from optionalDependencies.`
      );
    }
  }

  for (const name of Object.keys(optionalDependencies)) {
    if (!Object.values(PLATFORM_PACKAGES).includes(name)) {
      failures.push(
        `optionalDependency ${name} is not reachable from the wrapper platform map.`
      );
    }
  }

  for (const [name, version] of Object.entries(optionalDependencies)) {
    const localVersion = localVersions.get(name);
    if (!localVersion) {
      failures.push(`Missing local platform manifest for ${name}.`);
      continue;
    }

    if (localVersion !== version) {
      failures.push(
        `Local manifest version mismatch for ${name}: optionalDependency=${version}, local=${localVersion}.`
      );
      continue;
    }

    try {
      verifyPublishedVersion(name, version);
    } catch (error) {
      const message =
        error.stderr?.toString("utf8").trim() || error.message || String(error);
      failures.push(`Published package missing for ${name}@${version}: ${message}`);
    }
  }

  if (failures.length > 0) {
    console.error("[agt] Platform package verification failed:");
    for (const failure of failures) {
      console.error(`- ${failure}`);
    }
    process.exit(1);
  }

  console.log(
    `[agt] Verified ${Object.keys(optionalDependencies).length} platform package versions.`
  );
}

main();
