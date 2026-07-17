import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import { KruonClient, WorldSnapshot } from "../lib/kruon";
import { WorldApp } from "./WorldApp";
import { WorldSceneProps } from "./WorldScene";
import { WORLD_SCENE_BUDGET } from "./budget";

const snapshot: WorldSnapshot = {
  generatedAt: "2026-07-16T00:00:00Z",
  stations: [
    {
      stationId: "codex-desk",
      adapter: "codex",
      runId: "run-1",
      state: "running",
      runStatus: "running",
      sourceSequence: 4,
      updatedAt: "2026-07-16T00:00:00Z",
    },
    {
      stationId: "claude-desk",
      adapter: "claude",
      runId: null,
      state: "sleeping",
      runStatus: null,
      sourceSequence: 0,
      updatedAt: null,
    },
  ],
};

function TestScene({ stations, onSelectRun }: WorldSceneProps) {
  const station = stations[0];
  if (!station) return null;
  return (
    <button type="button" onClick={() => station.runId && onSelectRun(station.runId)}>
      Select projected desk
    </button>
  );
}

describe("WorldApp", () => {
  it("reads only the world snapshot and focuses the same run in the 2D window", async () => {
    const invoke = vi.fn(async (command: string) => {
      if (command === "get_world_snapshot") return snapshot;
      if (command === "focus_main_run") return undefined;
      throw new Error(`Unexpected command ${command}`);
    });
    const client = { invoke } as unknown as KruonClient;
    render(<WorldApp client={client} SceneComponent={TestScene} />);

    expect(await screen.findByText("Codex")).toBeDefined();
    expect(screen.getByText("sleeping")).toBeDefined();
    fireEvent.click(screen.getByRole("button", { name: "Select projected desk" }));
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("focus_main_run", { runId: "run-1" }));
  });

  it("keeps the frozen low-power scene budget small", () => {
    expect(WORLD_SCENE_BUDGET.maxAgents).toBe(2);
    expect(WORLD_SCENE_BUDGET.maxMeshes).toBeLessThanOrEqual(32);
    expect(WORLD_SCENE_BUDGET.maxTextures).toBe(0);
    expect(WORLD_SCENE_BUDGET.hiddenPollMs).toBeGreaterThan(WORLD_SCENE_BUDGET.visiblePollMs);
  });
});
