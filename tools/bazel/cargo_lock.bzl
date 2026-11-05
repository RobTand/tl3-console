"""Derive crate_universe's synthetic root entry without changing Cargo's lockfile."""

def _package_name(package):
    return package.split("\n", 1)[0]

def _cargo_feature_lock_impl(ctx):
    parts = ctx.read(ctx.attr.source).split("\n[[package]]\n")
    packages = [part.rstrip("\n") for part in parts[1:]]
    packages.append('''
name = "direct-cargo-bazel-deps"
version = "0.0.1"
dependencies = [
 "sqlx-postgres",
]
'''.strip())
    # Match Cargo's ordering so resolution does not change this repository input's digest.
    lock = parts[0].rstrip("\n") + "\n\n[[package]]\n"
    lock += "\n\n[[package]]\n".join(sorted(packages, key = _package_name)) + "\n"
    ctx.file("Cargo.lock", lock)
    ctx.file("BUILD.bazel", 'exports_files(["Cargo.lock"])\n')

cargo_feature_lock = repository_rule(
    implementation = _cargo_feature_lock_impl,
    attrs = {"source": attr.label(mandatory = True, allow_single_file = True)},
)
