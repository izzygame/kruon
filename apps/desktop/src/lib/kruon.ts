import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

export type AdapterKind = "codex" | "claude";

export interface AdapterConnection {
  adapter: AdapterKind;
  command: string;
  status: "ready" | "not_found" | "version_check_failed" | "unsupported_version";
  version: string | null;
  normalizedVersion: string | null;
  compatibility: "supported" | "unsupported" | "unverified";
  supportedVersions: string[];
  authentication: "authenticated" | "unauthenticated" | "unknown";
  approvalMode: string;
  capabilities: string[];
  detail: string;
}

export interface WorkspaceRecord {
  workspaceId: string;
  root: string;
  displayName: string;
  trusted: boolean;
  createdAt: string;
  updatedAt: string;
}

export interface TaskRecord {
  taskId: string;
  workspaceId: string;
  title: string;
  goal: string;
  context: string;
  allowedPaths: string[];
  acceptanceCriteria: string;
  testPlan: string;
  rollbackPlan: string;
  createdAt: string;
  updatedAt: string;
}

export interface QueueEntry {
  queueId: string;
  taskId: string;
  adapter: AdapterKind;
  state: "queued" | "started" | "failed";
  runId: string | null;
  timeoutMs: number | null;
  failureCode: string | null;
  createdAt: string;
  updatedAt: string;
}

export interface RunSnapshot {
  runId: string;
  adapter: AdapterKind;
  workspaceRoot: string;
  workingDirectory: string;
  policyId: string | null;
  status: string;
  terminalState: string | null;
  createdAt: string;
  updatedAt: string;
  lastSequence: number;
  promptHash: string;
  launchFingerprint: string;
  pid: number | null;
  pgid: number | null;
}

export interface EventEnvelope {
  eventId: string;
  runId: string;
  sequence: number;
  eventType: string;
  phase: string;
  occurredAt: string;
  terminalState?: string | null;
  payload: Record<string, unknown>;
}

export interface WorkspaceCreateRequest {
  root: string;
  displayName: string;
}

export interface TaskCreateRequest {
  workspaceId: string;
  title: string;
  goal: string;
  context: string;
  allowedPaths: string[];
  acceptanceCriteria: string;
  testPlan: string;
  rollbackPlan: string;
}

export interface ApprovalRecord {
  approvalId: string;
  runId: string;
  taskId: string;
  adapter: AdapterKind;
  mode: "per_action" | "sandbox_policy_only";
  actionKind: string;
  actionSummary: string;
  parameterFingerprint: string;
  parameters: Record<string, unknown>;
  status: "pending" | "approved" | "rejected" | "expired" | "superseded";
  expiresAt: string;
  createdAt: string;
  updatedAt: string;
  supersedesApprovalId: string | null;
}

export interface ArtifactRecord {
  artifactId: string;
  runId: string;
  taskId: string | null;
  kind: "file" | "diff" | "test" | "completion_report";
  path: string | null;
  inWorkspace: boolean;
  summary: string;
  contentSha256: string | null;
  metadata: Record<string, unknown>;
  sourceEventSequence: number | null;
  createdAt: string;
}

export interface CompletionTestResult {
  name: string;
  status: string;
  detail: string;
}

export interface CompletionReportCreate {
  runId: string;
  taskId: string;
  summary: string;
  tests: CompletionTestResult[];
  changedPaths: string[];
}

export interface TaskReviewRecord {
  reviewId: string;
  taskId: string;
  runId: string;
  status: "accepted" | "returned";
  note: string;
  createdAt: string;
}

export interface AuditRecord {
  auditId: string;
  entityType: string;
  entityId: string;
  eventType: string;
  payload: Record<string, unknown>;
  createdAt: string;
}

export interface RecoveryAdvice {
  code: string;
  message: string;
  canRestartFollowUp: boolean;
}

export interface PauseCapability {
  supported: boolean;
  message: string;
}

export interface DiagnosticExportRecord {
  fileName: string;
  savedIn: "downloads" | "app_data";
  byteCount: number;
  sha256: string;
  generatedAt: string;
  includedRuns: number;
  totalRuns: number;
}

export interface AlphaMetricsExportRecord {
  fileName: string;
  savedIn: "downloads" | "app_data";
  byteCount: number;
  sha256: string;
  generatedAt: string;
  taskCount: number;
  runCount: number;
}

export type WorldAgentState =
  | "idle"
  | "planning"
  | "running"
  | "waiting_approval"
  | "blocked"
  | "reviewing"
  | "completed"
  | "sleeping";

export interface WorldStationProjection {
  stationId: string;
  adapter: AdapterKind;
  runId: string | null;
  state: WorldAgentState;
  runStatus: string | null;
  sourceSequence: number;
  updatedAt: string | null;
}

export interface WorldSnapshot {
  generatedAt: string;
  stations: WorldStationProjection[];
}

export interface KruonClient {
  invoke<T>(command: string, args?: Record<string, unknown>): Promise<T>;
  listenWorldRunSelection?(handler: (runId: string) => void): Promise<() => void>;
}

export const desktopClient: KruonClient = {
  invoke,
  async listenWorldRunSelection(handler) {
    return listen<{ runId: string }>("world-run-selected", (event) => handler(event.payload.runId));
  },
};

export function displayAdapter(adapter: AdapterKind): string {
  return adapter === "codex" ? "Codex" : "Claude Code";
}
