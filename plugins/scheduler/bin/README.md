# `bin/` — the `tf` binary is NOT committed

This directory is intentionally empty in git. The compiled per-arch `tf` binaries are **not**
checked in (they would bloat history and churn on every change). Instead they are published as
**GitHub Release assets** and resolved at runtime.

## How a binary gets here

`../hooks/tf-hook.sh` (the shim every hook invokes) resolves `tf` in this order:

1. **A local build** at `bin/tf-<arch>-<os>** — present in a dev tree or CI after
   `cargo build --release && cp target/release/tf bin/tf-$(uname -m)-$(uname -s | tr A-Z a-z)`.
2. **A cached download** under `$XDG_CACHE_HOME/token-fairness/<version>/`.
3. **A lazy download** of the matching asset from the release tagged `v<version>` (the version in
   `../.claude-plugin/plugin.json`), checksum-verified against the release's `SHA256SUMS`, then
   cached and executed.
4. **A `tf` on `PATH`** (e.g. `cargo install`).
5. Otherwise it **fails soft** (exit 0) — hooks are wrapped in `|| true`, so a missing binary never
   blocks a session; the next hook fire retries the download.

## Publishing (maintainers)

Binaries are built and uploaded by `.github/workflows/release.yml` on a `v*` tag:

```sh
git tag v0.1.0 && git push origin v0.1.0   # builds 4 arches, uploads + SHA256SUMS
```

Asset names follow the shim's `uname` convention: `tf-x86_64-linux`, `tf-aarch64-linux`,
`tf-x86_64-darwin`, `tf-arm64-darwin`. Bumping the plugin version requires a matching new tag.
