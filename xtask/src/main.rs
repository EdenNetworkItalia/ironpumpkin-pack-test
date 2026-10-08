//! `cargo xtask`: builds the modpack binary from `modpack.toml`.

mod modpack;
mod patches;

use std::{
    env,
    fmt::Write as _,
    fs,
    path::{Path, PathBuf},
    process::{Command, ExitCode},
};

use modpack::{ModSource, Modpack};
use patches::ModPatches;
use serde_json::Value;

const USAGE: &str = "usage: cargo xtask <command>

commands:
  check            validate modpack.toml, fetch IronPumpkin and apply the mods' patches
  build [--debug]  check, then build the pack binary (release unless --debug)
  name             print the pack name";

/// The `IronPumpkin` source checkout, relative to the blueprint root.
const CHECKOUT_DIR: &str = ".ironpumpkin";
/// The URL that mods outside this tree use for their `IronPumpkin` dependencies.
const UPSTREAM_GIT: &str = "https://github.com/EdenNetworkItalia/IronPumpkin";
/// The `IronPumpkin` commit that `bin/Cargo.lock` was seeded from, relative to the blueprint root.
const LOCK_COMMIT_FILE: &str = "bin/Cargo.lock.commit";

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), String> {
    let root = &blueprint_root()?;
    let args: Vec<String> = env::args().skip(1).collect();
    match args
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>()
        .as_slice()
    {
        ["check"] => prepare(root, false).map(drop),
        ["build"] => build(root, true),
        ["build", "--debug"] => build(root, false),
        ["name"] => {
            println!("{}", load(root)?.name);
            Ok(())
        }
        _ => Err(USAGE.into()),
    }
}

/// The blueprint root, read at run time: a copied blueprint that shares a target directory runs
/// the same xtask binary, and must not act on the tree the binary was built from.
fn blueprint_root() -> Result<PathBuf, String> {
    let root = match env::var_os("CARGO_MANIFEST_DIR") {
        Some(xtask_dir) => Path::new(&xtask_dir)
            .parent()
            .ok_or("the xtask crate has no parent directory")?
            .to_path_buf(),
        None => env::current_dir().map_err(|e| format!("no current directory: {e}"))?,
    };
    if root.join("modpack.toml").is_file() {
        Ok(root)
    } else {
        Err(format!(
            "{} has no modpack.toml; run `cargo xtask` from the blueprint root",
            root.display()
        ))
    }
}

fn load(root: &Path) -> Result<Modpack, String> {
    let path = root.join("modpack.toml");
    let text = fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    modpack::parse(&text)
}

struct Prepared {
    pack: Modpack,
    target_dir: PathBuf,
}

/// Validates the pack, syncs the checkout, writes the bin crate and applies the patches. With
/// `locked`, `bin/Cargo.lock` must already match the pack.
fn prepare(root: &Path, locked: bool) -> Result<Prepared, String> {
    let pack = load(root)?;
    let checkout = root.join(CHECKOUT_DIR);
    sync_checkout(&checkout, &pack.git, &pack.commit)?;
    let crates = write_manifest(root, &checkout, &pack)?;
    seed_lock(root, &checkout, &pack.commit, locked)?;
    let metadata = cargo_metadata(root, locked)?;
    check_one_copy(&metadata, &crates)?;
    let mods = resolve_mods(&metadata, &pack)?;
    write_mods_rs(root, &mods)?;
    let patches: Vec<ModPatches> = mods.into_iter().map(|m| m.patches).collect();
    let applied = patches::apply_all(&checkout, &pack.commit, pack.allow_drift, &patches)?;
    eprintln!(
        "[xtask] {}: IronPumpkin {}, {} mod(s), {applied} patch(es) applied",
        pack.name,
        &pack.commit[..12],
        patches.len()
    );
    let target_dir = metadata["target_directory"]
        .as_str()
        .ok_or("cargo metadata has no target_directory")?
        .into();
    Ok(Prepared { pack, target_dir })
}

fn build(root: &Path, release: bool) -> Result<(), String> {
    let Prepared { pack, target_dir } = prepare(root, release)?;
    let mut command = Command::new(cargo());
    command
        .args(["build", "--manifest-path", "bin/Cargo.toml"])
        .current_dir(root);
    if release {
        command.args(["--release", "--locked"]);
    }
    let status = command
        .status()
        .map_err(|e| format!("cannot run cargo: {e}"))?;
    if !status.success() {
        return Err(format!("cargo build failed: {status}"));
    }
    let binary = target_dir
        .join(if release { "release" } else { "debug" })
        .join(format!("{}{}", pack.name, env::consts::EXE_SUFFIX));
    eprintln!("[xtask] built {}", binary.display());
    Ok(())
}

fn cargo() -> String {
    env::var("CARGO").unwrap_or_else(|_| "cargo".into())
}

fn git(dir: &Path, args: &[&str]) -> Result<(), String> {
    let output = Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .map_err(|e| format!("cannot run git: {e}"))?;
    if output.status.success() {
        Ok(())
    } else {
        Err(format!(
            "git {} failed in {}: {}",
            args.join(" "),
            dir.display(),
            String::from_utf8_lossy(&output.stderr).trim()
        ))
    }
}

/// Puts the checkout at `commit` with no local changes, so patches always apply to a clean tree.
fn sync_checkout(dir: &Path, url: &str, commit: &str) -> Result<(), String> {
    if dir.join(".git").exists() {
        git(dir, &["remote", "set-url", "--", "origin", url])?;
    } else {
        fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        git(dir, &["init", "-q"])?;
        git(dir, &["remote", "add", "--", "origin", url])?;
    }
    // Patches are LF diffs; a Windows default of `core.autocrlf=true` would check out CRLF files.
    git(dir, &["config", "core.autocrlf", "false"])?;
    if git(dir, &["cat-file", "-e", &format!("{commit}^{{commit}}")]).is_err() {
        eprintln!("[xtask] fetching IronPumpkin {commit} from {url}");
        // A server that does not serve a commit by hash still serves its branches and tags.
        if git(dir, &["fetch", "-q", "--depth", "1", "origin", commit]).is_err() {
            git(
                dir,
                &[
                    "fetch",
                    "-q",
                    "--tags",
                    "origin",
                    "+refs/heads/*:refs/remotes/origin/*",
                ],
            )?;
        }
    }
    git(
        dir,
        &[
            "-c",
            "advice.detachedHead=false",
            "-c",
            "core.autocrlf=false",
            "checkout",
            "-q",
            "--force",
            "--detach",
            commit,
        ],
    )?;
    git(dir, &["clean", "-q", "-f", "-d"])
}

fn read_toml(path: &Path) -> Result<toml::Table, String> {
    let text = fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    text.parse().map_err(|e| format!("{}: {e}", path.display()))
}

fn quote(value: &str) -> String {
    toml::Value::String(value.to_owned()).to_string()
}

/// Writes `bin/Cargo.toml` and returns the server crates, as [`server_crates`] lists them.
fn write_manifest(
    root: &Path,
    checkout: &Path,
    pack: &Modpack,
) -> Result<Vec<(String, String)>, String> {
    let server = read_toml(&checkout.join("Cargo.toml"))?;
    let mut manifest = format!(
        "# Generated by `cargo xtask` from modpack.toml. Do not edit.\n\n\
         [package]\nname = {}\nversion = \"0.1.0\"\nedition = \"2024\"\n\
         license = \"GPL-3.0\"\npublish = false\n\n\
         [dependencies]\npumpkin = {{ path = {} }}\n",
        quote(&pack.name),
        quote(&format!("../{CHECKOUT_DIR}/crates/pumpkin")),
    );
    for (id, source) in &pack.mods {
        let spec = match source {
            ModSource::Path(path) if Path::new(path).is_absolute() => {
                format!("path = {}", quote(path))
            }
            ModSource::Path(path) => format!("path = {}", quote(&format!("../{path}"))),
            ModSource::Git { url, rev } => format!("git = {}, rev = {}", quote(url), quote(rev)),
            ModSource::Version(version) => format!("version = {}", quote(version)),
        };
        let _ = writeln!(manifest, "{id} = {{ {spec} }}");
    }
    manifest.push_str("\n[workspace]\n");
    // Mods depend on IronPumpkin by git URL. The patch section points those dependencies at the
    // patched checkout, so the binary links one copy of each server crate.
    let crates = server_crates(checkout, &server)?;
    let mut urls = vec![UPSTREAM_GIT];
    if pack.git != UPSTREAM_GIT {
        urls.push(&pack.git);
    }
    for url in urls {
        let _ = write!(manifest, "\n[patch.{}]\n", quote(url));
        for (name, member) in &crates {
            let _ = writeln!(
                manifest,
                "{name} = {{ path = {} }}",
                quote(&format!("../{CHECKOUT_DIR}/{member}"))
            );
        }
    }
    // A separate workspace does not inherit the profiles of the server workspace.
    if let Some(profile) = server.get("profile") {
        let mut table = toml::Table::new();
        table.insert("profile".into(), profile.clone());
        manifest.push('\n');
        manifest.push_str(&toml::to_string(&table).map_err(|e| e.to_string())?);
    }
    write(&root.join("bin/Cargo.toml"), &manifest)?;
    Ok(crates)
}

/// Seeds `bin/Cargo.lock` from the server's lock file, which pins the dependency versions the
/// pinned commit was tested with, when the lock is missing or was seeded for another commit.
fn seed_lock(root: &Path, checkout: &Path, commit: &str, locked: bool) -> Result<(), String> {
    let lock = root.join("bin/Cargo.lock");
    let commit_file = root.join(LOCK_COMMIT_FILE);
    let seeded_for = fs::read_to_string(&commit_file).unwrap_or_default();
    if lock.is_file() && seeded_for.trim() == commit {
        return Ok(());
    }
    if locked {
        return Err(format!(
            "bin/Cargo.lock is missing or was seeded for another IronPumpkin commit; run \
             `cargo xtask check` and commit bin/Cargo.lock and {LOCK_COMMIT_FILE}"
        ));
    }
    eprintln!("[xtask] seeding bin/Cargo.lock from IronPumpkin {commit}");
    fs::copy(checkout.join("Cargo.lock"), &lock)
        .map_err(|e| format!("cannot seed bin/Cargo.lock: {e}"))?;
    write(&commit_file, &format!("{commit}\n"))
}

/// Fails when a server crate resolves to more than one package: the mods would register in a
/// second copy of `ironpumpkin-mods` that the server never reads.
fn check_one_copy(metadata: &Value, crates: &[(String, String)]) -> Result<(), String> {
    let packages = metadata["packages"]
        .as_array()
        .ok_or("cargo metadata has no packages")?;
    for (name, _) in crates {
        let ids: Vec<&str> = packages
            .iter()
            .filter(|p| p["name"] == name.as_str())
            .filter_map(|p| p["id"].as_str())
            .collect();
        if ids.len() > 1 {
            return Err(format!(
                "the build has {} copies of the server crate `{name}`: {}. A mod depends on \
                 IronPumpkin from another source (a fork URL or crates.io); make it depend on \
                 {UPSTREAM_GIT} or on a path into {CHECKOUT_DIR}/",
                ids.len(),
                ids.join(", ")
            ));
        }
    }
    Ok(())
}

/// The package name and member path of each crate under `crates/` in the server workspace.
fn server_crates(checkout: &Path, server: &toml::Table) -> Result<Vec<(String, String)>, String> {
    let members = server
        .get("workspace")
        .and_then(|w| w.get("members"))
        .and_then(toml::Value::as_array)
        .ok_or("the IronPumpkin Cargo.toml has no workspace members")?;
    let mut crates = Vec::new();
    for member in members.iter().filter_map(toml::Value::as_str) {
        if !member.starts_with("crates/") {
            continue;
        }
        let manifest = read_toml(&checkout.join(member).join("Cargo.toml"))?;
        let name = manifest
            .get("package")
            .and_then(|p| p.get("name"))
            .and_then(toml::Value::as_str)
            .ok_or_else(|| format!("{member}/Cargo.toml has no package name"))?;
        crates.push((name.to_owned(), member.to_owned()));
    }
    Ok(crates)
}

fn write(path: &Path, text: &str) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
    }
    fs::write(path, text).map_err(|e| format!("{}: {e}", path.display()))
}

fn cargo_metadata(root: &Path, locked: bool) -> Result<Value, String> {
    let mut command = Command::new(cargo());
    command
        .args([
            "metadata",
            "--format-version",
            "1",
            "--manifest-path",
            "bin/Cargo.toml",
        ])
        .current_dir(root);
    if locked {
        command.arg("--locked");
    }
    let output = command
        .stderr(std::process::Stdio::inherit())
        .output()
        .map_err(|e| format!("cannot run cargo: {e}"))?;
    if !output.status.success() {
        return Err(format!("cargo metadata failed: {}", output.status));
    }
    serde_json::from_slice(&output.stdout).map_err(|e| format!("cargo metadata: {e}"))
}

struct ResolvedMod {
    /// The name of the library target, as Rust code refers to it.
    lib_name: String,
    patches: ModPatches,
}

/// Finds the crate of every mod in the resolved dependency graph of the bin crate, wherever cargo
/// put its sources (a path, a git checkout or the registry cache).
fn resolve_mods(metadata: &Value, pack: &Modpack) -> Result<Vec<ResolvedMod>, String> {
    let packages = metadata["packages"]
        .as_array()
        .ok_or("cargo metadata has no packages")?;
    let root_id = &metadata["resolve"]["root"];
    let root_node = metadata["resolve"]["nodes"]
        .as_array()
        .and_then(|nodes| nodes.iter().find(|node| &node["id"] == root_id))
        .ok_or("cargo metadata has no node for the bin crate")?;
    let dependencies: Vec<&Value> = root_node["deps"]
        .as_array()
        .map(|deps| {
            deps.iter()
                .filter_map(|dep| packages.iter().find(|p| p["id"] == dep["pkg"]))
                .collect()
        })
        .unwrap_or_default();
    let mut mods = Vec::new();
    for id in pack.mods.keys() {
        let package = dependencies
            .iter()
            .find(|p| p["name"] == id.as_str())
            .ok_or_else(|| {
                format!("mod `{id}`: its source has no package named `{id}`; the key in modpack.toml must be the package name")
            })?;
        let manifest_path = package["manifest_path"]
            .as_str()
            .ok_or_else(|| format!("mod `{id}`: no manifest path in cargo metadata"))?;
        let lib_name = package["targets"]
            .as_array()
            .and_then(|targets| {
                targets.iter().find(|t| {
                    t["kind"]
                        .as_array()
                        .is_some_and(|kinds| kinds.iter().any(|k| k == "lib"))
                })
            })
            .and_then(|t| t["name"].as_str())
            .ok_or_else(|| format!("mod `{id}`: the crate has no library target"))?;
        mods.push(ResolvedMod {
            lib_name: lib_name.replace('-', "_"),
            patches: ModPatches {
                id: id.clone(),
                dir: Path::new(manifest_path)
                    .parent()
                    .map_or_else(|| PathBuf::from("."), Path::to_path_buf),
                commit: package["metadata"]["ironpumpkin"]["commit"]
                    .as_str()
                    .map(str::to_owned),
            },
        });
    }
    Ok(mods)
}

fn write_mods_rs(root: &Path, mods: &[ResolvedMod]) -> Result<(), String> {
    let mut text = String::from("// Generated by `cargo xtask` from modpack.toml. Do not edit.\n");
    for resolved in mods {
        let _ = writeln!(text, "use {} as _;", resolved.lib_name);
    }
    write(&root.join("bin/src/mods.rs"), &text)
}
