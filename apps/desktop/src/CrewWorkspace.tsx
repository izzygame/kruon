import { FormEvent, useMemo, useState } from "react";

import {
  AdapterConnection,
  AdapterKind,
  displayAdapter,
  QueueEntry,
  RunSnapshot,
  TaskRecord,
} from "./lib/kruon";

type CrewState = "offline" | "idle" | "planning" | "running" | "waiting" | "blocked" | "reviewing" | "completed";

interface CrewMember {
  adapter: AdapterKind;
  label: string;
  avatar: string;
  role: string;
  state: CrewState;
  run: RunSnapshot | null;
  task: TaskRecord | null;
  connection: AdapterConnection | null;
}

interface CrewWorkspaceProps {
  connections: AdapterConnection[];
  runs: RunSnapshot[];
  queue: QueueEntry[];
  tasks: TaskRecord[];
  selectedRunId: string | null;
  onSelectRun: (runId: string) => void;
  onComposeTask: (draft: string) => void;
}

const CREW: Array<Pick<CrewMember, "adapter" | "label" | "avatar" | "role">> = [
  { adapter: "codex", label: "Codex", avatar: "CX", role: "builder" },
  { adapter: "claude", label: "Claude", avatar: "CL", role: "researcher" },
];

export function CrewWorkspace({
  connections,
  runs,
  queue,
  tasks,
  selectedRunId,
  onSelectRun,
  onComposeTask,
}: CrewWorkspaceProps) {
  const [draft, setDraft] = useState("");
  const members = useMemo(() => makeCrew(connections, runs, queue, tasks), [connections, runs, queue, tasks]);
  const selectedRun = runs.find((run) => run.runId === selectedRunId)
    ?? members.find((member) => member.run?.terminalState === null)?.run
    ?? members.find((member) => member.run)?.run
    ?? null;
  const selectedMember = selectedRun
    ? members.find((member) => member.adapter === selectedRun.adapter) ?? null
    : null;
  const selectedTask = selectedMember?.task ?? null;
  const activeCount = members.filter((member) => member.run?.terminalState === null).length;

  function submitDraft(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const nextDraft = draft.trim();
    if (!nextDraft) return;
    onComposeTask(nextDraft);
    setDraft("");
  }

  return (
    <section className="crew-workspace" aria-labelledby="crew-workspace-title">
      <aside className="crew-rail" aria-label="Crew navigation">
        <div className="crew-brand">
          <span className="crew-brand-mark" aria-hidden="true">K</span>
          <div>
            <p>LOCAL CREW</p>
            <h2 id="crew-workspace-title">kruon</h2>
          </div>
        </div>

        <nav className="crew-nav" aria-label="Workspace views">
          <a className="active" href="#crew-workspace-title">Workspace</a>
          <a href="#queue-title">Runs <span>{activeCount}</span></a>
          <a href="#task-form-title">Tasks <span>{tasks.length}</span></a>
          <a href="#connections-title">Connections</a>
        </nav>

        <section className="crew-roster" aria-labelledby="crew-roster-title">
          <p className="crew-section-label" id="crew-roster-title">YOUR CREW</p>
          {members.map((member) => (
            <button
              aria-label={`Inspect ${member.label} crew status${member.run ? `, ${humanizeRunState(member.run)}` : ""}`}
              className={`crew-member crew-state-${member.state} ${selectedMember?.adapter === member.adapter ? "selected" : ""}`}
              key={member.adapter}
              type="button"
              onClick={() => member.run && onSelectRun(member.run.runId)}
              disabled={!member.run}
            >
              <span className={`crew-avatar crew-avatar-${member.adapter}`} aria-hidden="true">{member.avatar}</span>
              <span>
                <strong>{member.label}</strong>
                <small>{member.state === "offline" ? "connect tool" : member.role}</small>
              </span>
              <i aria-label={member.state} />
            </button>
          ))}
        </section>

        <div className="crew-rail-footnote">
          <span className="crew-live-dot" aria-hidden="true" />
          Local event stream
        </div>
      </aside>

      <section className="crew-main-stage" aria-label="Spatial task overview">
        <header className="crew-stage-header">
          <div>
            <p className="crew-kicker">TODAY'S WORKSPACE</p>
            <h2>{selectedTask?.title ?? (selectedRun ? `${displayAdapter(selectedRun.adapter)} is ${humanizeRunState(selectedRun)}` : "Your crew is standing by")}</h2>
          </div>
          <div className="crew-stage-actions">
            <span>{activeCount} active / 2 capacity</span>
            <a href="#task-form-title">Task details</a>
          </div>
        </header>

        <div className="crew-office" aria-label="Interactive crew office">
          <div className="crew-aurora" aria-hidden="true" />
          <div className="crew-grid" aria-hidden="true" />
          <div className="crew-room crew-room-review" aria-hidden="true">
            <span>REVIEW</span>
            <i />
            <i />
          </div>
          <div className="crew-room crew-room-brief" aria-hidden="true">
            <span>BRIEFING</span>
            <b />
          </div>
          <div className="crew-window-glow" aria-hidden="true" />
          <div className="crew-plant crew-plant-left" aria-hidden="true" />
          <div className="crew-plant crew-plant-right" aria-hidden="true" />

          {members.map((member) => (
            <button
              aria-label={`Inspect ${member.label} station${member.run ? `, ${humanizeRunState(member.run)}` : ""}`}
              className={`crew-station crew-station-${member.adapter} crew-state-${member.state} ${selectedMember?.adapter === member.adapter ? "selected" : ""}`}
              key={member.adapter}
              type="button"
              onClick={() => member.run && onSelectRun(member.run.runId)}
              disabled={!member.run}
            >
              <span className="crew-station-halo" aria-hidden="true" />
              <span className="crew-desk" aria-hidden="true"><i /><b /></span>
              <span className={`crew-character crew-character-${member.adapter}`} aria-hidden="true"><i /><b /></span>
              <span className="crew-station-copy">
                <strong>{member.label}</strong>
                <small>{member.run ? humanizeRunState(member.run) : member.state === "offline" ? "tool offline" : "ready"}</small>
              </span>
              {member.run?.terminalState === null ? (
                <span className="crew-task-bubble">
                  <b>{member.run.status}</b>
                  <small>{member.task?.title ?? "Awaiting task brief"}</small>
                </span>
              ) : null}
            </button>
          ))}
        </div>

        <form className="crew-composer" onSubmit={submitDraft}>
          <span className="crew-composer-icon" aria-hidden="true">+</span>
          <input
            aria-label="Draft a task from the workspace"
            value={draft}
            onChange={(event) => setDraft(event.target.value)}
            placeholder="Tell your crew what to work on…"
          />
          <button type="submit" disabled={!draft.trim()}>Draft task</button>
        </form>
      </section>

      <aside className="crew-inspector" aria-label="Selected crew activity">
        <div className="crew-inspector-heading">
          <p className="crew-section-label">LIVE ACTIVITY</p>
          <span>{runs.length} runs</span>
        </div>
        {selectedRun ? (
          <>
            <div className={`crew-inspector-status crew-state-${stateForRun(selectedRun)}`}>
              <span>{selectedMember?.avatar ?? "K"}</span>
              <div>
                <strong>{displayAdapter(selectedRun.adapter)}</strong>
                <small>{humanizeRunState(selectedRun)}</small>
              </div>
            </div>
            {selectedTask ? (
              <section className="crew-task-focus" aria-label="Selected task brief">
                <p>ACTIVE BRIEF</p>
                <strong>{selectedTask.title}</strong>
                <span>{selectedTask.goal}</span>
              </section>
            ) : null}
            <dl className="crew-run-facts">
              <div><dt>Event stream</dt><dd>#{selectedRun.lastSequence}</dd></div>
              <div><dt>Mode</dt><dd>{selectedRun.policyId ? "guarded" : "local"}</dd></div>
              <div><dt>CLI</dt><dd>{selectedMember?.connection?.normalizedVersion ?? "unverified"}</dd></div>
              <div><dt>Updated</dt><dd>{new Date(selectedRun.updatedAt).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" })}</dd></div>
            </dl>
            <button className="crew-inspect-button" type="button" onClick={() => onSelectRun(selectedRun.runId)}>
              Open run detail
            </button>
          </>
        ) : (
          <div className="crew-empty-inspector">
            <span aria-hidden="true">✦</span>
            <strong>No active run yet</strong>
            <p>Draft a scoped task below, then choose an available crew member in the run queue.</p>
          </div>
        )}
        <div className="crew-inspector-footer">
          <span>2D control remains authoritative</span>
          <a href="#runs-title">Open run board</a>
        </div>
      </aside>
    </section>
  );
}

function makeCrew(
  connections: AdapterConnection[],
  runs: RunSnapshot[],
  queue: QueueEntry[],
  tasks: TaskRecord[],
): CrewMember[] {
  return CREW.map((definition) => {
    const connection = connections.find((candidate) => candidate.adapter === definition.adapter) ?? null;
    const run = latestRunForAdapter(runs, definition.adapter);
    const state = connection?.status !== "ready" || connection.authentication !== "authenticated"
      ? "offline"
      : run ? stateForRun(run) : "idle";
    return { ...definition, connection, run, task: taskForRun(run, queue, tasks), state };
  });
}

function taskForRun(run: RunSnapshot | null, queue: QueueEntry[], tasks: TaskRecord[]): TaskRecord | null {
  if (!run) return null;
  const queueEntry = queue.find((entry) => entry.runId === run.runId);
  return queueEntry ? tasks.find((task) => task.taskId === queueEntry.taskId) ?? null : null;
}

function latestRunForAdapter(runs: RunSnapshot[], adapter: AdapterKind): RunSnapshot | null {
  const adapterRuns = runs.filter((run) => run.adapter === adapter);
  return adapterRuns.reduce<RunSnapshot | null>((latest, candidate) => {
    if (!latest || Date.parse(candidate.updatedAt) > Date.parse(latest.updatedAt)) return candidate;
    return latest;
  }, null);
}

function stateForRun(run: RunSnapshot): CrewState {
  const value = (run.terminalState ?? run.status).replaceAll("_", " ").toLowerCase();
  if (value.includes("complete")) return "completed";
  if (value.includes("review")) return "reviewing";
  if (value.includes("approv") || value.includes("wait")) return "waiting";
  if (value.includes("block") || value.includes("fail") || value.includes("cancel")) return "blocked";
  if (value.includes("plan")) return "planning";
  return "running";
}

function humanizeRunState(run: RunSnapshot): string {
  const value = run.terminalState ?? run.status;
  return value.replaceAll("_", " ");
}
