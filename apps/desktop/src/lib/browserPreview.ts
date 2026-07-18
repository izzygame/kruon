import {
  AdapterConnection,
  KruonClient,
  QueueEntry,
  RunSnapshot,
  TaskRecord,
  WorkspaceRecord,
} from "./kruon";

const workspace: WorkspaceRecord = {
  workspaceId: "visual-preview-workspace",
  root: "D:/projects/kruon",
  displayName: "kruon desktop",
  trusted: true,
  createdAt: "2026-07-18T00:00:00Z",
  updatedAt: "2026-07-18T00:00:00Z",
};

const tasks: TaskRecord[] = [
  {
    taskId: "visual-preview-task",
    workspaceId: workspace.workspaceId,
    title: "Shape the crew workspace",
    goal: "Turn the task console into a calm, spatial control surface.",
    context: "Browser-only visual preview. No local process is started.",
    allowedPaths: ["apps/desktop/src"],
    acceptanceCriteria: "The main control shell stays usable beside the world view.",
    testPlan: "Render the preview and inspect the layout at desktop size.",
    rollbackPlan: "Discard the browser preview data.",
    createdAt: "2026-07-18T00:00:00Z",
    updatedAt: "2026-07-18T00:00:00Z",
  },
];

const runs: RunSnapshot[] = [
  {
    runId: "visual-preview-codex-run",
    adapter: "codex",
    workspaceRoot: workspace.root,
    workingDirectory: workspace.root,
    policyId: "preview:read_only",
    status: "running",
    terminalState: null,
    createdAt: "2026-07-18T00:00:00Z",
    updatedAt: "2026-07-18T00:02:00Z",
    lastSequence: 18,
    promptHash: "preview-only",
    launchFingerprint: "preview-only",
    pid: null,
    pgid: null,
  },
  {
    runId: "visual-preview-claude-run",
    adapter: "claude",
    workspaceRoot: workspace.root,
    workingDirectory: workspace.root,
    policyId: "preview:read_only",
    status: "planning",
    terminalState: null,
    createdAt: "2026-07-18T00:00:00Z",
    updatedAt: "2026-07-18T00:01:00Z",
    lastSequence: 7,
    promptHash: "preview-only",
    launchFingerprint: "preview-only",
    pid: null,
    pgid: null,
  },
];

const queue: QueueEntry[] = runs.map((run, index) => ({
  queueId: `visual-preview-queue-${index}`,
  taskId: tasks[0]!.taskId,
  adapter: run.adapter,
  state: "started",
  runId: run.runId,
  timeoutMs: 60_000,
  failureCode: null,
  createdAt: run.createdAt,
  updatedAt: run.updatedAt,
}));

const connections: AdapterConnection[] = [
  {
    adapter: "codex",
    command: "codex",
    status: "ready",
    version: "preview",
    normalizedVersion: "0.144.2",
    compatibility: "supported",
    supportedVersions: ["0.144.2"],
    authentication: "authenticated",
    approvalMode: "sandbox_policy_only",
    capabilities: ["read_only", "replay"],
    detail: "Visual-preview connection; no CLI process is invoked.",
  },
  {
    adapter: "claude",
    command: "claude",
    status: "ready",
    version: "preview",
    normalizedVersion: "2.1.211",
    compatibility: "supported",
    supportedVersions: ["2.1.211"],
    authentication: "authenticated",
    approvalMode: "sandbox_policy_only",
    capabilities: ["read_only", "replay"],
    detail: "Visual-preview connection; no CLI process is invoked.",
  },
];

/** Keeps plain-browser visual review honest and entirely in-memory. */
export const browserPreviewClient: KruonClient = {
  async invoke<T>(command: string): Promise<T> {
    const result: unknown = (() => {
      switch (command) {
        case "probe_connections": return connections;
        case "list_workspaces": return [workspace];
        case "list_tasks": return tasks;
        case "list_queue": return queue;
        case "list_runs": return runs;
        case "latest_task_reviews":
        case "list_events":
        case "list_approvals":
        case "list_artifacts":
        case "list_run_audit": return [];
        case "get_recovery_advice": return [];
        case "get_pause_capability": return { supported: false, message: "Preview only" };
        case "open_world_view": return undefined;
        default: throw new Error(`browser_preview_unsupported: ${command}`);
      }
    })();
    return result as T;
  },
};

export function hasTauriRuntime(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}
