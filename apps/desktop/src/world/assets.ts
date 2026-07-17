export const KENNEY_ASSET_LIMIT_BYTES = 5 * 1024 * 1024;
export const KENNEY_VISIBLE_TRIANGLE_LIMIT = 50_000;

export const KENNEY_WORLD_ASSETS = [
  {
    id: "desk",
    kind: "model",
    pack: "furniture-kit",
    url: "/assets/world/kenney/furniture-kit/desk.glb",
    bytes: 15_048,
    triangles: 198,
    sha256: "0164FE828F028B321730FB8C74502E353583F751BE74DA1682D42FFF7D3C5A42",
  },
  {
    id: "chair",
    kind: "model",
    pack: "furniture-kit",
    url: "/assets/world/kenney/furniture-kit/chairDesk.glb",
    bytes: 39_016,
    triangles: 588,
    sha256: "46406619186034CBE92B19B79A5F1A8E3F442A17A70A7524EF2C0AD0F35095C6",
  },
  {
    id: "screen",
    kind: "model",
    pack: "furniture-kit",
    url: "/assets/world/kenney/furniture-kit/computerScreen.glb",
    bytes: 6_404,
    triangles: 72,
    sha256: "5942CBAD29A2D2B956619569926E81D50A4174403D592FECE6556E57632DC643",
  },
  {
    id: "keyboard",
    kind: "model",
    pack: "furniture-kit",
    url: "/assets/world/kenney/furniture-kit/computerKeyboard.glb",
    bytes: 3_476,
    triangles: 32,
    sha256: "9D9135789DE2120E9F41CE3F0A56DD1B15F40ADCC7888739BB842A2D820DA322",
  },
  {
    id: "mouse",
    kind: "model",
    pack: "furniture-kit",
    url: "/assets/world/kenney/furniture-kit/computerMouse.glb",
    bytes: 5_868,
    triangles: 71,
    sha256: "D3AD1A923F0F7707EF367BB969EB94C9076E211C98C830BA52E133FADC5CDFC0",
  },
  {
    id: "plant",
    kind: "model",
    pack: "furniture-kit",
    url: "/assets/world/kenney/furniture-kit/pottedPlant.glb",
    bytes: 7_576,
    triangles: 60,
    sha256: "5B760EDA2766F75FDA36B2C5DF652A1662F82981EF64CD8FA7FE7BCD386B3A15",
  },
  {
    id: "codex-character",
    kind: "model",
    pack: "mini-characters",
    url: "/assets/world/kenney/mini-characters/character-male-a.glb",
    bytes: 246_916,
    triangles: 723,
    sha256: "77572792BFE2773B715B8CD8E18644B52B3E1F155FE10450254B50F9C364382A",
  },
  {
    id: "claude-character",
    kind: "model",
    pack: "mini-characters",
    url: "/assets/world/kenney/mini-characters/character-female-a.glb",
    bytes: 273_448,
    triangles: 876,
    sha256: "8CFCFF43460DA8B421F2A7FDFB43EC177321CAD2746DB9497CBF128D5806E2A8",
  },
  {
    id: "character-colormap",
    kind: "texture",
    pack: "mini-characters",
    url: "/assets/world/kenney/mini-characters/Textures/colormap.png",
    bytes: 8_706,
    triangles: 0,
    sha256: "0D4947D34FF32ACF4A359C7F22CA784E057E7E72F622170A9A77B6FC88FDB70E",
  },
] as const;

export const KENNEY_WORLD_MODEL_URLS = KENNEY_WORLD_ASSETS.filter(
  (asset) => asset.kind === "model",
).map((asset) => asset.url);

export const KENNEY_WORLD_PAYLOAD_BYTES = KENNEY_WORLD_ASSETS.reduce(
  (total, asset) => total + asset.bytes,
  0,
);

// The six workstation furniture models are instanced twice. The plant and two characters are
// rendered once each.
export const KENNEY_VISIBLE_TRIANGLES =
  2 * (198 + 588 + 72 + 32 + 71) + 60 + 723 + 876;
