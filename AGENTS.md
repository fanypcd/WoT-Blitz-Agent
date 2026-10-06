# AGENTS.md

## Release policy

`Cargo.toml` → `[workspace.metadata.release].version` is the single source of truth for WoT-Blitz-Agent / WASM release versions.

- Keep `[workspace.metadata.release].version` as plain SemVer `x.y.z` without a leading `v`.
- Normal feature/fix PRs must not bump `workspace.metadata.release.version` unless that merge is intentionally meant to publish a release.
- A release-intended PR must bump `workspace.metadata.release.version` and update the matching release entry in `docs/index.md`.
- When `workspace.metadata.release.version` changes on `main`, `.github/workflows/release.yml` automatically runs tests, builds the WASM package, creates tag `v<version>`, and publishes the GitHub Release.
- Do not manually create or move `v*` tags or GitHub Releases in the normal release flow.
- Released tags are immutable. Never reuse an existing version for a different commit.
- `Cargo.toml` package versions are not the product/WASM release version; do not change them just to publish a GitHub release.
- Release artifacts must be named `wotb-replay-wasm-v<version>.zip`, and `fingerprint.json` must record the exact release tag and upstream commit.
- If a release job fails after tag creation, rerun the same workflow/commit instead of creating another tag.

## Getting CI results (no sleep polling)

Wait for CI with blocking watches instead of `sleep N` + polling:

- Live PR status: `gh pr checks <pr-number> --watch` (prints one line per check as its state changes; works in non-TTY/background).
- One run's verdict: `gh run watch <run-id> --exit-status --compact` (blocks to the end; when piped/backgrounded it prints the summary only at completion — don't expect incremental progress).
- Run id: `gh run list --branch <branch> --limit 1 --json databaseId,headSha` (a run is bound to its head SHA and immutable — re-inspect it instead of re-running). Right after a push the check runs may not exist yet: `gh pr checks --watch` then exits 1 with `no checks reported`; fetch the run id first and use `gh run watch`.

Exit status: with `--exit-status` the verdict *is* the process exit code — never pipe a watch through `tail` (the pipeline then reports the last command's status and swallows failures); drop the pipe or use `set -o pipefail`.

Run the watch in the background and read its output when it finishes; keep working in the meantime. A push cancels in-flight runs for the same ref (workflow concurrency) — if you need the CI evidence for a specific head, let its watch finish before pushing the next commit.

Inspecting a run while it is in flight (a snapshot, not a way to wait):

- Which step a run is on now: `gh run view <run-id> --json status,conclusion,jobs --jq '.jobs[] | select(.status != "completed") | {name, status}'` (one call; do not loop it as a substitute for `gh run watch`).
- Failed-step evidence without re-running: `gh run view <run-id> --log-failed` (whole step log); narrow it by the step's own log markers, e.g. `gh run view <run-id> --log 2>/dev/null | grep -a '\[armor-aiming\]'`.
- Which step costs what: `gh api repos/<owner>/<repo>/actions/runs/<run-id>/jobs --jq '.jobs[] | select(.name|test("<job>")) | .steps[] | "\(.number). \(.name) \((.completed_at|fromdate) - (.started_at|fromdate))s \(.conclusion)"'`.

## Parser and contract changes

- Preserve the repository's fail-closed behavior for ambiguous replay evidence; do not guess protocol semantics from a single sample.
- Public replay/facet contract changes must be documented together with the implementation.
- WotbTools consumes this repository as the upstream replay parser. Parser fixes belong here first, then WotbTools updates its pinned Agent release.

## Frontend ownership (read before touching `frontend/`)

**This repository does not own the product frontend.** Frontend development **and frontend
testing** happen in the WotbTools repository (https://github.com/A158Coke/WotbTools).

- Any Web UI / 3D replay scene / playback control / label / effect / styling change, and any
  frontend test for it, belongs in WotbTools — not here. A frontend change made here is dead
  code: nothing deploys or maintains this frontend, so the fix never reaches users.
- When a task is phrased as a frontend or UI problem (rendering, HUD, overlays, interaction),
  treat WotbTools as the implementation target and this repository as the place for
  parser / facet / data / asset-pipeline work only.
- `frontend/` is frozen and kept only for local debugging (`cargo run --release -- web`).
  Leave it read-only; do not add features, fixes, or tests to it.
- All frontend testing — including playback verification of parser/pipeline fixes — uses the
  WotbTools frontend dev server, not this frozen copy; see
  [§All frontend testing runs the WotbTools frontend](#all-frontend-testing-runs-the-wotbtools-frontend-local-dev-build)
  for the standard posture (`serve_asset_pack.mjs` + `npm run dev` + `admin=1`).
- Cross-repo consequence: UI fixes may still need parser-side support here (e.g. a data field
  the UI consumes). Land the parser part here first, then the UI change in WotbTools.
- Background: [README §与 WotbTools 的关系](README.md), [docs/index.md](docs/index.md) §前端面收敛,
  [docs/architecture-debt.md](docs/architecture-debt.md).

## Visual verification belongs to the user

**Do not perform visual acceptance yourself.** For 3D replay playback, tank/model viewing,
scene rendering, overlays, effects and any other "does it look right" question:

- Do not launch or drive a browser (including the in-app browser) to screen-read, screenshot or
  otherwise eyeball the 3D output, and do not present screenshots as acceptance evidence.
- Instead land the change with tests that lock the invariant — pure-function unit tests,
  source-level wiring guards for the scene kernel, and the repository's automated browser gates
  — then state explicitly what the user should look at and hand the visual check over.
- This applies to both sides of the split: `frontend/` here is frozen, and WotbTools is where
  the UI lives; either way the visual verdict is the user's.

Rationale: the 3D scene depends on real GPU/terrain/asset-plane state that a detached,
throttled or headless browser does not faithfully reproduce; agent-driven screenshots cost many
round-trips and still leave the verdict unproven.

## All frontend testing runs the WotbTools frontend (local dev build)

**The test frontend is always the WotbTools frontend, never this repository's frozen
`frontend/`.** The frozen copy predates current scene features (e.g. destructible tree-fall) and
only exists for `cargo run --release -- web` self-hosting — verifying against it proves nothing.
Standard local test posture:

1. `node scripts/serve_asset_pack.mjs 8123` — serves `release/asset_pack/` with CORS. If the port
   is already bound, an instance is likely still running: verify it serves the current pack
   (`curl http://127.0.0.1:8123/index.json`) instead of starting a second one — the script reads
   files from disk per request, so a running instance automatically picks up rebuilt packs.
2. In the WotbTools checkout, `frontend/.env.local` must contain
   `VITE_ASSET_BASE_URL=http://127.0.0.1:8123` (gitignored; `?assets=` in a URL overrides it and
   persists to localStorage, so clear it with an empty `?assets=` when returning to the default).
3. `npm run dev` in the WotbTools `frontend/` — Vite takes the first free port from 5173
   (stale instances often hold 5173/5174; use the port it actually prints).
4. Open the local dev page with `admin=1` and drop a `.wotbreplay` into it, e.g.
   `http://localhost:<port>/?view=agent-replay&agentViews=1&admin=1`. `admin=1` is a dev-only
   visibility bypass (WotbTools `useAuth.js`): dev builds treat the `wotbtools-admin`/`HoF-admin`
   realm roles as held, which local accounts normally lack; production builds ignore it and the
   backend still enforces real auth.
5. The WotbTools side of this workflow is documented in its `docs/frontend/local-production-dev.md`
   and `frontend/AGENTS.md`.
