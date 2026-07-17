import { Canvas } from "@react-three/fiber";
import { Component, lazy, ReactNode, Suspense } from "react";

import { WorldAgentState, WorldStationProjection } from "../lib/kruon";
import { WORLD_SCENE_BUDGET } from "./budget";

const LazyKenneyOffice = lazy(() =>
  import("./KenneyOffice").then((module) => ({ default: module.KenneyOffice })),
);

export interface WorldSceneProps {
  stations: WorldStationProjection[];
  onSelectRun: (runId: string) => void;
}

const STATE_COLORS: Record<WorldAgentState, string> = {
  idle: "#7f91a8",
  planning: "#8c7ee8",
  running: "#4e9fe6",
  waiting_approval: "#e8aa4e",
  blocked: "#d95e62",
  reviewing: "#cf80dc",
  completed: "#54b987",
  sleeping: "#465361",
};

const DESK_POSITIONS: Record<string, [number, number, number]> = {
  "codex-desk": [-2.45, 0, 0],
  "claude-desk": [2.45, 0, 0],
};

export default function WorldScene({ stations, onSelectRun }: WorldSceneProps) {
  return (
    <Canvas
      camera={{ position: [8, 7, 9], fov: 40 }}
      dpr={[1, WORLD_SCENE_BUDGET.maxDevicePixelRatio]}
      frameloop="demand"
      gl={{ antialias: true, alpha: false, powerPreference: "low-power" }}
      shadows={false}
    >
      <color attach="background" args={["#0b1017"]} />
      <ambientLight intensity={1.7} />
      <directionalLight position={[4, 8, 5]} intensity={2.1} color="#dceaff" />
      <OfficeShell />
      <AssetRenderBoundary fallback={<ProceduralOffice stations={stations} onSelectRun={onSelectRun} />}>
        <Suspense fallback={<ProceduralOffice stations={stations} onSelectRun={onSelectRun} />}>
          <LazyKenneyOffice
            stations={stations}
            stateColors={STATE_COLORS}
            onSelectRun={onSelectRun}
          />
        </Suspense>
      </AssetRenderBoundary>
    </Canvas>
  );
}

function ProceduralOffice({ stations, onSelectRun }: WorldSceneProps) {
  return (
    <group>
      <ProceduralPlant />
      {stations.map((station) => (
        <DeskStation
          key={station.stationId}
          station={station}
          position={DESK_POSITIONS[station.stationId] ?? [0, 0, 0]}
          onSelectRun={onSelectRun}
        />
      ))}
    </group>
  );
}

function OfficeShell() {
  return (
    <group>
      <mesh position={[0, -0.18, 0]} scale={[8, 0.3, 6]}>
        <boxGeometry />
        <meshStandardMaterial color="#172331" roughness={0.92} />
      </mesh>
      <mesh position={[0, 0.005, 0.35]} scale={[6.3, 0.025, 3.9]}>
        <boxGeometry />
        <meshStandardMaterial color="#1f3040" roughness={1} />
      </mesh>
      <mesh position={[0, 1.45, -3]} scale={[8, 3, 0.16]}>
        <boxGeometry />
        <meshStandardMaterial color="#243442" roughness={0.96} />
      </mesh>
      <mesh position={[-4, 1.45, 0]} scale={[0.16, 3, 6]}>
        <boxGeometry />
        <meshStandardMaterial color="#1b2a38" roughness={0.96} />
      </mesh>
      <mesh position={[0, 0.5, -2.86]} scale={[2.7, 0.7, 0.08]}>
        <boxGeometry />
        <meshStandardMaterial color="#34506a" roughness={0.8} />
      </mesh>
    </group>
  );
}

function ProceduralPlant() {
  return (
    <group>
      <mesh position={[-3.35, 0.35, -2.25]} scale={[0.55, 0.7, 0.55]}>
        <cylinderGeometry args={[0.5, 0.42, 1, 6]} />
        <meshStandardMaterial color="#6f5b45" roughness={1} />
      </mesh>
      <mesh position={[-3.35, 1.05, -2.25]} scale={[0.8, 1.15, 0.8]}>
        <coneGeometry args={[0.7, 1.4, 7]} />
        <meshStandardMaterial color="#3f7b68" roughness={0.9} />
      </mesh>
    </group>
  );
}

class AssetRenderBoundary extends Component<
  { children: ReactNode; fallback: ReactNode },
  { failed: boolean }
> {
  state = { failed: false };

  static getDerivedStateFromError() {
    return { failed: true };
  }

  render() {
    return this.state.failed ? this.props.fallback : this.props.children;
  }
}

function DeskStation({
  station,
  position,
  onSelectRun,
}: {
  station: WorldStationProjection;
  position: [number, number, number];
  onSelectRun: (runId: string) => void;
}) {
  const stateColor = STATE_COLORS[station.state];
  const select = () => {
    if (station.runId) onSelectRun(station.runId);
  };

  return (
    <group
      position={position}
      onClick={(event) => {
        event.stopPropagation();
        select();
      }}
    >
      <mesh position={[0, 0.62, 0]} scale={[2.4, 0.12, 1.25]}>
        <boxGeometry />
        <meshStandardMaterial color="#735a45" roughness={0.78} />
      </mesh>
      <mesh position={[0, 1.22, -0.35]} scale={[1.25, 0.76, 0.1]}>
        <boxGeometry />
        <meshStandardMaterial color="#253847" roughness={0.65} />
      </mesh>
      <mesh position={[0, 1.22, -0.28]} scale={[1.02, 0.55, 0.025]}>
        <boxGeometry />
        <meshStandardMaterial color={stateColor} emissive={stateColor} emissiveIntensity={0.24} />
      </mesh>
      <mesh position={[0, 0.86, -0.35]} scale={[0.12, 0.5, 0.12]}>
        <boxGeometry />
        <meshStandardMaterial color="#334b5d" />
      </mesh>
      <mesh position={[0, 0.38, 1.15]} scale={[1.1, 0.18, 0.9]}>
        <boxGeometry />
        <meshStandardMaterial color="#394a59" roughness={0.95} />
      </mesh>
      <mesh position={[0, 0.86, 1.48]} scale={[1.1, 0.85, 0.15]}>
        <boxGeometry />
        <meshStandardMaterial color="#354654" roughness={0.95} />
      </mesh>

      <group position={[0, 0.9, 0.65]}>
        <mesh position={[0, 0.64, 0]} scale={[0.5, 0.5, 0.5]}>
          <sphereGeometry args={[0.55, 8, 6]} />
          <meshStandardMaterial color="#dfb58f" roughness={0.9} />
        </mesh>
        <mesh position={[0, 0.08, 0]} scale={[0.7, 0.9, 0.48]}>
          <cylinderGeometry args={[0.45, 0.5, 1.2, 8]} />
          <meshStandardMaterial color={stateColor} roughness={0.85} />
        </mesh>
        <mesh position={[-0.25, -0.62, 0]} scale={[0.18, 0.65, 0.18]}>
          <boxGeometry />
          <meshStandardMaterial color="#273443" />
        </mesh>
        <mesh position={[0.25, -0.62, 0]} scale={[0.18, 0.65, 0.18]}>
          <boxGeometry />
          <meshStandardMaterial color="#273443" />
        </mesh>
      </group>

      <mesh position={[0.9, 1.55, 0.15]} scale={[0.22, 0.22, 0.22]}>
        <octahedronGeometry args={[1, 0]} />
        <meshStandardMaterial color={stateColor} emissive={stateColor} emissiveIntensity={0.55} />
      </mesh>
    </group>
  );
}
