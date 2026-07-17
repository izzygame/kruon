import { FormEvent, useCallback, useEffect, useMemo, useState } from "react";

import {
  AdapterConnection,
  AdapterKind,
  ApprovalRecord,
  AuditRecord,
  ArtifactRecord,
  CompletionReportCreate,
  desktopClient,
  DiagnosticExportRecord,
  displayAdapter,
  EventEnvelope,
  KruonClient,
  PauseCapability,
  QueueEntry,
  RecoveryAdvice,
  RunSnapshot,
  TaskCreateRequest,
  TaskRecord,
  TaskReviewRecord,
  WorkspaceRecord,
} from "./lib/kruon";
import "./App.css";

const MAX_CONCURRENT_RUNS = 2;
const ONBOARDING_SAMPLE_TASK_TITLE = "Inspect this workspace";
const ONBOARDING_SAMPLE_TASK_CONTEXT =
  "Kruon Alpha onboarding sample. This task is read-only and must not modify files.";

const emptyTask = (): TaskCreateRequest => ({
  workspaceId: "",
  title: "",
  goal: "",
  context: "",
  allowedPaths: ["."],
  acceptanceCriteria: "",
  testPlan: "",
  rollbackPlan: "",
});

interface AppProps {
  client?: KruonClient;
}

export function App({ client = desktopClient }: AppProps) {
  const [connections, setConnections] = useState<AdapterConnection[]>([]);
  const [workspaces, setWorkspaces] = useState<WorkspaceRecord[]>([]);
  const [tasks, setTasks] = useState<TaskRecord[]>([]);
  const [queue, setQueue] = useState<QueueEntry[]>([]);
  const [runs, setRuns] = useState<RunSnapshot[]>([]);
  const [selectedRunId, setSelectedRunId] = useState<string | null>(null);
  const [events, setEvents] = useState<EventEnvelope[]>([]);
  const [approvals, setApprovals] = useState<ApprovalRecord[]>([]);
  const [artifacts, setArtifacts] = useState<ArtifactRecord[]>([]);
  const [audit, setAudit] = useState<AuditRecord[]>([]);
  const [recovery, setRecovery] = useState<RecoveryAdvice[]>([]);
  const [pause, setPause] = useState<PauseCapability | null>(null);
  const [reviews, setReviews] = useState<TaskReviewRecord[]>([]);
  const [completionSummary, setCompletionSummary] = useState("");
  const [completionTests, setCompletionTests] = useState("");
  const [changedPaths, setChangedPaths] = useState("");
  const [reviewNote, setReviewNote] = useState("");
  const [backendReady, setBackendReady] = useState(false);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [workspaceRoot, setWorkspaceRoot] = useState("");
  const [workspaceName, setWorkspaceName] = useState("");
  const [taskForm, setTaskForm] = useState<TaskCreateRequest>(emptyTask);
  const [diagnosticExport, setDiagnosticExport] = useState<DiagnosticExportRecord | null>(null);
  const [exportingDiagnostics, setExportingDiagnostics] = useState(false);
  const [creatingSample, setCreatingSample] = useState(false);

  const refreshBoard = useCallback(async () => {
    const [nextWorkspaces, nextTasks, nextQueue, nextRuns, nextReviews] = await Promise.all([
      client.invoke<WorkspaceRecord[]>("list_workspaces"),
      client.invoke<TaskRecord[]>("list_tasks"),
      client.invoke<QueueEntry[]>("list_queue"),
      client.invoke<RunSnapshot[]>("list_runs"),
      client.invoke<TaskReviewRecord[]>("latest_task_reviews"),
    ]);
    setWorkspaces(nextWorkspaces);
    setTasks(nextTasks);
    setQueue(nextQueue);
    setRuns(nextRuns);
    setReviews(nextReviews);
  }, [client]);

  const refresh = useCallback(async () => {
    setLoading(true);
    try {
      const [nextConnections] = await Promise.all([
        client.invoke<AdapterConnection[]>("probe_connections"),
        refreshBoard(),
      ]);
      setConnections(nextConnections);
      setBackendReady(true);
      setError(null);
    } catch (cause) {
      setBackendReady(false);
      setError(publicMessage(cause));
    } finally {
      setLoading(false);
    }
  }, [client, refreshBoard]);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  useEffect(() => {
    if (!client.listenWorldRunSelection) {
      return undefined;
    }
    let disposed = false;
    let unlisten: (() => void) | undefined;
    void client
      .listenWorldRunSelection((runId) => {
        setSelectedRunId(runId);
        void loadRunDetails(runId);
      })
      .then((nextUnlisten) => {
        if (disposed) {
          nextUnlisten();
        } else {
          unlisten = nextUnlisten;
        }
      })
      .catch((cause) => setError(publicMessage(cause)));
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, [client]);

  useEffect(() => {
    if (!backendReady) {
      return undefined;
    }
    const timer = window.setInterval(() => {
      void refreshBoard().catch((cause) => setError(publicMessage(cause)));
    }, 2_000);
    return () => window.clearInterval(timer);
  }, [backendReady, refreshBoard]);

  useEffect(() => {
    const firstWorkspace = workspaces[0];
    if (taskForm.workspaceId || !firstWorkspace) {
      return;
    }
    setTaskForm((current) => ({ ...current, workspaceId: firstWorkspace.workspaceId }));
  }, [taskForm.workspaceId, workspaces]);

  const workspaceById = useMemo(
    () => new Map(workspaces.map((workspace) => [workspace.workspaceId, workspace])),
    [workspaces],
  );
  const connectionByAdapter = useMemo(
    () => new Map(connections.map((connection) => [connection.adapter, connection])),
    [connections],
  );
  const activeRuns = runs.filter((run) => run.terminalState === null).length;
  const selectedRun = useMemo(
    () => runs.find((run) => run.runId === selectedRunId) ?? null,
    [runs, selectedRunId],
  );
  const selectedTaskId = useMemo(
    () => queue.find((entry) => entry.runId === selectedRunId)?.taskId ?? null,
    [queue, selectedRunId],
  );
  const launchReadyConnections = connections.filter(connectionCanLaunch);
  const trustedWorkspace = workspaces.find((workspace) => workspace.trusted) ?? null;
  const sampleTask = tasks.find(
    (task) => task.title === ONBOARDING_SAMPLE_TASK_TITLE && task.context === ONBOARDING_SAMPLE_TASK_CONTEXT,
  ) ?? null;
  const sampleQueue = sampleTask
    ? queue.find((entry) => entry.taskId === sampleTask.taskId) ?? null
    : null;
  const sampleRun = sampleQueue?.runId
    ? runs.find((run) => run.runId === sampleQueue.runId) ?? null
    : null;
  const sampleReview = sampleTask
    ? reviews.find((review) => review.taskId === sampleTask.taskId) ?? null
    : null;

  async function createWorkspace(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    try {
      const workspace = await client.invoke<WorkspaceRecord>("create_workspace", {
        request: { root: workspaceRoot, displayName: workspaceName },
      });
      setWorkspaceRoot("");
      setWorkspaceName("");
      setTaskForm((current) => ({ ...current, workspaceId: workspace.workspaceId }));
      await refresh();
    } catch (cause) {
      setError(publicMessage(cause));
    }
  }

  async function trustWorkspace(workspaceId: string) {
    try {
      await client.invoke<WorkspaceRecord>("trust_workspace", { workspaceId });
      await refreshBoard();
      setError(null);
    } catch (cause) {
      setError(publicMessage(cause));
    }
  }

  async function untrustWorkspace(workspaceId: string) {
    try {
      await client.invoke<WorkspaceRecord>("untrust_workspace", { workspaceId });
      await refreshBoard();
      setError(null);
    } catch (cause) {
      setError(publicMessage(cause));
    }
  }

  async function createTask(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    try {
      const request = {
        ...taskForm,
        allowedPaths: taskForm.allowedPaths.filter(Boolean),
      };
      await client.invoke<TaskRecord>("create_task", { request });
      setTaskForm((current) => ({ ...emptyTask(), workspaceId: current.workspaceId }));
      await refreshBoard();
      setError(null);
    } catch (cause) {
      setError(publicMessage(cause));
    }
  }

  async function createSampleTask(workspaceId: string) {
    setCreatingSample(true);
    try {
      await client.invoke<TaskRecord>("create_sample_task", { workspaceId });
      await refreshBoard();
      setError(null);
    } catch (cause) {
      setError(publicMessage(cause));
    } finally {
      setCreatingSample(false);
    }
  }

  async function enqueue(taskId: string, adapter: AdapterKind) {
    try {
      await client.invoke<QueueEntry>("enqueue_task_run", {
        request: { taskId, adapter, timeoutMs: 60_000 },
      });
      await refreshBoard();
      setError(null);
    } catch (cause) {
      setError(publicMessage(cause));
    }
  }

  async function loadRunDetails(runId: string) {
    try {
      const [nextEvents, nextApprovals, nextArtifacts, nextAudit, nextRecovery, nextPause] = await Promise.all([
        client.invoke<EventEnvelope[]>("list_events", { runId, afterSequence: 0 }),
        client.invoke<ApprovalRecord[]>("list_approvals", { runId }),
        client.invoke<ArtifactRecord[]>("list_artifacts", { runId }),
        client.invoke<AuditRecord[]>("list_run_audit", { runId }),
        client.invoke<RecoveryAdvice[]>("get_recovery_advice", { runId }),
        client.invoke<PauseCapability>("get_pause_capability"),
      ]);
      setEvents(nextEvents);
      setApprovals(nextApprovals);
      setArtifacts(nextArtifacts);
      setAudit(nextAudit);
      setRecovery(nextRecovery);
      setPause(nextPause);
      setError(null);
    } catch (cause) {
      setError(publicMessage(cause));
    }
  }

  async function selectRun(runId: string) {
    setSelectedRunId(runId);
    await loadRunDetails(runId);
  }

  async function cancelSelectedRun() {
    if (!selectedRunId) return;
    try {
      await client.invoke<RunSnapshot>("cancel_run", { runId: selectedRunId });
      await refreshBoard();
      await loadRunDetails(selectedRunId);
      setError(null);
    } catch (cause) {
      setError(publicMessage(cause));
    }
  }

  async function collectRunArtifacts() {
    if (!selectedRunId) return;
    try {
      const nextArtifacts = await client.invoke<ArtifactRecord[]>("collect_artifacts", { runId: selectedRunId });
      setArtifacts(nextArtifacts);
      setError(null);
    } catch (cause) {
      setError(publicMessage(cause));
    }
  }

  async function recordCompletionReport(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    if (!selectedRunId || !selectedTaskId) return;
    const tests = parseTestResults(completionTests);
    const report: CompletionReportCreate = {
      runId: selectedRunId,
      taskId: selectedTaskId,
      summary: completionSummary,
      tests,
      changedPaths: changedPaths.split("\n").map((path) => path.trim()).filter(Boolean),
    };
    try {
      await client.invoke<ArtifactRecord>("record_completion_report", { report });
      setCompletionSummary("");
      setCompletionTests("");
      setChangedPaths("");
      await loadRunDetails(selectedRunId);
      setError(null);
    } catch (cause) {
      setError(publicMessage(cause));
    }
  }

  async function reviewTask(status: "accepted" | "returned") {
    if (!selectedRunId || !selectedTaskId) return;
    try {
      const command = status === "accepted" ? "accept_task" : "return_task";
      const review = await client.invoke<TaskReviewRecord>(command, {
        taskId: selectedTaskId,
        runId: selectedRunId,
        note: reviewNote,
      });
      setReviews((current) => [review, ...current.filter((item) => item.taskId !== review.taskId)]);
      setReviewNote("");
      setError(null);
    } catch (cause) {
      setError(publicMessage(cause));
    }
  }

  async function restartFollowUp() {
    if (!selectedRunId) return;
    try {
      await client.invoke<QueueEntry>("restart_follow_up", { runId: selectedRunId });
      await refreshBoard();
      await loadRunDetails(selectedRunId);
      setError(null);
    } catch (cause) {
      setError(publicMessage(cause));
    }
  }

  async function openWorldView() {
    try {
      await client.invoke<void>("open_world_view");
      setError(null);
    } catch (cause) {
      setError(publicMessage(cause));
    }
  }

  async function exportDiagnostics() {
    setExportingDiagnostics(true);
    try {
      const exported = await client.invoke<DiagnosticExportRecord>("export_diagnostic_bundle");
      setDiagnosticExport(exported);
      setError(null);
    } catch (cause) {
      setError(publicMessage(cause));
    } finally {
      setExportingDiagnostics(false);
    }
  }

  return (
    <main className="console-shell">
      <header className="console-header">
        <div>
          <p className="eyebrow">local multi-agent control plane</p>
          <h1>kruon</h1>
        </div>
        <div className="header-actions">
          <span className={`runtime-badge ${backendReady ? "ready" : "offline"}`}>
            {backendReady ? "local runtime connected" : "runtime unavailable"}
          </span>
          <button className="button world-button" type="button" disabled={!backendReady} onClick={() => void openWorldView()}>
            Open world view
          </button>
          <button className="button secondary" type="button" onClick={() => void refresh()}>
            Refresh
          </button>
        </div>
      </header>

      <section className="m1-notice" aria-label="M3 projection boundaries">
        <span>M3 optional world</span>
        <p>The 3D window is a read-only projection of the same local run events. Closing it never removes 2D control, approval, cancellation, artifacts, or review; unverified per-action approval is not exposed.</p>
      </section>

      {error ? (
        <>
          <section className="error-banner" role="alert">
            {error}
          </section>
          <ErrorRecoveryPanel
            error={error}
            backendReady={backendReady}
            exportingDiagnostics={exportingDiagnostics}
            onRefresh={() => void refresh()}
            onExport={() => void exportDiagnostics()}
          />
        </>
      ) : null}

      <section
        className={`onboarding-panel ${sampleReview ? "complete" : ""}`}
        aria-labelledby="onboarding-title"
      >
        <div className="onboarding-heading">
          <div>
            <p className="eyebrow">DEV-405</p>
            <h2 id="onboarding-title">First connection</h2>
          </div>
          <span className="onboarding-count">
            {[launchReadyConnections.length > 0, Boolean(trustedWorkspace), Boolean(sampleTask), Boolean(sampleQueue), Boolean(sampleReview)].filter(Boolean).length} / 5
          </span>
        </div>
        <ol className="onboarding-progress">
          <OnboardingStep label="Connect a tool" done={launchReadyConnections.length > 0} active={launchReadyConnections.length === 0} />
          <OnboardingStep label="Trust a workspace" done={Boolean(trustedWorkspace)} active={launchReadyConnections.length > 0 && !trustedWorkspace} />
          <OnboardingStep label="Create read-only sample" done={Boolean(sampleTask)} active={Boolean(trustedWorkspace) && !sampleTask} />
          <OnboardingStep label="Queue and run" done={Boolean(sampleQueue)} active={Boolean(sampleTask) && !sampleQueue} />
          <OnboardingStep label="Human review" done={Boolean(sampleReview)} active={Boolean(sampleRun?.terminalState) && !sampleReview} />
        </ol>
        <div className="onboarding-next">
          {launchReadyConnections.length === 0 ? (
            <>
              <strong>Connect one supported, authenticated CLI.</strong>
              <p>Install Codex or Claude Code for this Windows user, use a listed Alpha version, finish its own terminal login, then refresh discovery.</p>
              <a className="button secondary" href="#connections-title">Check tool setup</a>
            </>
          ) : !trustedWorkspace ? (
            <>
              <strong>Choose the directory Kruon may control.</strong>
              <p>Add a workspace below and explicitly trust it. Trust never expands beyond that canonical root.</p>
              <a className="button secondary" href="#workspace-title">Add or trust workspace</a>
            </>
          ) : !sampleTask ? (
            <>
              <strong>Create the built-in read-only sample.</strong>
              <p>It asks the selected CLI to summarize structure and entry points without modifying files. Repeated creation returns the same local task.</p>
              <button
                className="button"
                type="button"
                disabled={creatingSample}
                onClick={() => void createSampleTask(trustedWorkspace.workspaceId)}
              >
                {creatingSample ? "Creating sample…" : "Create read-only sample"}
              </button>
            </>
          ) : !sampleQueue || (sampleQueue.state === "failed" && !sampleRun) ? (
            <>
              <strong>{sampleQueue?.state === "failed" ? "Retry the sample with a fresh run." : "Launch the sample with a verified tool."}</strong>
              <p>The sample uses the same durable queue, read-only adapter policy, event log, cancellation, and review path as a normal task.</p>
              <button
                className="button"
                type="button"
                onClick={() => void enqueue(sampleTask.taskId, launchReadyConnections[0]!.adapter)}
              >
                {sampleQueue?.state === "failed" ? "Retry sample" : `Run sample with ${displayAdapter(launchReadyConnections[0]!.adapter)}`}
              </button>
            </>
          ) : !sampleRun ? (
            <>
              <strong>Sample queued locally.</strong>
              <p>Kruon will start it when a run slot is available. Keep this window open or refresh to re-read durable state.</p>
              <a className="button secondary" href="#queue-title">View run queue</a>
            </>
          ) : sampleRun.terminalState === null ? (
            <>
              <strong>Sample run is active.</strong>
              <p>Open the 2D run board to inspect events or cancel. The optional world view is never required for control.</p>
              <button className="button secondary" type="button" onClick={() => void selectRun(sampleRun.runId)}>
                Inspect active sample
              </button>
            </>
          ) : !sampleReview ? (
            <>
              <strong>Review the terminal result.</strong>
              <p>Collect artifacts, record a completion report, then accept a successful result or return it with a note. Kruon never auto-accepts.</p>
              <button className="button secondary" type="button" onClick={() => void selectRun(sampleRun.runId)}>
                Review sample run
              </button>
            </>
          ) : (
            <>
              <strong>First connection loop complete.</strong>
              <p>The sample has a durable {sampleReview.status} review. You can now create a scoped task of your own below.</p>
              <a className="button secondary" href="#task-form-title">Create your own task</a>
            </>
          )}
        </div>
        <p className="onboarding-boundary">Credentials and login remain inside the upstream CLI. Kruon stores no API keys, passwords, prompt bodies in diagnostics, or automatic acceptance decisions.</p>
      </section>

      <section className="dashboard-grid">
        <section className="panel connections-panel" aria-labelledby="connections-title">
          <div className="panel-heading">
            <div>
              <p className="eyebrow">DEV-101</p>
              <h2 id="connections-title">Tool connections</h2>
            </div>
            <span className="subtle">version · auth · capabilities</span>
          </div>
          <div className="connection-list">
            {connections.map((connection) => (
              <article className="connection-card" key={connection.adapter}>
                <div className="connection-title">
                  <h3>{displayAdapter(connection.adapter)}</h3>
                  <span className={`status-pill ${connection.status}`}>{connection.status}</span>
                </div>
                <p>{connection.version ?? "Version unavailable"}</p>
                <p className="connection-detail">{connection.detail}</p>
                <dl>
                  <div>
                    <dt>Authentication</dt>
                    <dd>{connection.authentication}</dd>
                  </div>
                  <div>
                    <dt>Approval mode</dt>
                    <dd>{connection.approvalMode}</dd>
                  </div>
                  <div>
                    <dt>Compatibility</dt>
                    <dd>{connection.compatibility}</dd>
                  </div>
                </dl>
                <p className="connection-detail">Alpha versions: {connection.supportedVersions.join(" · ")}</p>
                <p className="capabilities">{connection.capabilities.join(" · ")}</p>
                {connectionRecovery(connection) ? (
                  <p className="connection-recovery">Next: {connectionRecovery(connection)}</p>
                ) : null}
              </article>
            ))}
            {!loading && connections.length === 0 ? <EmptyState text="No local adapters discovered." /> : null}
          </div>
        </section>

        <section className="panel workspace-panel" aria-labelledby="workspace-title">
          <div className="panel-heading">
            <div>
              <p className="eyebrow">DEV-102</p>
              <h2 id="workspace-title">Workspaces</h2>
            </div>
            <span className="subtle">trust is explicit</span>
          </div>
          <form className="compact-form" onSubmit={createWorkspace}>
            <label htmlFor="workspace-name">Display name</label>
            <input
              id="workspace-name"
              required
              value={workspaceName}
              onChange={(event) => setWorkspaceName(event.target.value)}
              placeholder="kruon"
            />
            <label htmlFor="workspace-root">Workspace root</label>
            <input
              id="workspace-root"
              required
              value={workspaceRoot}
              onChange={(event) => setWorkspaceRoot(event.target.value)}
              placeholder="D:\\projects\\kruon"
            />
            <button className="button" type="submit" disabled={!backendReady}>
              Add workspace
            </button>
          </form>
          <div className="workspace-list">
            {workspaces.map((workspace) => (
              <article className="workspace-card" key={workspace.workspaceId}>
                <div>
                  <h3>{workspace.displayName}</h3>
                  <p className="path">{workspace.root}</p>
                </div>
                {workspace.trusted ? (
                  <div className="workspace-trust-actions">
                    <span className="status-pill ready">trusted</span>
                    <button
                      className="button warning"
                      type="button"
                      title="Blocks new and queued launches; an already active Run must still be cancelled separately."
                      onClick={() => void untrustWorkspace(workspace.workspaceId)}
                      disabled={!backendReady}
                    >
                      Revoke trust
                    </button>
                  </div>
                ) : (
                  <button
                    className="button warning"
                    type="button"
                    onClick={() => void trustWorkspace(workspace.workspaceId)}
                    disabled={!backendReady}
                  >
                    Trust workspace
                  </button>
                )}
              </article>
            ))}
            {!loading && workspaces.length === 0 ? <EmptyState text="Add a local directory to begin." /> : null}
          </div>
        </section>

        <section className="panel task-form-panel" aria-labelledby="task-form-title">
          <div className="panel-heading">
            <div>
              <p className="eyebrow">DEV-103</p>
              <h2 id="task-form-title">Create task</h2>
            </div>
            <span className="subtle">definition before execution</span>
          </div>
          <form className="task-form" onSubmit={createTask}>
            <label htmlFor="task-workspace">Workspace</label>
            <select
              id="task-workspace"
              value={taskForm.workspaceId}
              onChange={(event) => setTaskForm((current) => ({ ...current, workspaceId: event.target.value }))}
              required
            >
              <option value="">Choose a workspace</option>
              {workspaces.map((workspace) => (
                <option key={workspace.workspaceId} value={workspace.workspaceId}>
                  {workspace.displayName} ({workspace.trusted ? "trusted" : "untrusted"})
                </option>
              ))}
            </select>
            <label htmlFor="task-title">Title</label>
            <input
              id="task-title"
              required
              value={taskForm.title}
              onChange={(event) => setTaskForm((current) => ({ ...current, title: event.target.value }))}
            />
            <label htmlFor="task-goal">Goal</label>
            <textarea
              id="task-goal"
              required
              value={taskForm.goal}
              onChange={(event) => setTaskForm((current) => ({ ...current, goal: event.target.value }))}
            />
            <label htmlFor="task-context">Context</label>
            <textarea
              id="task-context"
              value={taskForm.context}
              onChange={(event) => setTaskForm((current) => ({ ...current, context: event.target.value }))}
            />
            <label htmlFor="task-scopes">Allowed paths (one per line)</label>
            <textarea
              id="task-scopes"
              value={taskForm.allowedPaths.join("\n")}
              onChange={(event) =>
                setTaskForm((current) => ({ ...current, allowedPaths: event.target.value.split("\n") }))
              }
            />
            <label htmlFor="task-acceptance">Acceptance criteria</label>
            <textarea
              id="task-acceptance"
              value={taskForm.acceptanceCriteria}
              onChange={(event) => setTaskForm((current) => ({ ...current, acceptanceCriteria: event.target.value }))}
            />
            <label htmlFor="task-tests">Test plan</label>
            <textarea
              id="task-tests"
              value={taskForm.testPlan}
              onChange={(event) => setTaskForm((current) => ({ ...current, testPlan: event.target.value }))}
            />
            <label htmlFor="task-rollback">Rollback plan</label>
            <textarea
              id="task-rollback"
              value={taskForm.rollbackPlan}
              onChange={(event) => setTaskForm((current) => ({ ...current, rollbackPlan: event.target.value }))}
            />
            <button className="button" type="submit" disabled={!backendReady || workspaces.length === 0}>
              Save task
            </button>
          </form>
        </section>

        <section className="panel queue-panel" aria-labelledby="queue-title">
          <div className="panel-heading">
            <div>
              <p className="eyebrow">DEV-104 · DEV-106</p>
              <h2 id="queue-title">Run queue</h2>
            </div>
            <span className="capacity">{activeRuns} / {MAX_CONCURRENT_RUNS} active slots</span>
          </div>
          <p className="queue-caption">Executor selection is manual. Additional tasks stay durable in the queue.</p>
          <div className="task-list">
            {tasks.map((task) => {
              const workspace = workspaceById.get(task.workspaceId);
              const taskQueue = queue.filter((entry) => entry.taskId === task.taskId);
              const codexConnection = connectionByAdapter.get("codex");
              const claudeConnection = connectionByAdapter.get("claude");
              const canRunCodex = Boolean(backendReady && workspace?.trusted && codexConnection && connectionCanLaunch(codexConnection));
              const canRunClaude = Boolean(backendReady && workspace?.trusted && claudeConnection && connectionCanLaunch(claudeConnection));
              return (
                <article className="task-card" key={task.taskId}>
                  <div>
                    <h3>{task.title}</h3>
                    <p>{task.goal}</p>
                    <p className="task-scope">Scope: {task.allowedPaths.join(", ")}</p>
                  </div>
                  <div className="task-actions">
                    <button className="button" type="button" disabled={!canRunCodex} title={codexConnection?.detail} onClick={() => void enqueue(task.taskId, "codex")}>
                      Run with Codex
                    </button>
                    <button className="button secondary" type="button" disabled={!canRunClaude} title={claudeConnection?.detail} onClick={() => void enqueue(task.taskId, "claude")}>
                      Run with Claude
                    </button>
                  </div>
                  {reviews.find((review) => review.taskId === task.taskId) ? (
                    <p className="review-entry">
                      Latest review: {reviews.find((review) => review.taskId === task.taskId)?.status}
                    </p>
                  ) : null}
                  {!workspace?.trusted ? <p className="trust-note">Trust this workspace to enable CLI launch.</p> : null}
                  {workspace?.trusted && (!codexConnection || !connectionCanLaunch(codexConnection)) ? <p className="trust-note">Codex launch unavailable: {codexConnection ? connectionRecovery(codexConnection) ?? codexConnection.detail : "connection check pending"}.</p> : null}
                  {workspace?.trusted && (!claudeConnection || !connectionCanLaunch(claudeConnection)) ? <p className="trust-note">Claude Code launch unavailable: {claudeConnection ? connectionRecovery(claudeConnection) ?? claudeConnection.detail : "connection check pending"}.</p> : null}
                  {taskQueue.map((entry) => (
                    <p className={`queue-entry ${entry.state}`} key={entry.queueId}>
                      {displayAdapter(entry.adapter)} · {entry.state}{entry.failureCode ? ` · ${entry.failureCode}` : ""}
                    </p>
                  ))}
                </article>
              );
            })}
            {!loading && tasks.length === 0 ? <EmptyState text="Save a task to make it runnable." /> : null}
          </div>
        </section>

        <section className="panel runs-panel" aria-labelledby="runs-title">
          <div className="panel-heading">
            <div>
              <p className="eyebrow">DEV-105 · DEV-107 · DEV-403</p>
              <h2 id="runs-title">2D run board</h2>
            </div>
            <div className="diagnostic-export-control">
              <span className="subtle">metadata only · no prompts, projects, paths, credentials, or raw logs</span>
              <button
                className="button secondary"
                type="button"
                disabled={!backendReady || exportingDiagnostics}
                onClick={() => void exportDiagnostics()}
              >
                {exportingDiagnostics ? "Exporting…" : "Export diagnostics"}
              </button>
            </div>
          </div>
          {diagnosticExport ? (
            <p className="diagnostic-export-result" role="status">
              Saved {diagnosticExport.fileName} in {diagnosticExport.savedIn === "downloads" ? "Downloads" : "Kruon app data"}. {diagnosticExport.includedRuns} of {diagnosticExport.totalRuns} run summaries included; SHA-256 {shortFingerprint(diagnosticExport.sha256)}.
            </p>
          ) : null}
          <div className="run-columns">
            <div className="run-list">
              {runs.map((run) => (
                <button
                  type="button"
                  className={`run-card ${selectedRunId === run.runId ? "selected" : ""}`}
                  key={run.runId}
                  onClick={() => void selectRun(run.runId)}
                >
                  <span>{displayAdapter(run.adapter)}</span>
                  <strong>{run.status}</strong>
                  <small>{new Date(run.updatedAt).toLocaleString()}</small>
                </button>
              ))}
              {!loading && runs.length === 0 ? <EmptyState text="No runs have been launched." /> : null}
            </div>
            <aside className="event-panel" aria-label="Selected run diagnostics">
              <div className="run-detail-heading">
                <div>
                  <h3>Diagnostics &amp; handoff</h3>
                  {selectedRunId ? <p className="subtle">{selectedRunId}</p> : <p className="subtle">Select a run to inspect its event sequence.</p>}
                </div>
                {selectedRun ? <span className="status-pill">{selectedRun.terminalState ?? "active"}</span> : null}
              </div>
              {selectedRun ? (
                <div className="run-control-grid">
                  <p className="fingerprint">Frozen launch: {shortFingerprint(selectedRun.launchFingerprint)}</p>
                  <div className="run-actions">
                    <button className="button warning" type="button" onClick={() => void cancelSelectedRun()} disabled={selectedRun.terminalState !== null}>
                      Cancel run
                    </button>
                    <button className="button secondary" type="button" onClick={() => void collectRunArtifacts()}>
                      Collect artifacts
                    </button>
                    <button
                      className="button secondary"
                      type="button"
                      onClick={() => void restartFollowUp()}
                      disabled={!recovery.some((advice) => advice.canRestartFollowUp)}
                    >
                      Fresh follow-up
                    </button>
                  </div>
                  <p className="policy-boundary">
                    Per-action approvals: {approvals.length ? `${approvals.length} audited record(s)` : "not enabled for the frozen adapters"}. {pause?.message ?? "Pause capability loads with the run."}
                  </p>
                  {recovery.map((advice) => <p className="recovery-note" key={advice.code}>{advice.message}</p>)}
                  <ArtifactList artifacts={artifacts} />
                  <AuditList audit={audit} />
                  {selectedTaskId && selectedRun.terminalState ? (
                    <form className="completion-form" onSubmit={recordCompletionReport}>
                      <label htmlFor="completion-summary">Completion summary</label>
                      <textarea
                        id="completion-summary"
                        required
                        value={completionSummary}
                        onChange={(event) => setCompletionSummary(event.target.value)}
                        placeholder="Human-readable handoff summary"
                      />
                      <label htmlFor="completion-tests">Checks (name | status | detail, one per line)</label>
                      <textarea
                        id="completion-tests"
                        value={completionTests}
                        onChange={(event) => setCompletionTests(event.target.value)}
                        placeholder="pnpm test | passed | 4 tests"
                      />
                      <label htmlFor="completion-paths">Changed paths (one relative path per line)</label>
                      <textarea
                        id="completion-paths"
                        value={changedPaths}
                        onChange={(event) => setChangedPaths(event.target.value)}
                        placeholder="apps/desktop/src/App.tsx"
                      />
                      <button className="button secondary" type="submit">Record completion report</button>
                    </form>
                  ) : null}
                  {selectedTaskId && selectedRun.terminalState ? (
                    <div className="review-controls">
                      <label htmlFor="review-note">Review note {selectedRun.terminalState === "completed" ? "(optional for acceptance)" : "(required for return)"}</label>
                      <textarea
                        id="review-note"
                        value={reviewNote}
                        onChange={(event) => setReviewNote(event.target.value)}
                        placeholder="Why this work is accepted or needs another pass"
                      />
                      <div className="run-actions">
                        <button
                          className="button"
                          type="button"
                          onClick={() => void reviewTask("accepted")}
                          disabled={selectedRun.terminalState !== "completed" || !artifacts.some((artifact) => artifact.kind === "completion_report")}
                        >
                          Accept task
                        </button>
                        <button className="button warning" type="button" onClick={() => void reviewTask("returned")} disabled={!reviewNote.trim()}>
                          Return task
                        </button>
                      </div>
                    </div>
                  ) : null}
                </div>
              ) : null}
              {events.map((entry) => (
                <div className="event-row" key={entry.eventId}>
                  <span>#{entry.sequence}</span>
                  <strong>{entry.eventType}</strong>
                  <small>{entry.phase} · {new Date(entry.occurredAt).toLocaleTimeString()}</small>
                </div>
              ))}
            </aside>
          </div>
        </section>
      </section>
    </main>
  );
}

function OnboardingStep({ label, done, active }: { label: string; done: boolean; active: boolean }) {
  return (
    <li className={done ? "done" : active ? "active" : "pending"}>
      <span aria-hidden="true">{done ? "✓" : ""}</span>
      {label}
    </li>
  );
}

function ErrorRecoveryPanel({
  error,
  backendReady,
  exportingDiagnostics,
  onRefresh,
  onExport,
}: {
  error: string;
  backendReady: boolean;
  exportingDiagnostics: boolean;
  onRefresh: () => void;
  onExport: () => void;
}) {
  const recovery = errorRecovery(error);
  return (
    <section className="error-recovery-panel" aria-labelledby="error-recovery-title">
      <div>
        <p className="eyebrow">Recovery guide · {recovery.code}</p>
        <h2 id="error-recovery-title">{recovery.title}</h2>
        <p>{recovery.message}</p>
      </div>
      <div className="run-actions">
        <button className="button secondary" type="button" onClick={onRefresh}>Refresh local state</button>
        <button className="button secondary" type="button" disabled={!backendReady || exportingDiagnostics} onClick={onExport}>
          {exportingDiagnostics ? "Exporting…" : "Export metadata-only diagnostics"}
        </button>
      </div>
      <p className="error-boundary">Never paste credentials into Kruon. Authentication repair happens in the upstream CLI terminal; diagnostics exclude prompt bodies, workspace paths, raw logs, and credentials.</p>
    </section>
  );
}

function EmptyState({ text }: { text: string }) {
  return <p className="empty-state">{text}</p>;
}

function ArtifactList({ artifacts }: { artifacts: ArtifactRecord[] }) {
  if (artifacts.length === 0) {
    return <p className="empty-state">No artifacts recorded for this run yet.</p>;
  }
  return (
    <div className="artifact-list" aria-label="Run artifacts">
      <h4>Recorded artifacts</h4>
      {artifacts.map((artifact) => (
        <div className="artifact-row" key={artifact.artifactId}>
          <strong>{artifact.kind.replaceAll("_", " ")}</strong>
          <span>{artifact.path ?? artifact.summary}</span>
          <small>{artifact.sourceEventSequence ? `event #${artifact.sourceEventSequence}` : "manual record"}</small>
        </div>
      ))}
    </div>
  );
}

function AuditList({ audit }: { audit: AuditRecord[] }) {
  if (audit.length === 0) {
    return <p className="empty-state">No M2 audit entries recorded for this run yet.</p>;
  }
  return (
    <div className="audit-list" aria-label="Run audit trail">
      <h4>Audit trail</h4>
      {audit.map((entry) => (
        <div className="artifact-row" key={entry.auditId}>
          <strong>{entry.eventType.replace("run.", "")}</strong>
          <span>{new Date(entry.createdAt).toLocaleString()}</span>
          <small>local record</small>
        </div>
      ))}
    </div>
  );
}

function parseTestResults(value: string): CompletionReportCreate["tests"] {
  return value
    .split("\n")
    .map((line) => line.trim())
    .filter(Boolean)
    .map((line) => {
      const [name, status, ...detail] = line.split("|").map((part) => part.trim());
      return {
        name: name || "manual check",
        status: status || "recorded",
        detail: detail.join(" | ") || "not provided",
      };
    });
}

function shortFingerprint(value: string): string {
  return value ? `${value.slice(0, 12)}…` : "legacy run";
}

function publicMessage(cause: unknown): string {
  if (typeof cause === "string") return cause;
  if (cause instanceof Error && cause.message) return cause.message;
  return "runtime_unavailable: the local runtime is unavailable";
}

function connectionCanLaunch(connection: AdapterConnection): boolean {
  return connection.status === "ready" && connection.authentication === "authenticated";
}

function connectionRecovery(connection: AdapterConnection): string | null {
  const adapter = displayAdapter(connection.adapter);
  const versionCommand = connection.adapter === "codex" ? "codex exec --version" : "claude --version";
  const authCommand = connection.adapter === "codex" ? "codex login status" : "claude auth status";
  switch (connection.status) {
    case "not_found":
      return `Install ${adapter} for the current Windows user, reopen Kruon, then Refresh. Discovery checks PATH and common per-user install directories`;
    case "version_check_failed":
      return `Run “${versionCommand}” in a terminal, repair that CLI if it fails, then Refresh`;
    case "unsupported_version":
      return `Install one of the listed Alpha versions (${connection.supportedVersions.join(", ")}); launch stays blocked until then`;
    case "ready":
      if (connection.authentication === "unauthenticated") {
        return `Complete the upstream CLI login, verify with “${authCommand}”, then Refresh; Kruon does not collect credentials`;
      }
      if (connection.authentication === "unknown") {
        return `Run “${authCommand}” in a terminal and finish upstream authentication before launch`;
      }
      return null;
  }
}

function errorRecovery(error: string): { code: string; title: string; message: string } {
  const code = error.includes(":") ? error.slice(0, error.indexOf(":")) : "runtime_unavailable";
  switch (code) {
    case "unsupported_adapter_version":
      return { code, title: "Restore a supported CLI version", message: "Check the Tool connections cards, install a listed Alpha version, then Refresh. Kruon keeps execution blocked when compatibility is unverified." };
    case "path_policy_violation":
      return { code, title: "Repair workspace trust or scope", message: "Trust the intended workspace and keep every allowed path relative to its canonical root. Do not widen the task to another directory." };
    case "process_error":
    case "adapter_error":
      return { code, title: "Verify the upstream CLI", message: "Run the adapter’s version and authentication checks in a terminal, Refresh, then retry as a fresh run. Kruon will not mark the failed attempt successful." };
    case "store_error":
    case "internal_error":
      return { code, title: "Restore local storage first", message: "Check free disk space and app-data permissions, then restart Kruon. Preserve the database; do not delete it unless you intentionally accept local history loss." };
    case "not_found":
      return { code, title: "Refresh the local record", message: "The selected workspace, task, queue entry, or run no longer exists. Refresh and choose an item that is still present in the local store." };
    case "conflict":
      return { code, title: "Re-read the current run state", message: "Another transition already won. Refresh, then wait for or cancel the active run before issuing a new action." };
    case "invalid_argument":
      return { code, title: "Review the submitted fields", message: "Check required task text and relative allowed paths, correct the form, and retry." };
    case "diagnostic_export_failed":
      return { code, title: "Restore a safe export destination", message: "Check free space and write permission for Downloads and Kruon app data, then retry the metadata-only export." };
    default:
      return { code, title: "Refresh before retrying", message: "Re-read durable local state. If the error repeats, export metadata-only diagnostics and retain the exact public error code for support." };
  }
}
