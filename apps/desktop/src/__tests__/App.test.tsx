import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import { App } from "../App";
import { AdapterConnection, KruonClient } from "../lib/kruon";

const workspace = {
  workspaceId: "workspace-1",
  root: "D:/projects/kruon",
  displayName: "kruon",
  trusted: false,
  createdAt: "2026-07-15T00:00:00Z",
  updatedAt: "2026-07-15T00:00:00Z",
};

const task = {
  taskId: "task-1",
  workspaceId: workspace.workspaceId,
  title: "Inspect MVP state",
  goal: "Review the local implementation without changing files.",
  context: "M1 validation",
  allowedPaths: ["."],
  acceptanceCriteria: "Report the current state.",
  testPlan: "Run existing checks.",
  rollbackPlan: "No changes are made.",
  createdAt: "2026-07-15T00:00:00Z",
  updatedAt: "2026-07-15T00:00:00Z",
};

const sampleTask = {
  ...task,
  taskId: "sample-task-1",
  title: "Inspect this workspace",
  goal: "Summarize the workspace structure and identify primary development entry points without changing files.",
  context: "Kruon Alpha onboarding sample. This task is read-only and must not modify files.",
  acceptanceCriteria: "Provide a concise structure summary and entry-point list; make no workspace changes.",
  testPlan: "Confirm the response is read-only and no workspace files changed.",
  rollbackPlan: "No rollback is expected because this task must not change files.",
};

const completedRun = {
  runId: "run-1",
  adapter: "codex" as const,
  workspaceRoot: workspace.root,
  workingDirectory: workspace.root,
  policyId: "workspace:workspace-1:read_only",
  status: "completed",
  terminalState: "completed",
  createdAt: "2026-07-15T00:00:00Z",
  updatedAt: "2026-07-15T00:01:00Z",
  lastSequence: 3,
  promptHash: "prompt-hash",
  launchFingerprint: "0123456789abcdef",
  pid: null,
  pgid: null,
};

const readyConnections: AdapterConnection[] = [
  {
    adapter: "codex",
    command: "codex",
    status: "ready",
    version: "codex-cli 0.144.1",
    normalizedVersion: "0.144.1",
    compatibility: "supported",
    supportedVersions: ["0.144.1", "0.144.2"],
    authentication: "authenticated",
    approvalMode: "sandbox_policy_only",
    capabilities: ["read_only", "replay"],
    detail: "resolved via common per-user installation location; version is covered by the Alpha fixture matrix",
  },
  {
    adapter: "claude",
    command: "claude",
    status: "ready",
    version: "2.1.211 (Claude Code)",
    normalizedVersion: "2.1.211",
    compatibility: "supported",
    supportedVersions: ["2.1.205", "2.1.211"],
    authentication: "authenticated",
    approvalMode: "sandbox_policy_only",
    capabilities: ["read_only", "replay"],
    detail: "resolved via common per-user installation location; version is covered by the Alpha fixture matrix",
  },
];

function createClient({
  trusted = false,
  tasks = [],
  queue = [],
  runs = [],
  connections = readyConnections,
  workspacePresent = tasks.length > 0,
  reviews = [],
  failCommand,
}: {
  trusted?: boolean;
  tasks?: typeof task[];
  queue?: Array<Record<string, unknown>>;
  runs?: typeof completedRun[];
  connections?: AdapterConnection[];
  workspacePresent?: boolean;
  reviews?: Array<Record<string, unknown>>;
  failCommand?: string;
} = {}) {
  const invoke = vi.fn(async (command: string) => {
    if (command === failCommand) {
      throw "path_policy_violation: workspace is not trusted or a path is outside its allowed scope";
    }
    switch (command) {
      case "probe_connections":
        return connections;
      case "list_workspaces":
        return workspacePresent ? [{ ...workspace, trusted }] : [];
      case "list_tasks":
        return tasks;
      case "list_queue":
        return queue;
      case "list_runs":
        return runs;
      case "list_events":
      case "list_approvals":
      case "list_artifacts":
      case "list_run_audit":
        return [];
      case "latest_task_reviews":
        return reviews;
      case "get_recovery_advice":
        return [{ code: "review_artifacts", message: "Review artifacts before acceptance.", canRestartFollowUp: true }];
      case "get_pause_capability":
        return { supported: false, message: "Pause is unavailable for the fixed noninteractive adapters." };
      case "create_workspace":
        return workspace;
      case "create_task":
        return task;
      case "create_sample_task":
        return sampleTask;
      case "enqueue_task_run":
        return {
          queueId: "queue-1",
          taskId: task.taskId,
          adapter: "codex",
          state: "queued",
          runId: null,
          timeoutMs: 60_000,
          failureCode: null,
          createdAt: "2026-07-15T00:00:00Z",
          updatedAt: "2026-07-15T00:00:00Z",
        };
      case "open_world_view":
        return undefined;
      case "export_diagnostic_bundle":
        return {
          fileName: "kruon-diagnostics-20260717T010203Z-a1b2c3d4.json",
          savedIn: "downloads",
          byteCount: 2048,
          sha256: "0123456789abcdef",
          generatedAt: "2026-07-17T01:02:03Z",
          includedRuns: runs.length,
          totalRuns: runs.length,
        };
      case "export_alpha_metrics":
        return {
          fileName: "kruon-alpha-metrics-20260718T010203Z-a1b2c3d4.json",
          savedIn: "downloads",
          byteCount: 1024,
          sha256: "abcdef0123456789",
          generatedAt: "2026-07-18T01:02:03Z",
          taskCount: tasks.length,
          runCount: runs.length,
        };
      case "trust_workspace":
        return { ...workspace, trusted: true };
      case "untrust_workspace":
        return { ...workspace, trusted: false };
      default:
        throw new Error(`Unexpected command ${command}`);
    }
  });
  return { client: { invoke } as unknown as KruonClient, invoke };
}

describe("App", () => {
  it("shows both local tool connections and their read-only control data", async () => {
    const notFoundConnections: AdapterConnection[] = [
      readyConnections[0]!,
      {
        ...readyConnections[1]!,
        status: "not_found",
        version: null,
        normalizedVersion: null,
        compatibility: "unverified",
        authentication: "unknown",
        detail: "claude was not found in PATH or common per-user installation locations",
      },
    ];
    const { client } = createClient({ connections: notFoundConnections });
    render(<App client={client} />);

    expect((await screen.findAllByText("Codex")).length).toBeGreaterThan(0);
    expect(screen.getByText("Claude Code")).toBeDefined();
    expect(screen.getAllByText("sandbox_policy_only")).toHaveLength(2);
    expect(screen.getByText(/unverified per-action approval is not exposed/i)).toBeDefined();
    expect(screen.getByText(/resolved via common per-user installation location/i)).toBeDefined();
    expect(screen.getByText(/claude was not found/i)).toBeDefined();
    expect(screen.getByText(/Alpha versions: 0.144.1/i)).toBeDefined();
  });

  it("sends the workspace creation request through the desktop runtime", async () => {
    const { client, invoke } = createClient();
    render(<App client={client} />);
    await screen.findByText("Tool connections");

    fireEvent.change(screen.getByLabelText("Display name"), { target: { value: "kruon" } });
    fireEvent.change(screen.getByLabelText("Workspace root"), { target: { value: "D:/projects/kruon" } });
    fireEvent.click(screen.getByRole("button", { name: "Add workspace" }));

    await waitFor(() => {
      expect(invoke).toHaveBeenCalledWith("create_workspace", {
        request: { root: "D:/projects/kruon", displayName: "kruon" },
      });
    });
  });

  it("blocks noninteractive launch for untrusted workspaces", async () => {
    const { client } = createClient({ tasks: [task] });
    render(<App client={client} />);
    await screen.findByText("Inspect MVP state");

    expect(screen.getByRole("button", { name: "Run with Codex" }).hasAttribute("disabled")).toBe(true);
    expect(screen.getByText("Trust this workspace to enable CLI launch.")).toBeDefined();
  });

  it("lets the user revoke workspace trust through the protected runtime command", async () => {
    const { client, invoke } = createClient({ trusted: true, tasks: [task] });
    render(<App client={client} />);

    fireEvent.click(await screen.findByRole("button", { name: "Revoke trust" }));
    await waitFor(() => {
      expect(invoke).toHaveBeenCalledWith("untrust_workspace", { workspaceId: "workspace-1" });
    });
  });

  it("queues a trusted task using the manually selected executor", async () => {
    const { client, invoke } = createClient({ trusted: true, tasks: [task] });
    render(<App client={client} />);
    await screen.findByText("Inspect MVP state");

    fireEvent.click(screen.getByRole("button", { name: "Run with Claude" }));
    await waitFor(() => {
      expect(invoke).toHaveBeenCalledWith("enqueue_task_run", {
        request: { taskId: "task-1", adapter: "claude", timeoutMs: 60_000 },
      });
    });
  });

  it("creates the idempotent read-only sample from the first-connection guide", async () => {
    const { client, invoke } = createClient({ trusted: true, workspacePresent: true });
    render(<App client={client} />);

    fireEvent.click(await screen.findByRole("button", { name: "Create read-only sample" }));
    await waitFor(() => {
      expect(invoke).toHaveBeenCalledWith("create_sample_task", { workspaceId: "workspace-1" });
    });
  });

  it("queues the onboarding sample through the normal durable run path", async () => {
    const { client, invoke } = createClient({ trusted: true, tasks: [sampleTask] });
    render(<App client={client} />);

    fireEvent.click(await screen.findByRole("button", { name: "Run sample with Codex" }));
    await waitFor(() => {
      expect(invoke).toHaveBeenCalledWith("enqueue_task_run", {
        request: { taskId: "sample-task-1", adapter: "codex", timeoutMs: 60_000 },
      });
    });
  });

  it("carries a workspace draft into the structured task form", async () => {
    const { client } = createClient({ trusted: true });
    render(<App client={client} />);

    const draft = await screen.findByLabelText("Draft a task from the workspace");
    fireEvent.change(draft, { target: { value: "Create a softer review corner" } });
    fireEvent.click(screen.getByRole("button", { name: "Draft task" }));

    expect((screen.getByLabelText("Title") as HTMLInputElement).value).toBe("Create a softer review corner");
    expect((screen.getByLabelText("Goal") as HTMLTextAreaElement).value).toBe("Create a softer review corner");
  });

  it("reconstructs completed onboarding from queue, run, and human-review records", async () => {
    const queue = [{
      queueId: "sample-queue-1",
      taskId: sampleTask.taskId,
      adapter: "codex",
      state: "started",
      runId: completedRun.runId,
      timeoutMs: 60_000,
      failureCode: null,
      createdAt: completedRun.createdAt,
      updatedAt: completedRun.updatedAt,
    }];
    const reviews = [{
      reviewId: "review-1",
      taskId: sampleTask.taskId,
      runId: completedRun.runId,
      status: "accepted",
      note: "Read-only sample checked.",
      createdAt: completedRun.updatedAt,
    }];
    const { client } = createClient({ trusted: true, tasks: [sampleTask], queue, runs: [completedRun], reviews });
    render(<App client={client} />);

    expect(await screen.findByText("First connection loop complete.")).toBeDefined();
    expect(screen.getByText("5 / 5")).toBeDefined();
    expect(screen.getByText(/durable accepted review/i)).toBeDefined();
  });

  it("turns public error codes into concrete recovery guidance", async () => {
    const { client } = createClient({ failCommand: "create_workspace" });
    render(<App client={client} />);
    await screen.findByText("Tool connections");

    fireEvent.change(screen.getByLabelText("Display name"), { target: { value: "kruon" } });
    fireEvent.change(screen.getByLabelText("Workspace root"), { target: { value: "D:/outside" } });
    fireEvent.click(screen.getByRole("button", { name: "Add workspace" }));

    expect(await screen.findByText("Repair workspace trust or scope")).toBeDefined();
    expect(screen.getByText(/keep every allowed path relative to its canonical root/i)).toBeDefined();
    expect(screen.getByText(/Never paste credentials into Kruon/i)).toBeDefined();
  });

  it("blocks an unauthenticated compatible CLI and points login back to the upstream terminal", async () => {
    const connections: AdapterConnection[] = [
      { ...readyConnections[0]!, authentication: "unauthenticated" },
      { ...readyConnections[1]!, status: "not_found", authentication: "unknown" },
    ];
    const { client } = createClient({ trusted: true, tasks: [task], connections });
    render(<App client={client} />);

    const runButton = await screen.findByRole("button", { name: "Run with Codex" });
    expect(runButton.hasAttribute("disabled")).toBe(true);
    expect(screen.getAllByText(/codex login status/i).length).toBeGreaterThan(0);
    expect(screen.getAllByText(/Kruon does not collect credentials/i).length).toBeGreaterThan(0);
  });

  it("disables launch when an installed CLI is outside the Alpha compatibility matrix", async () => {
    const unsupportedConnections: AdapterConnection[] = [
      {
        ...readyConnections[0]!,
        status: "unsupported_version",
        version: "codex-cli 0.145.0",
        normalizedVersion: "0.145.0",
        compatibility: "unsupported",
        detail: "launch blocked because the installed version is outside the Alpha fixture matrix",
      },
      readyConnections[1]!,
    ];
    const { client } = createClient({ trusted: true, tasks: [task], connections: unsupportedConnections });
    render(<App client={client} />);

    const codexButton = await screen.findByRole("button", { name: "Run with Codex" });
    expect(codexButton.hasAttribute("disabled")).toBe(true);
    expect(screen.getAllByText(/Install one of the listed Alpha versions \(0.144.1, 0.144.2\)/i).length).toBeGreaterThan(0);
    expect(screen.getByRole("button", { name: "Run with Claude" }).hasAttribute("disabled")).toBe(false);
  });

  it("shows the M2 handoff controls without claiming per-action approval", async () => {
    const queue = [{
      queueId: "queue-1",
      taskId: task.taskId,
      adapter: "codex",
      state: "started",
      runId: completedRun.runId,
      timeoutMs: 60_000,
      failureCode: null,
      createdAt: completedRun.createdAt,
      updatedAt: completedRun.updatedAt,
    }];
    const { client, invoke } = createClient({ trusted: true, tasks: [task], queue, runs: [completedRun] });
    render(<App client={client} />);

    fireEvent.click(await screen.findByRole("button", { name: /Codex completed/i }));
    expect(await screen.findByText("Diagnostics & handoff")).toBeDefined();
    expect(screen.getByText(/not enabled for the frozen adapters/i)).toBeDefined();
    expect(screen.getByRole("button", { name: "Cancel run" }).hasAttribute("disabled")).toBe(true);
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("get_pause_capability"));
  });

  it("opens the isolated M3 world window without replacing the 2D controls", async () => {
    const { client, invoke } = createClient();
    render(<App client={client} />);

    const button = await screen.findByRole("button", { name: "Open world view" });
    await waitFor(() => expect(button.hasAttribute("disabled")).toBe(false));
    fireEvent.click(button);

    await waitFor(() => expect(invoke).toHaveBeenCalledWith("open_world_view"));
    expect(screen.getByText("Tool connections")).toBeDefined();
  });

  it("exports only the backend-built metadata diagnostic bundle", async () => {
    const { client, invoke } = createClient({ runs: [completedRun] });
    render(<App client={client} />);

    const button = await screen.findByRole("button", { name: "Export diagnostics" });
    await waitFor(() => expect(button.hasAttribute("disabled")).toBe(false));
    fireEvent.click(button);

    await waitFor(() => expect(invoke).toHaveBeenCalledWith("export_diagnostic_bundle"));
    expect(await screen.findByText(/Saved kruon-diagnostics-.* in Downloads/i)).toBeDefined();
    expect(screen.getByText(/no prompts, projects, paths, credentials, or raw logs/i)).toBeDefined();
  });

  it("requires fresh consent before every local aggregate Alpha metrics export", async () => {
    const { client, invoke } = createClient({ trusted: true, tasks: [task], runs: [completedRun] });
    render(<App client={client} />);

    const button = await screen.findByRole("button", { name: "Export consented Alpha metrics" });
    expect(button.hasAttribute("disabled")).toBe(true);
    expect(invoke).not.toHaveBeenCalledWith("export_alpha_metrics", expect.anything());

    fireEvent.click(screen.getByRole("checkbox", { name: /I consent to this one local export/i }));
    await waitFor(() => expect(button.hasAttribute("disabled")).toBe(false));
    fireEvent.click(button);

    await waitFor(() => {
      expect(invoke).toHaveBeenCalledWith("export_alpha_metrics", { consented: true });
    });
    expect(await screen.findByText(/Saved kruon-alpha-metrics-.* in Downloads/i)).toBeDefined();
    expect(screen.getByText(/does not create a participant identifier/i)).toBeDefined();
    expect((screen.getByRole("checkbox", { name: /I consent to this one local export/i }) as HTMLInputElement).checked).toBe(false);
  });
});
