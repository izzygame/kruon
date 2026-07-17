import assert from "node:assert/strict";
import test from "node:test";

import {
  loadSecurityInputs,
  validateSecurityConfiguration,
} from "./check-alpha-security.mjs";

test("repository Alpha security contract is internally consistent", () => {
  assert.deepEqual(validateSecurityConfiguration(loadSecurityInputs()), []);
});

test("world control capability or direct start registration fails closed", () => {
  const inputs = loadSecurityInputs();
  inputs.worldPermission = inputs.worldPermission.replace(
    '"focus_main_run"',
    '"focus_main_run", "cancel_run"',
  );
  inputs.buildRs = inputs.buildRs.replace('"cancel_run",', '"start_run",\n            "cancel_run",');
  const errors = validateSecurityConfiguration(inputs);
  assert.ok(errors.some((error) => error.includes("projection-only")));
  assert.ok(errors.some((error) => error.includes("start_run")));
});

test("unsafe WebView script, writable adapter, or followable database fails closed", () => {
  const inputs = loadSecurityInputs();
  inputs.tauri = {
    ...inputs.tauri,
    app: {
      ...inputs.tauri.app,
      security: { csp: `${inputs.tauri.app.security.csp}; script-src 'unsafe-eval'` },
    },
  };
  inputs.adapterHostRs = inputs.adapterHostRs.replace('"read-only".into()', '"workspace-write".into()');
  inputs.databaseRs = inputs.databaseRs.replace("SQLITE_OPEN_NOFOLLOW", "SQLITE_OPEN_READ_ONLY");
  const errors = validateSecurityConfiguration(inputs);
  assert.ok(errors.some((error) => error.includes("unsafe inline or eval")));
  assert.ok(errors.some((error) => error.includes("adapter contract drifted")));
  assert.ok(errors.some((error) => error.includes("symbolic-link database")));
});
