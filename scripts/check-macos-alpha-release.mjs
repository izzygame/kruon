import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const SCRIPT_PATH = fileURLToPath(import.meta.url);
const ROOT = path.dirname(path.dirname(SCRIPT_PATH));
const EXPECTED_ICONS = [
  "icons/32x32.png",
  "icons/128x128.png",
  "icons/128x128@2x.png",
  "icons/icon.icns",
  "icons/icon.ico",
];

export function parseCargoPackageVersion(cargoToml) {
  const marker = "[package]";
  const start = cargoToml.indexOf(marker);
  if (start < 0) return null;
  const remainder = cargoToml.slice(start + marker.length);
  const nextSection = remainder.search(/^\[/m);
  const packageSection = nextSection >= 0 ? remainder.slice(0, nextSection) : remainder;
  return packageSection.match(/^version\s*=\s*"([^"]+)"/m)?.[1] ?? null;
}

export function validateReleaseConfiguration({ tauri, rootPackage, desktopPackage, cargoToml, entitlementsText, fileExists }) {
  const errors = [];
  const versions = [
    ["tauri.conf.json", tauri.version],
    ["apps/desktop/package.json", desktopPackage.version],
    ["apps/desktop/src-tauri/Cargo.toml", parseCargoPackageVersion(cargoToml)],
  ];
  const version = versions[0][1];
  if (!/^\d+\.\d+\.\d+$/.test(version ?? "")) {
    errors.push("tauri.conf.json version must be a stable three-part SemVer");
  }
  for (const [source, candidate] of versions.slice(1)) {
    if (candidate !== version) {
      errors.push(`${source} version must match tauri.conf.json`);
    }
  }
  if (tauri.identifier !== "com.kruon.desktop") {
    errors.push("bundle identifier must remain com.kruon.desktop for upgrade continuity");
  }
  if (tauri.bundle?.active !== true) {
    errors.push("Tauri bundling must be active");
  }
  if (tauri.bundle?.targets !== "all") {
    errors.push("default bundle targets must remain cross-platform; the macOS script narrows to app,dmg");
  }
  if (tauri.bundle?.createUpdaterArtifacts !== false) {
    errors.push("updater artifacts must stay disabled until a real public key and HTTPS endpoint are configured");
  }
  if (tauri.plugins?.updater !== undefined) {
    errors.push("updater configuration must not be added without an approved public key and HTTPS endpoint");
  }
  if (tauri.bundle?.category !== "DeveloperTool") {
    errors.push("bundle category must be DeveloperTool");
  }
  if (JSON.stringify(tauri.bundle?.icon) !== JSON.stringify(EXPECTED_ICONS)) {
    errors.push("bundle icon allowlist does not match the frozen Alpha set");
  }
  for (const icon of EXPECTED_ICONS) {
    if (!fileExists(path.join("apps", "desktop", "src-tauri", icon))) {
      errors.push(`missing bundle icon: ${icon}`);
    }
  }
  if (tauri.bundle?.macOS?.minimumSystemVersion !== "12.0") {
    errors.push("macOS Alpha minimumSystemVersion must be 12.0");
  }
  if (tauri.bundle?.macOS?.hardenedRuntime !== true) {
    errors.push("macOS hardened runtime must be enabled");
  }
  if (tauri.bundle?.macOS?.signingIdentity !== undefined) {
    errors.push("macOS signing identity must come from the CI environment, not repository config");
  }
  const entitlements = tauri.bundle?.macOS?.entitlements;
  if (entitlements !== "entitlements.plist" || !fileExists(path.join("apps", "desktop", "src-tauri", entitlements))) {
    errors.push("the reviewed macOS entitlements file must exist");
  }
  if (/<key>/i.test(entitlementsText) || !/<dict\s*\/>/i.test(entitlementsText)) {
    errors.push("Alpha entitlements must remain an empty dictionary until a reviewed capability requires more");
  }
  if (!fileExists(path.join(".github", "workflows", "macos-alpha.yml"))) {
    errors.push("manual macOS Alpha packaging workflow is missing");
  }
  if (rootPackage.scripts?.["desktop:bundle:macos"] !== "pnpm --filter @kruon/desktop tauri:bundle:macos") {
    errors.push("root macOS bundle script is missing or drifted");
  }
  if (desktopPackage.scripts?.["tauri:bundle:macos"] !== "tauri build --target aarch64-apple-darwin --bundles app,dmg") {
    errors.push("desktop macOS bundle script must target Apple Silicon app and dmg bundles");
  }
  return errors;
}

export function missingSigningEnvironment(environment) {
  const requiredSigning = [
    "APPLE_CERTIFICATE",
    "APPLE_CERTIFICATE_PASSWORD",
    "APPLE_SIGNING_IDENTITY",
  ];
  const appleIdNotarization = ["APPLE_ID", "APPLE_PASSWORD", "APPLE_TEAM_ID"];
  const apiNotarization = ["APPLE_API_ISSUER", "APPLE_API_KEY", "APPLE_API_KEY_PATH"];
  const missing = requiredSigning.filter((name) => !environment[name]);
  const hasAppleId = appleIdNotarization.every((name) => environment[name]);
  const hasApi = apiNotarization.every((name) => environment[name]);
  if (!hasAppleId && !hasApi) {
    missing.push("notarization credentials (APPLE_ID set or APPLE_API set)");
  }
  return missing;
}

export function loadReleaseInputs(root = ROOT) {
  const readJson = (relative) => JSON.parse(fs.readFileSync(path.join(root, relative), "utf8"));
  return {
    tauri: readJson("apps/desktop/src-tauri/tauri.conf.json"),
    rootPackage: readJson("package.json"),
    desktopPackage: readJson("apps/desktop/package.json"),
    cargoToml: fs.readFileSync(path.join(root, "apps/desktop/src-tauri/Cargo.toml"), "utf8"),
    entitlementsText: fs.readFileSync(path.join(root, "apps/desktop/src-tauri/entitlements.plist"), "utf8"),
    fileExists: (relative) => fs.existsSync(path.join(root, relative)),
  };
}

function main() {
  const requireSigning = process.argv.includes("--require-signing-env");
  const errors = validateReleaseConfiguration(loadReleaseInputs());
  if (requireSigning) {
    const missing = missingSigningEnvironment(process.env);
    errors.push(...missing.map((name) => `missing release secret: ${name}`));
  }
  if (errors.length > 0) {
    for (const error of errors) {
      console.error(`release gate: ${error}`);
    }
    process.exitCode = 1;
    return;
  }
  console.log(`macOS Alpha package gate passed${requireSigning ? " with signing/notarization inputs present" : ""}.`);
  console.log("Automatic update remains disabled until its signing public key and HTTPS endpoint are approved.");
}

if (process.argv[1] && path.resolve(process.argv[1]) === SCRIPT_PATH) {
  main();
}
