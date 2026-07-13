export type Id = string;

export interface Workspace {
  id: Id;
  name: string;
  rootPath: string;
  policyId: Id;
}

export interface Policy {
  id: Id;
  workspaceId: Id;
  approvalMode: "per_action" | "sandbox_policy_only";
  allowedPaths: readonly string[];
  deniedPaths: readonly string[];
}

export type TaskAcceptance = "pending" | "accepted";

export interface Task {
  id: Id;
  workspaceId: Id;
  title: string;
  acceptanceCriteria: readonly string[];
  acceptance: TaskAcceptance;
}

export type RunStatus =
  | "queued"
  | "planning"
  | "running"
  | "waiting_approval"
  | "blocked"
  | "reviewing"
  | "completed"
  | "failed"
  | "cancelling"
  | "cancelled"
  | "uncertain";

export interface Run {
  id: Id;
  taskId: Id;
  workspaceId: Id;
  adapter: "claude" | "codex" | "opencode";
  status: RunStatus;
}

export type ApprovalDecision = "pending" | "approved" | "denied";

export interface Approval {
  id: Id;
  runId: Id;
  action: string;
  decision: ApprovalDecision;
}

export interface Artifact {
  id: Id;
  runId: Id;
  kind: "patch" | "log" | "report" | "test_result";
  path: string;
  sha256?: string;
}

interface EventBase {
  id: Id;
  sequence: number;
  occurredAt: string;
  runId: Id;
}

export type Event =
  | (EventBase & {
      type: "run.transitioned";
      to: RunStatus;
    })
  | (EventBase & {
      type: "run.state_observed";
      observedState: string;
    })
  | (EventBase & {
      type: "approval.requested";
      approval: Approval;
    })
  | (EventBase & {
      type: "approval.resolved";
      approvalId: Id;
      decision: Exclude<ApprovalDecision, "pending">;
    })
  | (EventBase & {
      type: "task.accepted";
      taskId: Id;
      acceptedBy: string;
    });

export interface DomainState {
  task: Task;
  run: Run;
  approvals: readonly Approval[];
  lastSequence: number;
  appliedEventIds: readonly Id[];
}
