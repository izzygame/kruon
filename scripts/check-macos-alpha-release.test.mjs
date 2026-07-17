import assert from "node:assert/strict";
import test from "node:test";

import {
  loadReleaseInputs,
  missingSigningEnvironment,
  parseCargoPackageVersion,
  validateReleaseConfiguration,
} from "./check-macos-alpha-release.mjs";

test("repository macOS Alpha package contract is internally consistent", () => {
  assert.deepEqual(validateReleaseConfiguration(loadReleaseInputs()), []);
});

test("version or bundle drift fails closed", () => {
  const inputs = loadReleaseInputs();
  inputs.desktopPackage = { ...inputs.desktopPackage, version: "0.1.1" };
  inputs.tauri = { ...inputs.tauri, bundle: { ...inputs.tauri.bundle, active: false } };
  const errors = validateReleaseConfiguration(inputs);
  assert.ok(errors.some((error) => error.includes("version must match")));
  assert.ok(errors.some((error) => error.includes("bundling must be active")));
  assert.equal(parseCargoPackageVersion("[package]\nversion = \"1.2.3\"\n"), "1.2.3");
});

test("signing gate reports names only and accepts either notarization credential family", () => {
  assert.deepEqual(missingSigningEnvironment({}), [
    "APPLE_CERTIFICATE",
    "APPLE_CERTIFICATE_PASSWORD",
    "APPLE_SIGNING_IDENTITY",
    "notarization credentials (APPLE_ID set or APPLE_API set)",
  ]);
  assert.deepEqual(
    missingSigningEnvironment({
      APPLE_CERTIFICATE: "opaque",
      APPLE_CERTIFICATE_PASSWORD: "opaque",
      APPLE_SIGNING_IDENTITY: "opaque",
      APPLE_ID: "opaque",
      APPLE_PASSWORD: "opaque",
      APPLE_TEAM_ID: "opaque",
    }),
    [],
  );
});
