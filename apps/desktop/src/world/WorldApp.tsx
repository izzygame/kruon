import { Component, ComponentType, ReactNode, Suspense, lazy, useEffect, useState } from "react";

import {
  desktopClient,
  displayAdapter,
  KruonClient,
  WorldSnapshot,
  WorldStationProjection,
} from "../lib/kruon";
import { WORLD_SCENE_BUDGET } from "./budget";
import type { WorldSceneProps } from "./WorldScene";
import "./WorldApp.css";

const LazyWorldScene = lazy(() => import("./WorldScene"));

interface WorldAppProps {
  client?: KruonClient;
  SceneComponent?: ComponentType<WorldSceneProps>;
}

export function WorldApp({ client = desktopClient, SceneComponent = LazyWorldScene }: WorldAppProps) {
  const [snapshot, setSnapshot] = useState<WorldSnapshot | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let disposed = false;
    let timer: number | undefined;

    const poll = async () => {
      try {
        const next = await client.invoke<WorldSnapshot>("get_world_snapshot");
        if (!disposed) {
          setSnapshot(next);
          setError(null);
        }
      } catch {
        if (!disposed) setError("The local world projection is unavailable. The 2D control window remains authoritative.");
      } finally {
        if (!disposed) {
          timer = window.setTimeout(
            poll,
            document.hidden ? WORLD_SCENE_BUDGET.hiddenPollMs : WORLD_SCENE_BUDGET.visiblePollMs,
          );
        }
      }
    };

    void poll();
    return () => {
      disposed = true;
      if (timer !== undefined) window.clearTimeout(timer);
    };
  }, [client]);

  async function selectRun(runId: string) {
    try {
      await client.invoke<void>("focus_main_run", { runId });
      setError(null);
    } catch {
      setError("The selected run could not be opened in the 2D control window.");
    }
  }

  const stations = snapshot?.stations ?? [];
  return (
    <main className="world-shell">
      <header className="world-header">
        <div>
          <p className="world-eyebrow">M3 · read-only event projection</p>
          <h1>kruon world</h1>
        </div>
        <div className="world-boundary">
          no process · no files · no credentials
        </div>
      </header>

      {error ? <p className="world-error" role="alert">{error}</p> : null}

      <section className="world-layout">
        <WorldRenderBoundary>
          <Suspense fallback={<WorldLoading text="Loading the isolated renderer…" />}>
            {stations.length > 0 ? (
              <SceneComponent stations={stations} onSelectRun={(runId) => void selectRun(runId)} />
            ) : (
              <WorldLoading text="Waiting for the local event projection…" />
            )}
          </Suspense>
        </WorldRenderBoundary>

        <aside className="world-status" aria-label="Projected run states">
          <h2>Stations</h2>
          <p>Click a desk or card to focus the same Run in the 2D control window.</p>
          {stations.map((station) => (
            <StationCard key={station.stationId} station={station} onSelectRun={selectRun} />
          ))}
          <footer>
            {snapshot ? `Event snapshot ${new Date(snapshot.generatedAt).toLocaleTimeString()}` : "Local snapshot pending"}
          </footer>
        </aside>
      </section>
    </main>
  );
}

function StationCard({
  station,
  onSelectRun,
}: {
  station: WorldStationProjection;
  onSelectRun: (runId: string) => Promise<void>;
}) {
  return (
    <button
      className={`station-card state-${station.state}`}
      type="button"
      disabled={!station.runId}
      onClick={() => station.runId && void onSelectRun(station.runId)}
    >
      <span>{displayAdapter(station.adapter)}</span>
      <strong>{station.state.replaceAll("_", " ")}</strong>
      <small>{station.runId ? `event #${station.sourceSequence}` : "no local run"}</small>
    </button>
  );
}

function WorldLoading({ text }: { text: string }) {
  return <div className="world-loading">{text}</div>;
}

class WorldRenderBoundary extends Component<{ children: ReactNode }, { failed: boolean }> {
  state = { failed: false };

  static getDerivedStateFromError() {
    return { failed: true };
  }

  render() {
    if (this.state.failed) {
      return (
        <div className="world-loading world-failed" role="alert">
          3D rendering stopped safely. Close this window and continue in the unaffected 2D control view.
        </div>
      );
    }
    return this.props.children;
  }
}
