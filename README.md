# IronPumpkin modpack blueprint

This repository builds one IronPumpkin server binary for one modpack. The native mods of the pack
are compiled into the binary. GitHub Actions builds the binaries, so a pack maintainer does not
build on their own machine.

## Make a pack

1. Create a repository from this blueprint (fork it, or use it as a template).
2. Edit `modpack.toml`: the pack name, the IronPumpkin commit and the mods.
   Run `cargo xtask check` and commit `bin/Cargo.lock` and `bin/Cargo.lock.commit` with it.
3. Open a pull request. The `Build modpack` workflow runs `cargo xtask check` and a debug build.
4. Merge, then push a tag that starts with `v`, for example `git tag v1.0.0 && git push --tags`.
5. Download the binaries from the GitHub release of the tag:
   - `<pack>-<tag>-x86_64-linux`
   - `<pack>-<tag>-x86_64-windows.exe`
   - `SHA256SUMS`, the SHA-256 checksums of the two binaries.

Run the binary in the server directory, like the IronPumpkin binary. The log shows the mods that
loaded, for example `[ironpumpkin] loaded 1 native mod: hello-mod`.

## `modpack.toml`

```toml
[pack]
name = "my-pack"            # binary name and asset prefix: lowercase letters, digits, - and _

[ironpumpkin]
git = "https://github.com/EdenNetworkItalia/IronPumpkin"
commit = "<full 40-character commit hash>"
allow-drift = false         # optional; see "Patches"

[mods.hello-mod]            # the key is the package name of the mod crate
path = "mods/hello-mod"
```

Each mod has exactly one source:

| Source    | Keys                       | Example                                                     |
|-----------|----------------------------|-------------------------------------------------------------|
| git       | `git` and `rev`            | `git = "https://github.com/someone/my-mod"`, `rev = "v1.2.0"` |
| path      | `path`                     | `path = "mods/my-mod"`, relative to the repository root     |
| crates.io | `version`                  | `version = "1.2"`                                           |

Use a commit hash or a tag for `rev`. A branch name is not a pinned build.

## Add a mod

A mod is a Rust library crate that implements `NativeMod` from the `ironpumpkin-mods` crate and
registers it with `register_mod!`. See `examples/modpack/mods/hello-mod` in the IronPumpkin
repository.

- From git: add a `[mods.<name>]` table with `git` and `rev`. The mod crate depends on the
  IronPumpkin crates by the IronPumpkin git URL, for example
  `ironpumpkin-mods = { git = "https://github.com/EdenNetworkItalia/IronPumpkin" }`. The build
  points these dependencies at the IronPumpkin checkout of the pack, so the binary contains one
  copy of the server.
- From a path: put the crate in `mods/<name>/` and add a `[mods.<name>]` table with `path`. The
  crate depends on the IronPumpkin crates by the git URL, like a git mod, or by path into the
  checkout, for example `ironpumpkin-mods = { path = "../../.ironpumpkin/crates/ironpumpkin-mods" }`.
- From crates.io: add a `[mods.<name>]` table with `version`. The IronPumpkin crates are not on
  crates.io, so the mod crate depends on them by the git URL, never by a crates.io version.

The binary must contain exactly one copy of each server crate: the mods register in
`ironpumpkin-mods`, and a second copy is one the server never reads. The build fails when a server
crate resolves to more than one package, for example when a mod depends on IronPumpkin from a fork
URL. The build also prints one "patch was not used in the crate graph" warning per server crate
that no mod uses by the git URL; these warnings are expected.

## Patches

A mod can change the server source when a NeoForge event or API does not cover its need. This is
the compile-time equivalent of a mixin, an access transformer or reflection.

- The mod crate ships the patches as `patches/<name>.patch`, made with `git diff` against the
  root of the IronPumpkin source tree.
- Each patch has a justification file `patches/<name>.md` next to it. It says what the patch
  does, why a NeoForge event or API is not enough, and that the patch is bound to one IronPumpkin
  commit. The build fails when the file is missing.
- The mod declares the IronPumpkin commit the patches are written for:

  ```toml
  [package.metadata.ironpumpkin]
  commit = "<full 40-character commit hash>"
  ```

The build applies the patches before it compiles:

- It checks out IronPumpkin at the commit of the pack, with no local changes.
- It applies the patches of every mod in mod-id order, then in file-name order, with `git apply`:
  no fuzz, no whitespace leniency, no three-way merge. The mod id of the patch order is the key in
  `modpack.toml` (the package name of the mod crate), sorted by bytes. The server calls the `init`
  of the mods in the order of `NativeMod::id()`. The two orders are independent.
- It fails when a patch does not apply. When two mods patch the same lines, the message names
  both mods and the file. Resolve it by mod order, by one shared patch, or by proposing a NeoForge
  event for the change.
- It fails when the mod's commit is not a full 40-character hash.
- It fails when a patch is written for another commit than the pack's. The message names the mod,
  the patch and both commits. `allow-drift = true` applies the patch anyway, with a warning.
- It rejects a patch that touches a generated file under `crates/pumpkin-data/src/generated` or a
  `Cargo.lock`, in any letter case.

Governance: a patch is accepted in a mod only with its justification file. Every accepted patch
is a candidate for a NeoForge-shaped event in a later IronPumpkin version, so the API grows from
real patches.

## Build tool

`cargo xtask` runs from the repository root.

| Command                     | What it does                                                                                                                 |
|-----------------------------|------------------------------------------------------------------------------------------------------------------------------|
| `cargo xtask check`         | Validates `modpack.toml`, fetches IronPumpkin into `.ironpumpkin/`, generates the `bin` crate, resolves the mods and applies their patches. |
| `cargo xtask build`         | Runs `check` with `--locked`, then builds the release binary into `target/release/<pack>` with `--locked`.                  |
| `cargo xtask build --debug` | Same, with a debug build into `target/debug/<pack>`.                                                                         |
| `cargo xtask name`          | Prints the pack name.                                                                                                        |

The tool generates `bin/Cargo.toml` and `bin/src/mods.rs`; git ignores them and `.ironpumpkin/`.
`bin/src/mods.rs` has one `use <mod> as _;` per mod: a mod crate that the binary does not reference
is not linked, and its mod does not load.

`bin/Cargo.lock` makes the build reproducible, so it is committed. The tool seeds it from the
IronPumpkin lock file when it is missing or when `bin/Cargo.lock.commit` names another commit, so
the server dependencies have the versions of the pinned commit. `check` and `build --debug` then
add the mods' dependencies to it. The release build uses `--locked`: it fails when the committed
lock does not match `modpack.toml`, and the pull request check fails when `check` changes it.

## Licence

The IronPumpkin server is licensed under GPL-3.0. The mod API crates, `ironpumpkin-mods` and the
planned `ironpumpkin-neo`, are licensed under MIT OR Apache-2.0. A built binary links the server, so
it is a GPL-3.0 work: give the source of the pack to whoever receives the binary. A mod linked into
the binary and a patch to the server source are derivative works of the server: distribute them
under terms compatible with GPL-3.0.
