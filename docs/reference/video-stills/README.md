# Reference video stills

This folder is the lightweight visual source of truth for the crew-workspace
frontend. The original reference video is deliberately **not** stored in this
repository. Once the source is supplied again, extract the stills below into
this folder and record its timestamp plus a short observation in the table.

## Source

| Field | Value |
|---|---|
| Filename | `参考.mp4` |
| Duration | 160.6 s |
| SHA-256 | `e36ba14e93311c39f531956d49df6103e9a13e3b860fe23c28298554f94732a3` |
| Resolution | 1680 × 1080 |

## Stills

| File | Capture focus | What the frontend must preserve | Source timestamp | Observation |
|---|---|---|---|---|
| `01-overview.png` | Whole workspace composition | Spatial world stays central; controls remain visible around it | **00:01.25** | Full 3-panel layout: left crew rail (14 members), center isometric office world (desks + character avatars), right inspector panel (agent card), bottom chat composer, top status bar with workspace metrics |
| `02-crew-rail.png` | Left member rail | Crew identity, availability and grouping are readable at a glance | **00:22.07** | Member list prominent with role badges (팀장/전문가/팀원), team/role filter dropdowns, 14 members across teams (Hermes etc.), Atlas (monkey avatar) selected in center |
| `03-inspector.png` | Right detail panel | Selecting a member reveals task, model/status and next action | **01:59.65** | Milo agent card (품질관리자/QA manager, gpt-5.3-codex-spark) plus task-detail popup showing deployment-result checklist; 3D world shows desk zone context |
| `04-composer.png` | Bottom task composer | A single clear entry point starts a task without hiding scoped controls | **00:11.55** | Chat/composer bar as primary interaction surface; Juno agent selected; 3D camera shifted to meeting-area angle; top-bar shows CEO context |
| `05-active-feedback.png` | Running state | Character, desk and nearby feedback show work without pretending the task is complete | **00:56.35** | Active delegation modal ("quality-e3 - 밸 지켜기") centered over dimmed 3D scene; background shows Milo/Vega agents at desks; right panel displays DELEGATING task status |
| `06-review-space.png` | Review/meeting area | The world has clear spatial semantics beyond individual desks | **01:01.17** | Quest-track selection modal for "마이라게임 4월 캠페인": left = sub-task checklist, center = team roster with status counts, right = live activity log of agent actions |
| `07-camera-transition.png` | Camera/zoom state | Focus changes preserve orientation and never block the 2D control path | **00:30.22** | Design/theme panel ("디자인") overlaid on 3D scene; options for transparency/dark/mixed modes, color pickers, font controls; world remains visible behind config UI |
| `08-atmosphere.png` | Lighting and visual hierarchy | Dark, calm, readable depth supports rather than competes with task state | **01:40.35** | Full dark-mode overlay with collaboration-request modal ("협업 요청"); workspace dimmed to ~20 % brightness; demonstrates atmospheric depth option |

## Capture rules

- Use scene changes rather than evenly spaced timestamps; eight stills are the
  target, with a ninth only when a distinct interaction state appears.
- Keep PNG dimensions from the source. Do not upscale, add annotations, or
  commit the full video.
- Record source filename, duration, SHA-256 and the exact timestamp in this
  README when the stills are added.
- The stills are experience references, not product requirements. Kruon's
  `Run/Event` state and its authoritative 2D controls remain the functional
  source of truth.
