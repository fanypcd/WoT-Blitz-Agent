# AGENTS.md — `frontend/` is frozen

**Do not develop the UI here.** This directory is kept only for local debugging
(`cargo run --release -- web`); it is not deployed and not maintained.

All frontend work — Web UI, 3D replay scene, playback controls, overlays, effects, labels,
styling, and their tests — belongs to the WotbTools repository:
https://github.com/A158Coke/WotbTools

- A UI change made in this directory is dead code: nothing ships it, so the fix never reaches
  users. Reproduce the behaviour in WotbTools and change it there.
- This repository owns the **parser side**: replay decoding, facets, `PlaybackData` contract,
  data and asset pipelines. Those changes land here first, then WotbTools picks them up with its
  pinned WASM release.
- The same applies to the two-way traffic: when a UI fix needs parser support, add the parser
  part here and the UI part in WotbTools — do not smuggle UI behaviour into this repo.
- Keep this directory compiling (CI has a frontend job). Touch it only to keep that job green,
  or when the task is explicitly about this debug path.

## Visual verification is the user's job

Do not drive a browser to look at 3D replay playback, tank/model viewing or scene rendering —
here or in WotbTools — and do not treat screenshots as acceptance evidence. Land the change with
tests that lock the invariant (pure-function unit tests, source-level wiring guards, the
repository's automated browser gates), then say what the user should look at and hand the visual
check over. See root [`AGENTS.md`](../AGENTS.md) §Visual verification belongs to the user.

Background: root [`AGENTS.md`](../AGENTS.md) §Frontend ownership,
[README §分发形态](../README.md), [`docs/index.md`](../docs/index.md) §前端面收敛.
