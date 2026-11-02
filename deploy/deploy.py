#!/usr/bin/env python3
"""Build and install the TL3 operator console on dl380g10, without printing credentials.

build ARTIFACTS   Inside a PrismaBuild action: a release build of tl3-console and
                  the web app, copied to a new directory with SHA256SUMS.
install ARTIFACTS Verify the checksums, write the console's config once, check the
                  new binary against the live store, switch releases, install the
                  unit, restart only tl3-console.service and watch it for 90 s.
                  On any failure before the switch the old install keeps running;
                  a failed stability check switches back and restarts the old one.

deploy/deploy.sh runs both for a commit of origin/main.
"""
import argparse
import hashlib
import http.client
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import time
import tomllib

SOURCE = Path(__file__).resolve().parents[1]
UNIT = "tl3-console.service"
STABLE_SECONDS = 90


def paths(home):
    root = home / ".local/share/tl3-console"
    return {
        "root": root,
        "releases": root / "releases",
        "current": root / "current",
        "config": home / ".config/tl3-console/console.toml",
        "tl3_config": home / ".config/tl3/tl3.toml",
        "unit": home / ".config/systemd/user" / UNIT,
    }


def run(command, **kwargs):
    print("$ " + " ".join(map(str, command)), flush=True)
    subprocess.run([str(part) for part in command], check=True, **kwargs)


def checksums(directory):
    """SHA256SUMS lines for every file under directory, sorted by path."""
    lines = []
    for path in sorted(p for p in directory.rglob("*") if p.is_file() and p.name != "SHA256SUMS"):
        digest = hashlib.sha256(path.read_bytes()).hexdigest()
        lines.append(f"{digest}  {path.relative_to(directory)}")
    return "\n".join(lines) + "\n"


def verify(directory):
    recorded = (directory / "SHA256SUMS").read_text()
    if recorded != checksums(directory):
        raise RuntimeError(f"{directory} does not match its SHA256SUMS")
    return hashlib.sha256(recorded.encode()).hexdigest()


def build(output, commit, source=SOURCE):
    if not output.is_absolute() or output.exists():
        raise RuntimeError("build needs a new absolute artifacts directory")
    run(["cargo", "build", "--release", "--locked", "-p", "tl3-console", "--bins"], cwd=source)
    web = source / "web"
    run(["npm", "ci", "--no-audit", "--no-fund"], cwd=web)
    run(["npm", "run", "build"], cwd=web)
    target = Path(os.environ.get("CARGO_TARGET_DIR", source / "target"))
    staging = output.with_name(output.name + ".partial")
    shutil.rmtree(staging, ignore_errors=True)
    (staging / "bin").mkdir(parents=True, mode=0o700)
    shutil.copy2(target / "release/tl3-console", staging / "bin/tl3-console")
    shutil.copytree(web / "dist", staging / "assets")
    # PrismaBuild runs a snapshot of the checkout, so the caller names the commit.
    (staging / "COMMIT").write_text(commit + "\n")
    (staging / "SHA256SUMS").write_text(checksums(staging))
    staging.rename(output)
    print((output / "SHA256SUMS").read_text(), end="")
    print(f"Release artifacts: {output}")


def console_config(home, tl3_config):
    """The console's own config, from the [console] section TL3's deploy wrote."""
    section = tomllib.loads(tl3_config.read_text()).get("console")
    if not section or not section.get("user") or not section.get("password_file"):
        raise RuntimeError(f"{tl3_config} has no [console] listen, user and password_file to start from")
    values = {
        "listen": section["listen"],
        "assets": str(paths(home)["current"] / "assets"),
        "user": section["user"],
        "password_file": section["password_file"],
        "tl3_config": str(tl3_config),
    }
    return ("# Written once by tl3-console's deploy from TL3's [console] section.\n"
            "# Deploys never overwrite it; edit it here.\n"
            + "".join(f"{key} = {json.dumps(value)}\n" for key, value in values.items()))


def write_once(path, text):
    """Create path with text unless it exists. Returns True when written."""
    if path.exists():
        return False
    path.parent.mkdir(parents=True, exist_ok=True, mode=0o700)
    staging = path.with_name(path.name + ".new")
    staging.write_text(text)
    staging.chmod(0o600)
    staging.rename(path)
    return True


def unit_text():
    return (SOURCE / "deploy/tl3-console.service").read_text()


def systemctl(*args, check=True):
    return subprocess.run(["systemctl", "--user", *args], check=check, capture_output=True, text=True)


def board(listen):
    """Status and revision of GET /api/board, or (None, None) when unreachable."""
    host, port = listen.rsplit(":", 1)
    connection = http.client.HTTPConnection(host, int(port), timeout=5)
    try:
        connection.request("GET", "/api/board")
        response = connection.getresponse()
        body = response.read()
        revision = json.loads(body).get("revision") if response.status == 200 else None
        return response.status, revision
    except (OSError, ValueError):
        return None, None
    finally:
        connection.close()


def stable(listen, seconds=STABLE_SECONDS, clock=time.monotonic, sleep=time.sleep):
    """The unit stays active with no restarts, and the board answers with a revision."""
    started = clock()
    first = None
    while True:
        active = systemctl("is-active", "--quiet", UNIT, check=False).returncode == 0
        restarts = systemctl("show", UNIT, "-p", "NRestarts", "--value").stdout.strip()
        status, revision = board(listen)
        if first is None and status == 200 and revision is not None:
            first = revision
        elapsed = clock() - started
        print(f"stability {elapsed:5.1f}s: active={active} restarts={restarts} "
              f"board={status} revision={revision}", flush=True)
        if not active or restarts != "0":
            return False
        if elapsed >= seconds:
            return status == 200 and revision is not None
        sleep(15)


def install(artifacts, home):
    where = paths(home)
    digest = verify(artifacts)
    print(f"checksums OK ({digest[:12]})")
    if write_once(where["config"], console_config(home, where["tl3_config"])):
        print(f"wrote {where['config']} from TL3's [console] section")
    listen = tomllib.loads(where["config"].read_text())["listen"]
    # The pin guard: the new binary must accept the live store's schema.
    run([artifacts / "bin/tl3-console", "check", "--config", where["config"]])
    release = where["releases"] / f"{(artifacts / 'COMMIT').read_text().strip()[:12]}-{digest[:12]}"
    if not release.exists():
        staging = release.with_name(release.name + ".partial")
        shutil.rmtree(staging, ignore_errors=True)
        shutil.copytree(artifacts, staging, symlinks=False)
        staging.rename(release)
    verify(release)
    previous = where["current"].resolve() if where["current"].is_symlink() else None
    old_unit = where["unit"].read_text() if where["unit"].exists() else None
    changed = previous != release or old_unit != unit_text() or \
        systemctl("is-active", "--quiet", UNIT, check=False).returncode != 0
    if not changed:
        print(f"{release.name} is already installed and running")
    else:
        switch(where["current"], release)
        where["unit"].parent.mkdir(parents=True, exist_ok=True)
        where["unit"].write_text(unit_text())
        systemctl("daemon-reload")
        systemctl("enable", "--quiet", UNIT)
        systemctl("reset-failed", UNIT, check=False)
        systemctl("restart", UNIT)
        print(f"switched to {release.name}; restarted {UNIT}")
    if stable(listen):
        print(f"{UNIT} stable for {STABLE_SECONDS} s on {release.name}")
        return 0
    if changed and previous is not None:
        print(f"{UNIT} is not stable; switching back to {previous.name}")
        switch(where["current"], previous)
        if old_unit is not None:
            where["unit"].write_text(old_unit)
        systemctl("daemon-reload")
        systemctl("reset-failed", UNIT, check=False)
        systemctl("restart", UNIT)
    return 1


def switch(link, release):
    staging = link.with_name(link.name + ".new")
    staging.unlink(missing_ok=True)
    staging.symlink_to(release)
    os.replace(staging, link)


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("command", choices=["build", "install"])
    parser.add_argument("artifacts", type=Path)
    parser.add_argument("--home", type=Path, default=Path.home())
    parser.add_argument("--commit", help="the source commit, for build")
    args = parser.parse_args(argv)
    if args.command == "build":
        if not args.commit or len(args.commit) != 40:
            parser.error("build needs --commit FULL_SHA")
        build(args.artifacts, args.commit)
        return 0
    return install(args.artifacts, args.home)


if __name__ == "__main__":
    sys.exit(main())
