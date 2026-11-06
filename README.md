# TL3 operator console

The console is the operator's web view of TL3 (PRD RobTand/trustladderiii#327).
It shows TL3's board and case files, request commands and their outcomes,
and each role's prompt versions. A Rust service reads TL3's Postgres store,
follows its change feed with `LISTEN/NOTIFY`, and streams changes to a React
app over Server-Sent Events.

The console files typed request commands and answers for TL3 to apply. It also
saves prompt versions, activates or reverts them, and acknowledges built-in
prompt updates. Its database login has grants for these writes. TL3 remains
the only writer of the world snapshot and event log.

This repository was split out of TL3 so that console changes and deploys no
longer share TL3's merge queue or restart `tl3d`. Its history of
`crates/console` and `web/` comes from TL3. The current TL3 dependency commit is
pinned in `Cargo.toml`.

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
   the store's schema is older than the pinned TL3's or disagrees with it. A
   store that is ahead passes with a warning. The check also reads the board
   projection and refuses stored values the pinned TL3 cannot decode, before
   switching the release or restarting the console.
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
traces = "http://192.168.1.107:5033/#/experiments/2/traces"
public_hosts = ["console.example.test"]
hide_repos = ["RobTand/tl3-sandbox"]
```

`traces` is optional. With it, the case file links each worker run to its
MLflow traces; without it, runs have no trace link. Deploy writes the file only
once, so add the line by hand to an existing install and restart the console.

`public_hosts` is optional. See "Host names" below.

`hide_repos` is optional and defaults to none. See "Hidden repositories" below.

The console reads `[store]`, `[models]` and `github.repos` through its own
config boundary. Unknown TL3 keys and unrelated sections are ignored, including
new keys inside model ladders and repository entries. Missing or mistyped used
sections still fail startup. The console uses its own login from `console.toml`,
and repository branch templates name the case file's branch links. TL3's own
config parser remains strict. Deploy's one-time import of `[console]` uses
Python's TOML parser, which already accepts additional keys.

## Hidden repositories

`hide_repos` lists served repositories (`owner/name`, without regard to case)
whose requests the console never shows. The console filters on the server, so
nothing hidden reaches the browser:

- the board snapshot, every stream patch and the flow counts leave the
  repository's requests out, and so do Needs Rob and "Let TL3 decide on all";
- open incidents that name the repository without belonging to a request,
  which TL3 raises when it fails to read one of its issues, leave the list of
  incidents without a request;
- the `commands` stream event leaves out commands filed for its requests,
  including requests a newer one replaced;
- the case file (`GET /api/requests/{id}`) and the command route
  (`POST /api/requests/{id}/commands`) answer `404` for its requests and file
  nothing. The command route checks the request's repository before it reads
  the body.

An entry that isn't one of TL3's served repositories (`github.repos` in
`tl3.toml`) fails `tl3-console check` and the console's start with a message
that names it, so a typo can't quietly hide nothing. Deploy runs `check` before
it installs. Incidents that name no repository, such as a failed read of the
operations repository, still show. Add the line to the config by hand and
restart the console.

## Postmortems

The Postmortems page searches findings and recommendation evidence across
requests. Filter by repository, cause, selection reason, status and owed date.
Dates are inclusive UTC dates. Rows open the request's Postmortem drawer tab.
The tab shows every trigger, the recorded code grades and facts, and the judge's
findings when available. Recommendation counts include distinct visible
postmortems with the same fingerprint, across all pages.

Closed requests and requests in Needs Rob offer **Write postmortem** in the
drawer. Needs Rob cards offer it too. Confirm once to file the existing typed
console command. The stream reports whether TL3 applied or refused it, with
TL3's own reason. An owed or running postmortem disables the button.

Read-only API routes:

- `GET /api/postmortems`: filters `q`, `repo`, `cause`, `reason`, `status`,
  `from`, `to`, `limit` and `before`.
- `GET /api/requests/{id}/postmortems`: postmortems and code grades for a request. `before` and `grade_before` page
  each history independently.

Pages default to 50 postmortems, accept at most 100 and return `next_before`.
Request reads return at most 100 grades with `next_grade_before`. Queries use
migration 13's indexes and a three-second statement limit, on at most four
read-only sessions. Hidden repositories are excluded from reads and counts.
The `postmortems` stream event invalidates open queries whenever postmortems,
recommendations or grades change, and on reconnect. There is no refresh timer.

## Host names

A web page on another site can point its own host name at the console's
address (DNS rebinding). The browser then treats the console as that site and
sends that name as `Host` and `Origin`, so checking `Origin` against `Host`
proves nothing. The console answers only to the `Host` names it serves and
refuses every other request with `421 Misdirected Request`. Nothing is served.
That holds for every route, reads and the event stream included, and for
requests with no `Host`.

The console serves:

- its `listen` address with the port, for example `192.168.1.107:8034`;
- `localhost:<port>`, when `listen` is a loopback address;
- each name in `public_hosts`, such as a tunnel's public name, compared without
  regard to case. Give the port too when clients use one. A name is letters,
  digits and `.-:[]`; the console refuses to start on anything else.

A browser that reaches the console by another name, such as a DNS alias, gets
`421` until you add that name to `public_hosts` and restart the console.
`npm run dev` proxies `/api` with the console's own `Host`, so it works too.

Every route also reads at most 64 KiB of request body. A larger body gets
`413`. The limit sits next to the allowlist in `server::harden`, so a route
added later is covered without extra code.

## The TL3 pin

The console uses TL3's crates (`tl3-config`, `tl3-store`, `tl3-controller` and
the crates they need) at one TL3 commit. `Cargo.toml` names it once, under
`[workspace.dependencies]`, and Bazel reads it from `Cargo.lock`.

The pinned commit should be the TL3 deployed on dl380g10, or an older one.
TL3's migrations only add, so the console accepts a store whose applied
migrations are the pinned TL3's plus newer versions. It logs a warning that
names the extra versions, each time the board, the case file or
`tl3-console check` opens a store session, and keeps serving. Every store
session opens through `schema::Session`, so they all follow this rule. It refuses a store that is older than the pin, or that lacks or
changes a migration the pin knows. `crates/console/migrations.pin` lists the
pinned TL3's migrations with an md5 of each one's SQL, which is how the
console tells the two apart. A console test checks that file against a store
the pinned TL3 migrates.

A warning means a TL3 deploy added migrations. The board may lack what they
add: the console shows only what the pinned projection reads. Bump the pin in
a console pull request when a TL3 deploy changes the store schema, the board
projection or `tl3.toml`'s format:

1. Replace the commit in the four `rev = "..."` entries in `Cargo.toml`.
2. Run `cargo update -p tl3-config -p tl3-controller -p tl3-kernel -p tl3-store`.
3. Regenerate `MODULE.bazel.lock`:
   `bazel --bazelrc=<rc with startup --max_idle_secs=300> query //... --lockfile_mode=update`.
4. If TL3's `crates/store/tests/support` or `crates/config/example.toml`
   changed, copy them into `crates/console/tests`.
5. Run the console test. If `pinned_migrations_match_the_pinned_tl3` fails,
   replace the body of `crates/console/migrations.pin` with the lines it
   prints.
6. Open the pull request, queue it, and deploy once it merges.
