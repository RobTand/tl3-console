#!/usr/bin/env bash
# Run one of the web app's checks from a scratch copy of web/: typecheck,
# unit (Vitest), build (Vite) or e2e (Playwright against the console fixture).
set -euo pipefail
step=${1:?usage: npm_test.sh typecheck|unit|build|e2e}
scratch=${TEST_TMPDIR:?run under bazel test}
mkdir -p "$scratch/bin" "$scratch/web"
ln -sf "$(realpath "$NODE")" "$scratch/bin/node"
node_root=$(dirname "$(dirname "$(realpath "$NODE")")")
export PATH="$scratch/bin:$PATH" HOME="$scratch" npm_config_cache="$scratch/npm-cache"
for name in TL3_CONSOLE_FIXTURE TL3_TEST_POSTGRES_BIN; do
  if [ -n "${!name:-}" ]; then export "$name=$(realpath "${!name}")"; fi
done
cp -RL web/. "$scratch/web/"
cd "$scratch/web"
npm() { node "$node_root/lib/node_modules/npm/bin/npm-cli.js" "$@"; }
npm ci --no-audit --no-fund
case $step in
  typecheck) node node_modules/typescript/bin/tsc --noEmit ;;
  unit) node node_modules/vitest/vitest.mjs run ;;
  build) node node_modules/vite/bin/vite.js build && test -s dist/index.html ;;
  e2e)
    node node_modules/vite/bin/vite.js build
    TL3_SCREENSHOTS="${TEST_UNDECLARED_OUTPUTS_DIR:-$scratch}" node node_modules/@playwright/test/cli.js test
    ;;
  *) echo "unknown step $step" >&2; exit 2 ;;
esac
