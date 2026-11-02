#!/usr/bin/env bash
# Build tl3-console at origin/main (or a given commit) on dl380g10 through
# PrismaBuild, then install it and restart only tl3-console.service.
# Usage: deploy/deploy.sh [commit]. Safe to rerun: a built commit is reused,
# an installed release is not reinstalled, and a failed build changes nothing.
set -euo pipefail
repo=$(cd "$(dirname "$0")/.." && pwd)
tools=/mnt/shared/prismabuild-fleet/repo/tools
git -C "$repo" fetch -q origin
sha=$(git -C "$repo" rev-parse --verify "${1:-origin/main}^{commit}")
short=${sha:0:12}
tree=$HOME/tmp/tl3-console-deploy-$short
release=$HOME/tmp/tl3-console-release-$short
[ -d "$tree" ] || git -C "$repo" worktree add -q --detach "$tree" "$sha"
cd "$tree"
if [ ! -f "$release/SHA256SUMS" ]; then
  # dl380g10 because the release installs there; its Node and Rust are the ones it runs with.
  key=$(python3 "$tools/pbrun.py" --cwd "$PWD" --tag dl380g10 --cpus 8 --demand mem_gb=12 \
    --priority-reason "tl3-console release build $short" \
    --env PATH=/home/rob/.cargo/bin:/usr/local/bin:/usr/bin:/bin \
    --env RUSTUP_HOME=/home/rob/.rustup --env CARGO_HOME=/home/rob/.cargo --env CARGO_BUILD_JOBS=8 \
    --env OMP_NUM_THREADS=1 --env RAYON_NUM_THREADS=1 --env UV_THREADPOOL_SIZE=4 \
    --detach -- python3 deploy/deploy.py build "$release" --commit "$sha" 2>/dev/null |
    tail -1 | python3 -I -c "import json,sys; print(json.load(sys.stdin)['action_key'])")
  echo "release build $key"
  python3 "$tools/pbwait.py" --wait-s 3600 "$key" --endings > "$HOME/tmp/tl3-console-build-$short.json" 2>&1 || true
  [ -f "/mnt/shared/prismabuild-fleet/pb-queue/done/$key.json" ] || { echo "release build failed ($key); nothing installed"; exit 1; }
fi
python3 deploy/deploy.py install "$release"
