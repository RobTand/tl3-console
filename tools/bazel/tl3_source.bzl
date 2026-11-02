"""TL3's source tree at the commit Cargo pins, for what crate_universe can't build alone.

crate_universe fetches each TL3 crate from git with only that crate's
directory. tl3-controller's build.rs reads ../../workers, so its build script
runs from this full tree instead (see the annotation in MODULE.bazel).

The commit comes from Cargo.lock, so Cargo.toml stays the only pin. The rule
fails when Cargo.toml and Cargo.lock disagree or name more than one commit.
TL3's own Bazel files are removed: this tree is source only.
"""

_REMOTE = "https://github.com/RobTand/trustladderiii"

def pinned_commit(cargo_toml, cargo_lock):
    """The one TL3 commit both files name. Pure, so tools/ci tests can mirror it."""
    locked = {}
    for line in cargo_lock.splitlines():
        prefix = 'source = "git+%s?rev=' % _REMOTE
        if line.startswith(prefix):
            locked[line.rpartition("#")[2].rstrip('"')] = True
    wanted = {}
    for line in cargo_toml.splitlines():
        if _REMOTE in line and "rev = " in line:
            wanted[line.split('rev = "')[1].split('"')[0]] = True
    if len(locked) != 1 or len(wanted) != 1 or locked.keys() != wanted.keys():
        fail("Cargo.toml pins TL3 at %s but Cargo.lock has %s; run cargo update for the tl3 crates" %
             (wanted.keys(), locked.keys()))
    return locked.keys()[0]

def _tl3_source_impl(ctx):
    commit = pinned_commit(ctx.read(ctx.attr.cargo_toml), ctx.read(ctx.attr.cargo_lock))
    if len(commit) != 40:
        fail("TL3 pin must be a full commit SHA, not %s" % commit)
    ctx.download_and_extract(
        url = "%s/archive/%s.tar.gz" % (_REMOTE, commit),
        stripPrefix = "trustladderiii-" + commit,
    )
    found = ctx.execute(["find", ".", "-name", "BUILD", "-o", "-name", "BUILD.bazel", "-o",
                         "-name", "*.bzl", "-o", "-name", "MODULE.bazel", "-o", "-name", "REPO.bazel",
                         "-o", "-name", "WORKSPACE*", "-o", "-name", ".bazelignore"])
    if found.return_code:
        fail("cannot list TL3's Bazel files: " + found.stderr)
    for path in found.stdout.splitlines():
        ctx.delete(path)
    ctx.file("REPO.bazel", "")
    ctx.file("BUILD.bazel", """filegroup(
    name = "worker_schemas",
    srcs = glob(["workers/src/tl3_workers/schemas/*.json"]),
    visibility = ["//visibility:public"],
)
""")
    ctx.file("crates/controller/BUILD.bazel", """load("@rules_rust//cargo:defs.bzl", "cargo_build_script")

# build.rs writes include_str! paths under CARGO_MANIFEST_DIR/../../workers.
# Its build and the library's compile both get these files at their exec-root
# paths; the manifest makes crates/controller a directory, so ".." resolves.
# BuildBuddy runs every action in one fixed exec root, so the paths agree.
filegroup(
    name = "schemas",
    srcs = ["Cargo.toml", "//:worker_schemas"],
    visibility = ["//visibility:public"],
)

cargo_build_script(
    name = "build_script",
    srcs = ["build.rs"],
    crate_name = "tl3_controller",
    pkg_name = "tl3-controller",
    edition = "2024",
    data = [":schemas"],
    # rules_rust's runner joins this relative path onto the exec root.
    build_script_env = {"CARGO_MANIFEST_DIR": "external/+tl3_source+tl3/crates/controller"},
    rundir = ".",
    visibility = ["//visibility:public"],
)
""")
    ctx.file("PINNED_COMMIT", commit + "\n")

tl3_source = repository_rule(
    implementation = _tl3_source_impl,
    attrs = {
        "cargo_lock": attr.label(mandatory = True, allow_single_file = True),
        "cargo_toml": attr.label(mandatory = True, allow_single_file = True),
    },
)
