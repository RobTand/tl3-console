# TL3 operator console

The console is the operator's web view of TL3 (PRD RobTand/trustladderiii#327).
It shows TL3's board live: a Rust service reads TL3's Postgres store through a
read-only login, follows its change feed with `LISTEN/NOTIFY`, and streams the
board to a React app over Server-Sent Events.

The console only reads. Write paths, such as commands, answers to prompts and
settings, live in TL3. They reach the console through TL3's schema, and the
console shows their results like any other change.

This repository was split out of TL3 at commit
`43f6506dcd98c1599d9b4dadcfff476bb937332a`, so that console changes and deploys
no longer share TL3's merge queue or restart `tl3d`. Its history of
`crates/console` and `web/` comes from TL3.

## Layout

- `crates/console`: the `tl3-console` service and its tests.
- `web`: the React, TypeScript and Vite app. `web/src/api/types.gen.ts` is
  generated from the Rust API types.
- `deploy`: the release build, install and systemd unit for dl380g10.
- `tools/bazel`, `tools/ci`, `.github/workflows/gate.yml` and `.mergify.yml`:
  the build, the gate and the merge queue.

## Develop

Cargo and npm work as usual:

```sh
cargo build -p tl3-console
cd web && npm ci && npm run dev
```

The Rust build never needs Node. The service serves the built web app from the
directory its config names.

After changing an API type, regenerate the TypeScript and commit it:

```sh
TL3_WRITE_API_TYPES=1 cargo test -p tl3-console --lib
```

The console's tests start an embedded Postgres 17.4.0 as a child process. The
test support in `crates/console/tests/support` is a copy of TL3's
`crates/store/tests/support`. Keep the two in step when the TL3 pin moves.

On Rob's fleet, run tests on BuildBuddy (below) or through PrismaBuild, never
directly on a fleet host.

## Build and test with Bazel

Bazel builds the crate, runs its tests and runs the web app's checks: typecheck,
Vitest, the Vite build and Playwright against the console fixture. The Rust lib
test fails when `types.gen.ts` is stale.

Run Bazel on BuildBuddy with `bb remote`, as TL3 does. The Bazel client runs on
a Remote Bazel runner and every action runs on remote execution:

```sh
BUILDBUDDY_API_KEY="$(cat ~/.config/buildbuddy/api_key)" GIT_REPO_DEFAULT_BRANCH=main \
  bb remote \
  --container_image=docker://gcr.io/flame-public/rbe-ubuntu24-04@sha256:5f0c01e5f6d79e58d7a996330de33aa38ce599bf8431eaf27521e7602daadf50 \
  --runner_exec_properties=EstimatedCPU=8 --runner_exec_properties=EstimatedMemory=16GB \
  test //... --config=remote
```

Read the verdict from the command's exit status and the invocation it links.
Use local Bazel only for queries and for regenerating `MODULE.bazel.lock`.

Some details:

- The web tests run `npm ci`, so they have network access. `package-lock.json`
  stays the only lockfile.
- `//web:e2e` runs in Microsoft's Playwright 1.64.1 image, which carries
  Chromium and its system libraries. Move that image when `@playwright/test`
  moves.
- crate_universe builds TL3's crates from Cargo.lock's git source.
  `tl3-controller`'s build script reads TL3's `workers/` schemas, which sit
  outside the crate, so it runs from `@tl3`: TL3's full tree at the same
  commit (`tools/bazel/tl3_source.bzl`).

## CI and merging

The Gate workflow has the same shape as TL3's. On a pull request it starts
`tools/ci/bazel_gate.py` on a Remote Bazel runner. The gate selects the tests
the change can affect with bazel-diff, runs them and uploads the impacted
packages to Mergify as scopes. It reports two required checks:

- **Rust**: the Rust and gate tests Bazel selected, plus `cargo fmt` and
  `cargo clippy -D warnings` on a GitHub-hosted runner.
- **Web**: the web tests Bazel selected.

Every push to `main` runs all tests.

Pull requests merge through the Mergify queue in `.mergify.yml`. Add the
`queue` label once Rust and Web pass. Mergify tests batches on draft pull
requests and merges when Rust and Web pass on the batch. TL3's own pull
requests use the `tl3-queue` label and TL3's approval check instead.

Branch protection on `main` requires a pull request and forbids force pushes.
It requires no status checks, because Mergify would also require them on its
batch drafts. Never merge by hand: put the pull request in the queue.

## Deploy

`deploy/deploy.sh [commit]` deploys `origin/main`, or the given commit, on
dl380g10:

1. It builds a release of `tl3-console` and the web app through PrismaBuild on
   dl380g10, with SHA256SUMS over every file.
2. `deploy/deploy.py install` verifies the checksums. On the first install it
   writes `~/.config/tl3-console/console.toml` from the `[console]` section of
   TL3's `tl3.toml`. It never overwrites that file afterwards.
3. It runs the new binary's `check` against the live store, which fails when
   the store's schema is not the pinned TL3's.
4. It installs the release under `~/.local/share/tl3-console/releases/`, points
   `~/.local/share/tl3-console/current` at it, installs
   `deploy/tl3-console.service`, and restarts only `tl3-console.service`.
5. For 90 seconds the unit must stay active without restarts, and
   `GET /api/board` must return 200 with a revision. If not, deploy switches
   back to the previous release and restarts it.

A failed build or check changes nothing, and the old install keeps running.
Running deploy again for the same commit changes nothing.

The deploy never touches `tl3d`, `tl3-postgres`, `tl3-status` or TL3's config.
The `tl3_console` login and its grants are TL3's: `tl3-deploy console-role`
creates them.

The config file looks like this:

```toml
listen = "192.168.1.107:8034"
assets = "/home/rob/.local/share/tl3-console/current/assets"
user = "tl3_console"
password_file = "/home/rob/.config/tl3/secrets/console-postgres-password"
tl3_config = "/home/rob/.config/tl3/tl3.toml"
```

The console reads `tl3.toml` with the pinned TL3's own parser and uses only its
`[store]` and `[models]` sections.

## The TL3 pin

The console uses TL3's crates (`tl3-config`, `tl3-store`, `tl3-controller` and
the crates they need) at one TL3 commit. `Cargo.toml` names it once, under
`[workspace.dependencies]`, and Bazel reads it from `Cargo.lock`.

The pinned commit must match the TL3 deployed on dl380g10. The console refuses
a store whose migrations differ from the pinned TL3's. `tl3-console check`
fails, and the board stays unavailable until the schema matches. So bump the
pin in a console pull request after each TL3 deploy that changes the store
schema, the board projection or `tl3.toml`'s format:

1. Replace the commit in the four `rev = "..."` entries in `Cargo.toml`.
2. Run `cargo update -p tl3-config -p tl3-controller -p tl3-kernel -p tl3-store`.
3. Regenerate `MODULE.bazel.lock`:
   `bazel --bazelrc=<rc with startup --max_idle_secs=300> query //... --lockfile_mode=update`.
4. If TL3's `crates/store/tests/support` or `crates/config/example.toml`
   changed, copy them into `crates/console/tests`.
5. Open the pull request, queue it, and deploy once it merges.
