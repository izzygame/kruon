import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const SCRIPT_PATH = fileURLToPath(import.meta.url);
const ROOT = path.dirname(path.dirname(SCRIPT_PATH));

export function allowedCommands(permissionToml) {
  const match = permissionToml.match(/commands\.allow\s*=\s*\[([\s\S]*?)\]/m);
  if (!match) return [];
  return [...match[1].matchAll(/"([a-z0-9_]+)"/g)].map((item) => item[1]);
}

export function validateSecurityConfiguration(inputs) {
  const errors = [];
  const {
    tauri,
    mainCapability,
    worldCapability,
    mainPermission,
    worldPermission,
    buildRs,
    libRs,
    adapterHostRs,
    processSupervisorRs,
    databaseRs,
  } = inputs;
  const csp = tauri.app?.security?.csp ?? "";
  const mainCommands = allowedCommands(mainPermission);
  const worldCommands = allowedCommands(worldPermission);

  if (!csp.includes("default-src 'self'")) {
    errors.push("CSP must default to packaged self content");
  }
  if (/script-src[^;]*'unsafe-(?:inline|eval)'/i.test(csp)) {
    errors.push("CSP must not allow unsafe inline or eval script execution");
  }
  if (/connect-src[^;]*(?:https?:|wss?:|\*)/i.test(csp.replace("http://ipc.localhost", ""))) {
    errors.push("production CSP must not grant arbitrary network connections to the WebView");
  }
  if (JSON.stringify(mainCapability.windows) !== JSON.stringify(["main"])) {
    errors.push("main control capability must target only the main window");
  }
  if (JSON.stringify(worldCapability.windows) !== JSON.stringify(["world"])) {
    errors.push("world capability must target only the world window");
  }
  if (JSON.stringify([...worldCommands].sort()) !== JSON.stringify(["focus_main_run", "get_world_snapshot"])) {
    errors.push("world window command allowlist must stay projection-only");
  }
  for (const command of ["enqueue_task_run", "untrust_workspace", "export_diagnostic_bundle"]) {
    if (!mainCommands.includes(command)) {
      errors.push(`main control capability is missing security-critical command ${command}`);
    }
  }
  for (const command of ["start_run", "create_approval", "decide_approval"]) {
    if (mainCommands.includes(command) || worldCommands.includes(command)) {
      errors.push(`unreviewed direct control command is capability-exposed: ${command}`);
    }
  }
  if (/"start_run"/.test(buildRs) || /\bstart_run\s*,/.test(libRs)) {
    errors.push("direct start_run must not be registered with the Tauri command manifest");
  }
  for (const marker of [
    '"--sandbox".into(),\n                "read-only".into()',
    '"--ephemeral".into()',
    '"--permission-mode".into(),\n                "plan".into()',
    '"--no-session-persistence".into()',
    '"--no-chrome".into()',
  ]) {
    if (!adapterHostRs.includes(marker)) {
      errors.push(`fixed read-only adapter contract drifted: ${marker.split("\\n")[0]}`);
    }
  }
  if (!processSupervisorRs.includes(".env_clear()")) {
    errors.push("managed child processes must clear the inherited environment");
  }
  if (!databaseRs.includes("SQLITE_OPEN_NOFOLLOW")) {
    errors.push("local SQLite must reject a symbolic-link database target");
  }
  if (!databaseRs.includes("0o700") || !databaseRs.includes("0o600")) {
    errors.push("Unix app-data directory and database permissions must remain private");
  }
  return errors;
}

export function loadSecurityInputs(root = ROOT) {
  const read = (relative) => fs.readFileSync(path.join(root, relative), "utf8");
  const readJson = (relative) => JSON.parse(read(relative));
  return {
    tauri: readJson("apps/desktop/src-tauri/tauri.conf.json"),
    mainCapability: readJson("apps/desktop/src-tauri/capabilities/main-control.json"),
    worldCapability: readJson("apps/desktop/src-tauri/capabilities/world-readonly.json"),
    mainPermission: read("apps/desktop/src-tauri/permissions/main-control-commands.toml"),
    worldPermission: read("apps/desktop/src-tauri/permissions/world-readonly-commands.toml"),
    buildRs: read("apps/desktop/src-tauri/build.rs"),
    libRs: read("apps/desktop/src-tauri/src/lib.rs"),
    adapterHostRs: read("apps/desktop/src-tauri/src/core/adapter_host.rs"),
    processSupervisorRs: read("apps/desktop/src-tauri/src/core/process_supervisor.rs"),
    databaseRs: read("apps/desktop/src-tauri/src/core/database.rs"),
  };
}

function main() {
  const errors = validateSecurityConfiguration(loadSecurityInputs());
  if (errors.length > 0) {
    for (const error of errors) console.error(`security gate: ${error}`);
    process.exitCode = 1;
    return;
  }
  console.log("Alpha repository security contract passed.");
  console.log("This is an internal drift gate, not an independent external security review.");
}

if (process.argv[1] && path.resolve(process.argv[1]) === SCRIPT_PATH) main();
