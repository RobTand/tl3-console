#!/usr/bin/env bash
# Run from a clean checkout inside a PrismaBuild action.
set -euo pipefail
: "${PRISMABUILD_ACTION_KEY:?Submit this script through PrismaBuild}"
export CARGO_BUILD_JOBS=4 RUST_TEST_THREADS=1 OMP_NUM_THREADS=1 MKL_NUM_THREADS=1 OPENBLAS_NUM_THREADS=1 RAYON_NUM_THREADS=1 TOKIO_WORKER_THREADS=2
step=${1:?usage: postmortems.sh rust|web}
case "$step" in
  rust)
    cargo fmt --check
    cargo clippy --workspace --all-targets --locked -- -D warnings
    cargo test --workspace --all-targets --locked
    ;;
  web)
    node_dir=$(mktemp -d)
    trap 'rm -rf "$node_dir"' EXIT
    case $(uname -m) in
      x86_64) node_arch=x64; node_sha=1805bcd35e6815559bd2eb97db438d3fdb0eb3a5e6a977b7c1badca76337e9ce ;;
      aarch64) node_arch=arm64; node_sha=6287aa46b6c375d816e63ae66d024fbbfd70404825aabdfdf898a970871d2f4b ;;
      *) echo 'Unsupported Node architecture' >&2; exit 1 ;;
    esac
    curl -fsSL --max-time 120 "https://nodejs.org/dist/v22.23.4/node-v22.23.4-linux-$node_arch.tar.xz" -o "$node_dir/node.tar.xz"
    echo "$node_sha  $node_dir/node.tar.xz" | sha256sum --check
    tar -xJf "$node_dir/node.tar.xz" -C "$node_dir"
    export PATH="$node_dir/node-v22.23.4-linux-$node_arch/bin:$PATH"
    cargo build -p tl3-console --example console_fixture --locked
    export TL3_CONSOLE_FIXTURE="${CARGO_TARGET_DIR:-$PWD/target}/debug/examples/console_fixture"
    cd web
    npm ci --no-audit --no-fund
    npm run build
    npm test -- --maxWorkers=1
    npx playwright install chromium
    npx playwright test
    ;;
  *) echo "Unknown check: $step" >&2; exit 2 ;;
esac
