import { describe, expect, it } from "vitest";

import {
  KENNEY_ASSET_LIMIT_BYTES,
  KENNEY_VISIBLE_TRIANGLE_LIMIT,
  KENNEY_VISIBLE_TRIANGLES,
  KENNEY_WORLD_ASSETS,
  KENNEY_WORLD_PAYLOAD_BYTES,
} from "./assets";

describe("Kenney world asset manifest", () => {
  it("keeps every runtime asset local, hashed, and unique", () => {
    const urls = KENNEY_WORLD_ASSETS.map((asset) => asset.url);

    expect(new Set(urls).size).toBe(urls.length);
    expect(urls.every((url) => url.startsWith("/assets/world/kenney/"))).toBe(true);
    expect(urls.every((url) => !url.startsWith("http"))).toBe(true);
    expect(KENNEY_WORLD_ASSETS.every((asset) => /^[A-F0-9]{64}$/.test(asset.sha256))).toBe(true);
  });

  it("stays inside the frozen M3.1 payload and geometry budgets", () => {
    expect(KENNEY_WORLD_PAYLOAD_BYTES).toBe(606_458);
    expect(KENNEY_WORLD_PAYLOAD_BYTES).toBeLessThan(KENNEY_ASSET_LIMIT_BYTES);
    expect(KENNEY_VISIBLE_TRIANGLES).toBe(3_581);
    expect(KENNEY_VISIBLE_TRIANGLES).toBeLessThan(KENNEY_VISIBLE_TRIANGLE_LIMIT);
  });
});
