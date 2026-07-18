# Frontend preview baselines

`crew-workspace-preview-2026-07-18.png` is a local Vite rendering of the new
main workspace shell. It is an implementation baseline for visual regression
review, not a replacement for the missing reference-video stills.

The preview uses in-memory fixture data only when opened in a non-Tauri Vite
development browser. The production bundle and the Tauri application continue
to use the real local runtime.
