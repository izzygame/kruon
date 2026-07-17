import { useLoader } from "@react-three/fiber";
import { useMemo } from "react";
import { AnimationClip, AnimationMixer } from "three";
import type { Object3D } from "three";
import type { GLTF } from "three/examples/jsm/loaders/GLTFLoader.js";
import { GLTFLoader } from "three/examples/jsm/loaders/GLTFLoader.js";
import { clone as cloneSkeleton } from "three/examples/jsm/utils/SkeletonUtils.js";

import type { WorldAgentState, WorldStationProjection } from "../lib/kruon";
import { KENNEY_WORLD_MODEL_URLS } from "./assets";

interface KenneyOfficeProps {
  stations: WorldStationProjection[];
  stateColors: Record<WorldAgentState, string>;
  onSelectRun: (runId: string) => void;
}

const STATION_POSITIONS: Record<string, [number, number, number]> = {
  "codex-desk": [-2.45, 0, 0],
  "claude-desk": [2.45, 0, 0],
};

export function KenneyOffice({ stations, stateColors, onSelectRun }: KenneyOfficeProps) {
  const models = useLoader(GLTFLoader, KENNEY_WORLD_MODEL_URLS) as GLTF[];
  const modelAt = (index: number) => {
    const model = models[index];
    if (!model) throw new Error(`Kenney asset ${index} did not load`);
    return model;
  };
  const desk = modelAt(0);
  const chair = modelAt(1);
  const screen = modelAt(2);
  const keyboard = modelAt(3);
  const mouse = modelAt(4);
  const plant = modelAt(5);
  const codexCharacter = modelAt(6);
  const claudeCharacter = modelAt(7);

  return (
    <group>
      <ModelClone source={plant.scene} position={[-3.45, 0.02, -2.2]} scale={2.1} />
      {stations.map((station) => (
        <KenneyStation
          key={station.stationId}
          station={station}
          position={STATION_POSITIONS[station.stationId] ?? [0, 0, 0]}
          stateColor={stateColors[station.state]}
          character={station.adapter === "codex" ? codexCharacter : claudeCharacter}
          models={{ desk, chair, screen, keyboard, mouse }}
          onSelectRun={onSelectRun}
        />
      ))}
    </group>
  );
}

function KenneyStation({
  station,
  position,
  stateColor,
  character,
  models,
  onSelectRun,
}: {
  station: WorldStationProjection;
  position: [number, number, number];
  stateColor: string;
  character: GLTF;
  models: Record<"desk" | "chair" | "screen" | "keyboard" | "mouse", GLTF>;
  onSelectRun: (runId: string) => void;
}) {
  return (
    <group
      position={position}
      onClick={(event) => {
        event.stopPropagation();
        if (station.runId) onSelectRun(station.runId);
      }}
    >
      <ModelClone source={models.desk.scene} position={[-0.78, 0.03, 0.2]} scale={2.15} />
      <ModelClone source={models.chair.scene} position={[-0.12, 0.02, 1.2]} scale={2.05} />
      <ModelClone source={models.screen.scene} position={[-0.42, 0.86, -0.28]} scale={2.05} />
      <ModelClone source={models.keyboard.scene} position={[-0.23, 0.84, 0.3]} scale={2.05} />
      <ModelClone source={models.mouse.scene} position={[0.55, 0.84, 0.3]} scale={2.05} />
      <CharacterClone model={character} position={[-0.65, 0.02, 1.02]} scale={1.85} />

      <mesh position={[0.98, 1.72, 0.16]} scale={[0.24, 0.24, 0.24]}>
        <octahedronGeometry args={[1, 0]} />
        <meshStandardMaterial color={stateColor} emissive={stateColor} emissiveIntensity={0.55} />
      </mesh>
      <mesh position={[-0.15, 0.035, 0.92]} rotation={[-Math.PI / 2, 0, 0]}>
        <ringGeometry args={[0.62, 0.72, 24]} />
        <meshStandardMaterial color={stateColor} emissive={stateColor} emissiveIntensity={0.32} />
      </mesh>
    </group>
  );
}

function ModelClone({
  source,
  position,
  scale,
}: {
  source: Object3D;
  position: [number, number, number];
  scale: number;
}) {
  const object = useMemo(() => source.clone(true), [source]);

  return <primitive object={object} position={position} scale={scale} />;
}

function CharacterClone({
  model,
  position,
  scale,
}: {
  model: GLTF;
  position: [number, number, number];
  scale: number;
}) {
  const object = useMemo(() => {
    const cloned = cloneSkeleton(model.scene);
    const idle = AnimationClip.findByName(model.animations, "idle");
    if (idle) {
      const mixer = new AnimationMixer(cloned);
      mixer.clipAction(idle).play();
      mixer.setTime(idle.duration * 0.35);
      mixer.update(0);
    }
    return cloned;
  }, [model]);

  return <primitive object={object} position={position} scale={scale} />;
}
