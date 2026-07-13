import { execSync } from "node:child_process";

function check(label, command) {
  try {
    const out = execSync(`${command} 2>/dev/null`, {
      encoding: "utf8",
      timeout: 10_000,
    }).trim();
    const lines = out.split("\n");
    const first = lines[0];
    return { label, ok: true, value: first ?? out };
  } catch {
    return { label, ok: false, value: "not found" };
  }
}

const checks = [
  check("node", "node --version"),
  check("pnpm", "pnpm --version"),
  check("rustc", "rustc --version"),
  check("cargo", "cargo --version"),
  check("tauri-cli", "pnpm --filter @kruon/desktop exec tauri --version"),
];

console.log();
console.log("  kruon development environment check");
console.log();

let allOk = true;
for (const c of checks) {
  const icon = c.ok ? "✓" : "✗";
  console.log(`  ${icon} ${c.label.padEnd(14)} ${c.value}`);
  if (!c.ok) allOk = false;
}

console.log();
if (allOk) {
  console.log("  All dependencies satisfied.");
} else {
  console.log("  Some dependencies are missing. See above.");
}
console.log();

const rustMissing = !checks.find((c) => c.label === "rustc")?.ok;
const tauriMissing = !checks.find((c) => c.label === "tauri-cli")?.ok;

if (rustMissing || tauriMissing) {
  console.log("  Tauri/Rust build blocked locally.");
  console.log("  To build the desktop app, install:");
  console.log("    brew install rustup-init && rustup-init");
  console.log("  The project-local Tauri CLI is installed by pnpm install.");
  console.log();
}

process.exit(allOk ? 0 : 1);
