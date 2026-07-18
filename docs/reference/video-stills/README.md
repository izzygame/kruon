# Reference video stills

This folder is the lightweight visual source of truth for the crew-workspace
frontend. The original reference video is deliberately **not** stored in this
repository. Once the source is supplied again, extract the stills below into
this folder and record its timestamp plus a short observation in the table.

| File | Capture focus | What the frontend must preserve | Source timestamp |
|---|---|---|---|
| `01-overview.png` | Whole workspace composition | Spatial world stays central; controls remain visible around it | pending source |
| `02-crew-rail.png` | Left member rail | Crew identity, availability and grouping are readable at a glance | pending source |
| `03-inspector.png` | Right detail panel | Selecting a member reveals task, model/status and next action | pending source |
| `04-composer.png` | Bottom task composer | A single clear entry point starts a task without hiding scoped controls | pending source |
| `05-active-feedback.png` | Running state | Character, desk and nearby feedback show work without pretending the task is complete | pending source |
| `06-review-space.png` | Review/meeting area | The world has clear spatial semantics beyond individual desks | pending source |
| `07-camera-transition.png` | Camera/zoom state | Focus changes preserve orientation and never block the 2D control path | pending source |
| `08-atmosphere.png` | Lighting and visual hierarchy | Dark, calm, readable depth supports rather than competes with task state | pending source |

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
