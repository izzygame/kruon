import type {
  Approval,
  DomainState,
  Event,
  RunStatus,
  Task,
  Run,
} from "./types.ts";

const RUN_STATUSES = new Set<RunStatus>([
  "queued",
  "planning",
  "running",
  "waiting_approval",
  "blocked",
  "reviewing",
  "completed",
  "failed",
  "cancelling",
  "cancelled",
  "uncertain",
]);

const ALLOWED_TRANSITIONS: Readonly<Record<RunStatus, ReadonlySet<RunStatus>>> = {
  queued: new Set(["planning", "cancelling", "failed", "uncertain"]),
  planning: new Set([
    "running",
    "waiting_approval",
    "blocked",
    "cancelling",
    "failed",
    "uncertain",
  ]),
  running: new Set([
    "waiting_approval",
    "blocked",
    "reviewing",
    "completed",
    "failed",
    "cancelling",
    "uncertain",
  ]),
  waiting_approval: new Set([
    "running",
    "blocked",
    "cancelling",
    "failed",
    "uncertain",
  ]),
  blocked: new Set([
    "planning",
    "running",
    "cancelling",
    "failed",
    "uncertain",
  ]),
  reviewing: new Set(["running", "completed", "failed", "cancelling", "uncertain"]),
  completed: new Set(),
  failed: new Set(),
  cancelling: new Set(["cancelled", "failed", "uncertain"]),
  cancelled: new Set(),
  uncertain: new Set(RUN_STATUSES),
};

export class DomainTransitionError extends Error {
  override readonly name = "DomainTransitionError";
}

export function initialState(task: Task, run: Run): DomainState {
  if (run.taskId !== task.id) {
    throw new DomainTransitionError("run.taskId must match task.id");
  }
  if (task.acceptance !== "pending") {
    throw new DomainTransitionError("a new aggregate must begin with pending acceptance");
  }
  return {
    task: { ...task, acceptanceCriteria: [...task.acceptanceCriteria] },
    run: { ...run },
    approvals: [],
    lastSequence: 0,
    appliedEventIds: [],
  };
}

function assertEnvelope(state: DomainState, event: Event): void {
  if (event.runId !== state.run.id) {
    throw new DomainTransitionError(`event run ${event.runId} does not match ${state.run.id}`);
  }
  if (state.appliedEventIds.includes(event.id)) {
    throw new DomainTransitionError(`duplicate event id: ${event.id}`);
  }
  const expected = state.lastSequence + 1;
  if (event.sequence !== expected) {
    throw new DomainTransitionError(
      `out-of-order event sequence: expected ${expected}, received ${event.sequence}`,
    );
  }
}

function moveRun(state: DomainState, to: RunStatus): DomainState {
  const from = state.run.status;
  if (from === to) {
    throw new DomainTransitionError(`run is already ${to}`);
  }
  if (!ALLOWED_TRANSITIONS[from].has(to)) {
    throw new DomainTransitionError(`illegal run transition: ${from} -> ${to}`);
  }
  return { ...state, run: { ...state.run, status: to } };
}

function requestApproval(state: DomainState, approval: Approval): DomainState {
  if (approval.runId !== state.run.id) {
    throw new DomainTransitionError("approval belongs to a different run");
  }
  if (approval.decision !== "pending") {
    throw new DomainTransitionError("a requested approval must begin pending");
  }
  if (state.approvals.some((candidate) => candidate.id === approval.id)) {
    throw new DomainTransitionError(`duplicate approval id: ${approval.id}`);
  }
  const waiting = moveRun(state, "waiting_approval");
  return { ...waiting, approvals: [...waiting.approvals, { ...approval }] };
}

function resolveApproval(
  state: DomainState,
  approvalId: string,
  decision: "approved" | "denied",
): DomainState {
  const approval = state.approvals.find((candidate) => candidate.id === approvalId);
  if (!approval) {
    throw new DomainTransitionError(`approval not found: ${approvalId}`);
  }
  if (approval.decision !== "pending") {
    throw new DomainTransitionError(`approval already resolved: ${approvalId}`);
  }
  if (state.run.status !== "waiting_approval") {
    throw new DomainTransitionError("approval may only resolve while waiting_approval");
  }
  const nextStatus: RunStatus = decision === "approved" ? "running" : "blocked";
  const moved = moveRun(state, nextStatus);
  return {
    ...moved,
    approvals: moved.approvals.map((candidate) =>
      candidate.id === approvalId ? { ...candidate, decision } : candidate,
    ),
  };
}

function applyEvent(state: DomainState, event: Event): DomainState {
  switch (event.type) {
    case "run.transitioned":
      return moveRun(state, event.to);
    case "run.state_observed": {
      const observed = event.observedState as RunStatus;
      const target = RUN_STATUSES.has(observed) ? observed : "uncertain";
      if (target === state.run.status) return state;
      return moveRun(state, target);
    }
    case "approval.requested":
      return requestApproval(state, event.approval);
    case "approval.resolved":
      return resolveApproval(state, event.approvalId, event.decision);
    case "task.accepted":
      if (event.taskId !== state.task.id) {
        throw new DomainTransitionError("accepted task does not match the run task");
      }
      if (state.run.status !== "completed") {
        throw new DomainTransitionError("task acceptance requires a completed run");
      }
      if (state.task.acceptance === "accepted") {
        throw new DomainTransitionError("task is already accepted");
      }
      return { ...state, task: { ...state.task, acceptance: "accepted" } };
  }
}

export function transition(state: DomainState, event: Event): DomainState {
  assertEnvelope(state, event);
  const applied = applyEvent(state, event);
  return {
    ...applied,
    lastSequence: event.sequence,
    appliedEventIds: [...state.appliedEventIds, event.id],
  };
}

export function replay(initial: DomainState, events: readonly Event[]): DomainState {
  return events.reduce(transition, initial);
}
