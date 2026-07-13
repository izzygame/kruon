import assert from "node:assert/strict";
import test from "node:test";

import { DomainTransitionError, initialState, replay, transition } from "./state-machine.ts";
import type { Event, Run, Task } from "./types.ts";

const task = (): Task => ({
  id: "task-1",
  workspaceId: "workspace-1",
  title: "Build the MVP shell",
  acceptanceCriteria: ["tests pass"],
  acceptance: "pending",
});

const run = (): Run => ({
  id: "run-1",
  taskId: "task-1",
  workspaceId: "workspace-1",
  adapter: "opencode",
  status: "queued",
});

const event = <T extends Event>(value: Omit<T, "occurredAt" | "runId">): T =>
  ({ ...value, occurredAt: "2026-07-13T00:00:00Z", runId: "run-1" }) as T;

test("replays a legal run through completion", () => {
  const result = replay(initialState(task(), run()), [
    event({ id: "e1", sequence: 1, type: "run.transitioned", to: "planning" }),
    event({ id: "e2", sequence: 2, type: "run.transitioned", to: "running" }),
    event({ id: "e3", sequence: 3, type: "run.transitioned", to: "reviewing" }),
    event({ id: "e4", sequence: 4, type: "run.transitioned", to: "completed" }),
  ]);

  assert.equal(result.run.status, "completed");
  assert.equal(result.task.acceptance, "pending");
  assert.equal(result.lastSequence, 4);
});

test("rejects an illegal transition", () => {
  assert.throws(
    () =>
      transition(
        initialState(task(), run()),
        event({ id: "e1", sequence: 1, type: "run.transitioned", to: "completed" }),
      ),
    DomainTransitionError,
  );
});

test("rejects duplicate event ids", () => {
  const start = initialState(task(), run());
  const once = transition(
    start,
    event({ id: "same", sequence: 1, type: "run.transitioned", to: "planning" }),
  );
  assert.throws(
    () =>
      transition(
        once,
        event({ id: "same", sequence: 2, type: "run.transitioned", to: "running" }),
      ),
    /duplicate event id/,
  );
});

test("rejects gaps and non-increasing event sequences", () => {
  const start = initialState(task(), run());
  assert.throws(
    () =>
      transition(
        start,
        event({ id: "e2", sequence: 2, type: "run.transitioned", to: "planning" }),
      ),
    /out-of-order/,
  );
  const once = transition(
    start,
    event({ id: "e1", sequence: 1, type: "run.transitioned", to: "planning" }),
  );
  assert.throws(
    () =>
      transition(
        once,
        event({ id: "e0", sequence: 1, type: "run.transitioned", to: "running" }),
      ),
    /out-of-order/,
  );
});

test("models approval request and approval resolution", () => {
  const result = replay(initialState(task(), run()), [
    event({ id: "e1", sequence: 1, type: "run.transitioned", to: "planning" }),
    event({ id: "e2", sequence: 2, type: "run.transitioned", to: "running" }),
    event({
      id: "e3",
      sequence: 3,
      type: "approval.requested",
      approval: { id: "approval-1", runId: "run-1", action: "write", decision: "pending" },
    }),
    event({
      id: "e4",
      sequence: 4,
      type: "approval.resolved",
      approvalId: "approval-1",
      decision: "approved",
    }),
  ]);

  assert.equal(result.run.status, "running");
  assert.equal(result.approvals[0]?.decision, "approved");
});

test("a denied approval blocks the run", () => {
  const result = replay(initialState(task(), run()), [
    event({ id: "e1", sequence: 1, type: "run.transitioned", to: "planning" }),
    event({
      id: "e2",
      sequence: 2,
      type: "approval.requested",
      approval: { id: "approval-1", runId: "run-1", action: "shell", decision: "pending" },
    }),
    event({
      id: "e3",
      sequence: 3,
      type: "approval.resolved",
      approvalId: "approval-1",
      decision: "denied",
    }),
  ]);
  assert.equal(result.run.status, "blocked");
});

test("supports cancellation and failure paths", () => {
  const cancelling = replay(initialState(task(), run()), [
    event({ id: "e1", sequence: 1, type: "run.transitioned", to: "planning" }),
    event({ id: "e2", sequence: 2, type: "run.transitioned", to: "cancelling" }),
    event({ id: "e3", sequence: 3, type: "run.transitioned", to: "cancelled" }),
  ]);
  assert.equal(cancelling.run.status, "cancelled");

  const failed = replay(initialState(task(), run()), [
    event({ id: "f1", sequence: 1, type: "run.transitioned", to: "planning" }),
    event({ id: "f2", sequence: 2, type: "run.transitioned", to: "failed" }),
  ]);
  assert.equal(failed.run.status, "failed");
});

test("completion and task acceptance are separate explicit events", () => {
  const completed = replay(initialState(task(), run()), [
    event({ id: "e1", sequence: 1, type: "run.transitioned", to: "planning" }),
    event({ id: "e2", sequence: 2, type: "run.transitioned", to: "running" }),
    event({ id: "e3", sequence: 3, type: "run.transitioned", to: "completed" }),
  ]);
  assert.equal(completed.task.acceptance, "pending");

  const accepted = transition(
    completed,
    event({
      id: "e4",
      sequence: 4,
      type: "task.accepted",
      taskId: "task-1",
      acceptedBy: "owner",
    }),
  );
  assert.equal(accepted.task.acceptance, "accepted");
});

test("an unknown observed adapter state remains uncertain", () => {
  const result = replay(initialState(task(), run()), [
    event({ id: "e1", sequence: 1, type: "run.state_observed", observedState: "lost_signal" }),
  ]);
  assert.equal(result.run.status, "uncertain");
  assert.equal(result.task.acceptance, "pending");
});

test("state_observed matching current status is a no-op, not an error", () => {
  const start = initialState(task(), run());
  const planned = transition(
    start,
    event({ id: "e1", sequence: 1, type: "run.transitioned", to: "planning" }),
  );
  // Observing "planning" while already planning should not throw
  const same = transition(
    planned,
    event({ id: "e2", sequence: 2, type: "run.state_observed", observedState: "planning" }),
  );
  assert.equal(same.run.status, "planning");
  assert.equal(same.lastSequence, 2);
});

test("direct cancelled from non-cancelling states is rejected", () => {
  const start = initialState(task(), run());
  assert.throws(
    () =>
      transition(
        start,
        event({ id: "e1", sequence: 1, type: "run.transitioned", to: "cancelled" }),
      ),
    /illegal run transition/,
  );
  const planned = transition(
    start,
    event({ id: "e1", sequence: 1, type: "run.transitioned", to: "planning" }),
  );
  assert.throws(
    () =>
      transition(
        planned,
        event({ id: "e2", sequence: 2, type: "run.transitioned", to: "cancelled" }),
      ),
    /illegal run transition/,
  );
});

test("cancelling must go through cancelling before cancelled", () => {
  const result = replay(initialState(task(), run()), [
    event({ id: "e1", sequence: 1, type: "run.transitioned", to: "planning" }),
    event({ id: "e2", sequence: 2, type: "run.transitioned", to: "cancelling" }),
    event({ id: "e3", sequence: 3, type: "run.transitioned", to: "cancelled" }),
  ]);
  assert.equal(result.run.status, "cancelled");
});

test("cancelling can also go to failed or uncertain", () => {
  const failed = replay(initialState(task(), run()), [
    event({ id: "e1", sequence: 1, type: "run.transitioned", to: "planning" }),
    event({ id: "e2", sequence: 2, type: "run.transitioned", to: "cancelling" }),
    event({ id: "e3", sequence: 3, type: "run.transitioned", to: "failed" }),
  ]);
  assert.equal(failed.run.status, "failed");

  const uncertain = replay(initialState(task(), run()), [
    event({ id: "e1", sequence: 1, type: "run.transitioned", to: "planning" }),
    event({ id: "e2", sequence: 2, type: "run.transitioned", to: "cancelling" }),
    event({ id: "e3", sequence: 3, type: "run.transitioned", to: "uncertain" }),
  ]);
  assert.equal(uncertain.run.status, "uncertain");
});

test("reviewing can go to completed or back to running (rework)", () => {
  const completed = replay(initialState(task(), run()), [
    event({ id: "e1", sequence: 1, type: "run.transitioned", to: "planning" }),
    event({ id: "e2", sequence: 2, type: "run.transitioned", to: "running" }),
    event({ id: "e3", sequence: 3, type: "run.transitioned", to: "reviewing" }),
    event({ id: "e4", sequence: 4, type: "run.transitioned", to: "completed" }),
  ]);
  assert.equal(completed.run.status, "completed");

  const rework = replay(initialState(task(), run()), [
    event({ id: "e1", sequence: 1, type: "run.transitioned", to: "planning" }),
    event({ id: "e2", sequence: 2, type: "run.transitioned", to: "running" }),
    event({ id: "e3", sequence: 3, type: "run.transitioned", to: "reviewing" }),
    event({ id: "e4", sequence: 4, type: "run.transitioned", to: "running" }),
  ]);
  assert.equal(rework.run.status, "running");
});

test("blocked can retry via planning or running", () => {
  const planning = replay(initialState(task(), run()), [
    event({ id: "e1", sequence: 1, type: "run.transitioned", to: "planning" }),
    event({ id: "e2", sequence: 2, type: "run.transitioned", to: "blocked" }),
    event({ id: "e3", sequence: 3, type: "run.transitioned", to: "planning" }),
  ]);
  assert.equal(planning.run.status, "planning");

  const running = replay(initialState(task(), run()), [
    event({ id: "e1", sequence: 1, type: "run.transitioned", to: "planning" }),
    event({ id: "e2", sequence: 2, type: "run.transitioned", to: "blocked" }),
    event({ id: "e3", sequence: 3, type: "run.transitioned", to: "running" }),
  ]);
  assert.equal(running.run.status, "running");
});

test("terminal states reject any further transition", () => {
  const start = initialState(task(), run());
  const completed = replay(start, [
    event({ id: "e1", sequence: 1, type: "run.transitioned", to: "planning" }),
    event({ id: "e2", sequence: 2, type: "run.transitioned", to: "running" }),
    event({ id: "e3", sequence: 3, type: "run.transitioned", to: "completed" }),
  ]);
  assert.throws(
    () =>
      transition(
        completed,
        event({ id: "e4", sequence: 4, type: "run.transitioned", to: "uncertain" }),
      ),
    /illegal run transition/,
  );

  const failed = replay(start, [
    event({ id: "e1", sequence: 1, type: "run.transitioned", to: "planning" }),
    event({ id: "e2", sequence: 2, type: "run.transitioned", to: "failed" }),
  ]);
  assert.throws(
    () =>
      transition(
        failed,
        event({ id: "e3", sequence: 3, type: "run.transitioned", to: "uncertain" }),
      ),
    /illegal run transition/,
  );
});

test("task.accepted before run completed is rejected", () => {
  const start = initialState(task(), run());
  const running = replay(start, [
    event({ id: "e1", sequence: 1, type: "run.transitioned", to: "planning" }),
    event({ id: "e2", sequence: 2, type: "run.transitioned", to: "running" }),
  ]);
  assert.throws(
    () =>
      transition(
        running,
        event({
          id: "e3",
          sequence: 3,
          type: "task.accepted",
          taskId: "task-1",
          acceptedBy: "owner",
        }),
      ),
    /task acceptance requires a completed run/,
  );
});

test("task.accepted when already accepted is rejected", () => {
  const start = initialState(task(), run());
  const completed = replay(start, [
    event({ id: "e1", sequence: 1, type: "run.transitioned", to: "planning" }),
    event({ id: "e2", sequence: 2, type: "run.transitioned", to: "running" }),
    event({ id: "e3", sequence: 3, type: "run.transitioned", to: "completed" }),
    event({
      id: "e4",
      sequence: 4,
      type: "task.accepted",
      taskId: "task-1",
      acceptedBy: "owner",
    }),
  ]);
  assert.throws(
    () =>
      transition(
        completed,
        event({
          id: "e5",
          sequence: 5,
          type: "task.accepted",
          taskId: "task-1",
          acceptedBy: "owner",
        }),
      ),
    /task is already accepted/,
  );
});

test("approval.resolved when not waiting_approval is rejected", () => {
  const start = initialState(task(), run());
  // Request an approval (moves to waiting_approval), then observe back to running
  const running = replay(start, [
    event({ id: "e1", sequence: 1, type: "run.transitioned", to: "planning" }),
    event({
      id: "e2",
      sequence: 2,
      type: "approval.requested",
      approval: { id: "approval-1", runId: "run-1", action: "write", decision: "pending" },
    }),
    event({
      id: "e3",
      sequence: 3,
      type: "run.state_observed",
      observedState: "running",
    }),
  ]);
  assert.equal(running.run.status, "running");
  assert.throws(
    () =>
      transition(
        running,
        event({
          id: "e4",
          sequence: 4,
          type: "approval.resolved",
          approvalId: "approval-1",
          decision: "approved",
        }),
      ),
    /approval may only resolve while waiting_approval/,
  );
});

test("approval.resolved with non-existent id is rejected", () => {
  const start = initialState(task(), run());
  const waiting = replay(start, [
    event({ id: "e1", sequence: 1, type: "run.transitioned", to: "planning" }),
    event({
      id: "e2",
      sequence: 2,
      type: "approval.requested",
      approval: { id: "approval-1", runId: "run-1", action: "write", decision: "pending" },
    }),
  ]);
  assert.throws(
    () =>
      transition(
        waiting,
        event({
          id: "e3",
          sequence: 3,
          type: "approval.resolved",
          approvalId: "ghost",
          decision: "approved",
        }),
      ),
    /approval not found/,
  );
});

test("approval.resolved on already-resolved approval is rejected", () => {
  const start = initialState(task(), run());
  const resolved = replay(start, [
    event({ id: "e1", sequence: 1, type: "run.transitioned", to: "planning" }),
    event({
      id: "e2",
      sequence: 2,
      type: "approval.requested",
      approval: { id: "approval-1", runId: "run-1", action: "write", decision: "pending" },
    }),
    event({
      id: "e3",
      sequence: 3,
      type: "approval.resolved",
      approvalId: "approval-1",
      decision: "approved",
    }),
  ]);
  assert.throws(
    () =>
      transition(
        resolved,
        event({
          id: "e4",
          sequence: 4,
          type: "approval.resolved",
          approvalId: "approval-1",
          decision: "denied",
        }),
      ),
    /approval already resolved/,
  );
});

test("approval.requested with non-pending decision is rejected", () => {
  const start = initialState(task(), run());
  const planned = transition(
    start,
    event({ id: "e1", sequence: 1, type: "run.transitioned", to: "planning" }),
  );
  assert.throws(
    () =>
      transition(
        planned,
        event({
          id: "e2",
          sequence: 2,
          type: "approval.requested",
          approval: { id: "approval-1", runId: "run-1", action: "write", decision: "approved" },
        }),
      ),
    /must begin pending/,
  );
});

test("approval.requested with duplicate id is rejected", () => {
  const start = initialState(task(), run());
  const waiting = replay(start, [
    event({ id: "e1", sequence: 1, type: "run.transitioned", to: "planning" }),
    event({
      id: "e2",
      sequence: 2,
      type: "approval.requested",
      approval: { id: "approval-1", runId: "run-1", action: "write", decision: "pending" },
    }),
  ]);
  assert.throws(
    () =>
      transition(
        waiting,
        event({
          id: "e3",
          sequence: 3,
          type: "approval.requested",
          approval: { id: "approval-1", runId: "run-1", action: "shell", decision: "pending" },
        }),
      ),
    /duplicate approval id/,
  );
});

test("uncertain can transition to any known status", () => {
  const start = initialState(task(), run());
  const uncertain = transition(
    start,
    event({ id: "e1", sequence: 1, type: "run.state_observed", observedState: "lost_signal" }),
  );
  assert.equal(uncertain.run.status, "uncertain");

  const recovered = transition(
    uncertain,
    event({ id: "e2", sequence: 2, type: "run.transitioned", to: "running" }),
  );
  assert.equal(recovered.run.status, "running");
});

test("run with mismatched taskId in initialState is rejected", () => {
  const badRun: Run = { ...run(), taskId: "task-2" };
  assert.throws(() => initialState(task(), badRun), /run.taskId must match task.id/);
});

test("event with wrong runId is rejected", () => {
  const start = initialState(task(), run());
  assert.throws(
    () =>
      transition(start, {
        id: "e1",
        sequence: 1,
        occurredAt: "2026-07-13T00:00:00Z",
        runId: "run-other",
        type: "run.transitioned",
        to: "planning",
      }),
    /does not match/,
  );
});
