//! Applies the source patches of the mods to the `IronPumpkin` checkout.
//!
//! A mod crate ships `patches/<name>.patch` against the `IronPumpkin` source tree, a justification
//! `patches/<name>.md` next to each patch, and declares the `IronPumpkin` commit the patches are
//! written for in `[package.metadata.ironpumpkin] commit`. Patches apply in mod-id order, then in
//! file-name order within a mod, with `git apply` and no fuzz, no whitespace leniency and no
//! three-way merge.

use crate::modpack::is_full_commit;
use std::{
    collections::BTreeMap,
    fmt::Write as _,
    fs,
    path::{Path, PathBuf},
    process::Command,
};

/// The patch inputs of one mod.
pub struct ModPatches {
    pub id: String,
    /// The mod crate directory.
    pub dir: PathBuf,
    /// `[package.metadata.ironpumpkin] commit` of the mod crate.
    pub commit: Option<String>,
}

const GENERATED_DIR: &str = "crates/pumpkin-data/src/generated/";

/// Applies the patches of `mods` in mod-id order and returns how many applied.
pub fn apply_all(
    checkout: &Path,
    pack_commit: &str,
    allow_drift: bool,
    mods: &[ModPatches],
) -> Result<usize, String> {
    let mut ordered: Vec<&ModPatches> = mods.iter().collect();
    ordered.sort_by(|a, b| a.id.cmp(&b.id));
    let mut touched_by: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut applied = 0;
    for patched_mod in ordered {
        let id = &patched_mod.id;
        for patch in list_patches(&patched_mod.dir)? {
            let name = format!(
                "patches/{}",
                patch.file_name().unwrap_or_default().to_string_lossy()
            );
            let Some(commit) = &patched_mod.commit else {
                return Err(format!(
                    "mod `{id}` ships `{name}` but its Cargo.toml has no \
                     `[package.metadata.ironpumpkin] commit`"
                ));
            };
            if !is_full_commit(commit) {
                return Err(format!(
                    "mod `{id}`: `[package.metadata.ironpumpkin] commit` must be a full \
                     40-character commit hash, got `{commit}`"
                ));
            }
            if commit != pack_commit {
                let message = format!(
                    "mod `{id}`: `{name}` is written for IronPumpkin commit {commit}, the pack \
                     pins commit {pack_commit}"
                );
                if !allow_drift {
                    return Err(format!(
                        "{message}; update the mod or set `allow-drift = true` in modpack.toml"
                    ));
                }
                eprintln!("warning: {message}; applying it because `allow-drift` is set");
            }
            if !patch.with_extension("md").is_file() {
                return Err(format!(
                    "mod `{id}`: `{name}` has no justification file `{}` next to it",
                    name.replace(".patch", ".md")
                ));
            }
            let files = touched_files(checkout, &patch)?;
            if let Some(file) = files.iter().find(|file| is_forbidden(file)) {
                return Err(format!(
                    "mod `{id}`: `{name}` touches `{file}`; patches must not change generated \
                     files under `{GENERATED_DIR}` or a `Cargo.lock`"
                ));
            }
            if let Err(stderr) = git_apply(checkout, &patch) {
                return Err(conflict_message(
                    id,
                    &name,
                    pack_commit,
                    &files,
                    &stderr,
                    &touched_by,
                ));
            }
            for file in files {
                touched_by.entry(file).or_default().push(id.clone());
            }
            applied += 1;
        }
    }
    Ok(applied)
}

fn list_patches(mod_dir: &Path) -> Result<Vec<PathBuf>, String> {
    let dir = mod_dir.join("patches");
    if !dir.is_dir() {
        return Ok(Vec::new());
    }
    let entries = fs::read_dir(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let mut patches = Vec::new();
    for entry in entries {
        let path = entry.map_err(|e| format!("{}: {e}", dir.display()))?.path();
        if path.extension().is_some_and(|ext| ext == "patch") {
            patches.push(path);
        }
    }
    patches.sort();
    Ok(patches)
}

fn is_forbidden(file: &str) -> bool {
    // Windows file systems ignore case, so `Cargo.LOCK` is the lock file there.
    let file = file.to_ascii_lowercase();
    file.starts_with(GENERATED_DIR) || file == "cargo.lock" || file.ends_with("/cargo.lock")
}

/// The paths a patch touches, as `git apply --numstat -z` reports them.
fn touched_files(checkout: &Path, patch: &Path) -> Result<Vec<String>, String> {
    let output = Command::new("git")
        .args(["apply", "--numstat", "-z"])
        .arg(patch)
        .current_dir(checkout)
        .env("LC_ALL", "C")
        .output()
        .map_err(|e| format!("cannot run git: {e}"))?;
    if !output.status.success() {
        return Err(format!(
            "`{}` is not a valid patch: {}",
            patch.display(),
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut fields = stdout.split('\0');
    let mut files = Vec::new();
    // Each record is `added\tdeleted\tpath`, or `added\tdeleted\t` followed by the old and the new
    // path as two more fields for a rename.
    while let Some(record) = fields.next() {
        let Some(file) = record.splitn(3, '\t').nth(2) else {
            continue;
        };
        if file.is_empty() {
            files.extend(fields.next().map(str::to_owned));
            files.extend(fields.next().map(str::to_owned));
        } else {
            files.push(file.to_owned());
        }
    }
    Ok(files)
}

fn git_apply(checkout: &Path, patch: &Path) -> Result<(), String> {
    let output = Command::new("git")
        .arg("apply")
        .arg(patch)
        .current_dir(checkout)
        // `conflict_message` parses the English messages of `git apply`.
        .env("LC_ALL", "C")
        .output()
        .map_err(|e| format!("cannot run git: {e}"))?;
    if output.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).trim().to_owned())
    }
}

fn conflict_message(
    id: &str,
    name: &str,
    pack_commit: &str,
    files: &[String],
    stderr: &str,
    touched_by: &BTreeMap<String, Vec<String>>,
) -> String {
    // `git apply` names the file in lines such as `error: patch failed: <file>:<line>` and
    // `error: <file>: patch does not apply`.
    let mut failed: Vec<&String> = files
        .iter()
        .filter(|file| {
            stderr.lines().any(|line| {
                line.strip_prefix("error: ").is_some_and(|rest| {
                    let rest = rest.strip_prefix("patch failed: ").unwrap_or(rest);
                    rest.strip_prefix(file.as_str())
                        .is_some_and(|tail| tail.starts_with(':'))
                })
            })
        })
        .collect();
    if failed.is_empty() {
        failed = files.iter().collect();
    }
    let mut message = String::new();
    for file in failed {
        let _ = write!(
            message,
            "mod `{id}`: `{name}` does not apply to `{file}` at IronPumpkin commit {pack_commit}"
        );
        if let Some(earlier) = touched_by.get(file) {
            let mut earlier = earlier.clone();
            earlier.dedup();
            let names: Vec<String> = earlier.iter().map(|m| format!("`{m}`")).collect();
            let _ = write!(
                message,
                "; mod {} patches the same file and applies first (mod-id order)",
                names.join(", ")
            );
        }
        message.push('\n');
    }
    message.push_str("git apply: ");
    message.push_str(stderr);
    message
}

#[cfg(test)]
mod tests {
    use super::*;

    const COMMIT: &str = "64661ef1796a891ce586bd163097e3d49de0ed4a";
    const OTHER: &str = "f1c0871f4aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

    struct Scratch(PathBuf);

    impl Scratch {
        fn new(name: &str) -> Self {
            let dir =
                std::env::temp_dir().join(format!("xtask-test-{}-{name}", std::process::id()));
            let _ = fs::remove_dir_all(&dir);
            fs::create_dir_all(dir.join("checkout/src")).unwrap();
            fs::write(dir.join("checkout/src/lib.rs"), "one\ntwo\nthree\n").unwrap();
            let status = Command::new("git")
                .args(["init", "-q"])
                .current_dir(dir.join("checkout"))
                .status()
                .unwrap();
            assert!(status.success());
            Self(dir)
        }

        fn checkout(&self) -> PathBuf {
            self.0.join("checkout")
        }

        fn add_mod(&self, id: &str, commit: &str, patches: &[(&str, &str, bool)]) -> ModPatches {
            let dir = self.0.join(id);
            fs::create_dir_all(dir.join("patches")).unwrap();
            for (name, body, justified) in patches {
                fs::write(dir.join(format!("patches/{name}.patch")), body).unwrap();
                if *justified {
                    fs::write(dir.join(format!("patches/{name}.md")), "why").unwrap();
                }
            }
            ModPatches {
                id: id.into(),
                dir,
                commit: Some(commit.into()),
            }
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn line_two_patch(path: &str, replacement: &str) -> String {
        format!(
            "diff --git a/{path} b/{path}\n--- a/{path}\n+++ b/{path}\n@@ -1,3 +1,3 @@\n one\n-two\n+{replacement}\n three\n"
        )
    }

    #[test]
    fn applies_a_valid_patch() {
        let scratch = Scratch::new("valid");
        let patched = scratch.add_mod(
            "a-mod",
            COMMIT,
            &[("edit", &line_two_patch("src/lib.rs", "TWO"), true)],
        );
        let applied = apply_all(&scratch.checkout(), COMMIT, false, &[patched]).unwrap();
        assert_eq!(applied, 1);
        let text = fs::read_to_string(scratch.checkout().join("src/lib.rs")).unwrap();
        assert_eq!(text, "one\nTWO\nthree\n");
    }

    #[test]
    fn conflicting_hunks_name_both_mods_and_the_file() {
        let scratch = Scratch::new("conflict");
        let first = scratch.add_mod(
            "a-mod",
            COMMIT,
            &[("edit", &line_two_patch("src/lib.rs", "A"), true)],
        );
        let second = scratch.add_mod(
            "b-mod",
            COMMIT,
            &[("edit", &line_two_patch("src/lib.rs", "B"), true)],
        );
        // Mod-id order, not the order of the slice.
        let error = apply_all(&scratch.checkout(), COMMIT, false, &[second, first]).unwrap_err();
        assert!(error.contains("mod `b-mod`"), "{error}");
        assert!(
            error.contains("mod `a-mod` patches the same file"),
            "{error}"
        );
        assert!(error.contains("`src/lib.rs`"), "{error}");
    }

    #[test]
    fn drift_fails_unless_allowed() {
        let scratch = Scratch::new("drift");
        let drifted = scratch.add_mod(
            "a-mod",
            OTHER,
            &[("edit", &line_two_patch("src/lib.rs", "TWO"), true)],
        );
        let error = apply_all(
            &scratch.checkout(),
            COMMIT,
            false,
            std::slice::from_ref(&drifted),
        )
        .unwrap_err();
        for part in ["`a-mod`", "`patches/edit.patch`", COMMIT, OTHER] {
            assert!(error.contains(part), "{part} missing: {error}");
        }
        assert_eq!(
            apply_all(&scratch.checkout(), COMMIT, true, &[drifted]).unwrap(),
            1
        );
    }

    #[test]
    fn rejects_generated_files_lock_files_and_missing_justification() {
        let scratch = Scratch::new("rejected");
        for (id, path, justified) in [
            (
                "gen-mod",
                "crates/pumpkin-data/src/generated/block.rs",
                true,
            ),
            ("lock-mod", "Cargo.lock", true),
            ("nested-lock-mod", "examples/modpack/Cargo.lock", true),
            ("upper-lock-mod", "Cargo.LOCK", true),
            (
                "upper-gen-mod",
                "Crates/Pumpkin-Data/src/generated/block.rs",
                true,
            ),
            ("unjustified-mod", "src/lib.rs", false),
        ] {
            let patched = scratch.add_mod(
                id,
                COMMIT,
                &[("edit", &line_two_patch(path, "TWO"), justified)],
            );
            let error = apply_all(&scratch.checkout(), COMMIT, false, &[patched]).unwrap_err();
            assert!(error.contains(&format!("`{id}`")), "{error}");
        }
    }

    #[test]
    fn a_short_mod_commit_is_invalid_not_drift() {
        let scratch = Scratch::new("short");
        let short = scratch.add_mod(
            "a-mod",
            &COMMIT[..9],
            &[("edit", &line_two_patch("src/lib.rs", "TWO"), true)],
        );
        let error = apply_all(&scratch.checkout(), COMMIT, true, &[short]).unwrap_err();
        assert!(error.contains("full 40-character"), "{error}");
    }

    #[test]
    fn a_mod_without_patches_needs_no_metadata() {
        let scratch = Scratch::new("plain");
        let plain = ModPatches {
            id: "plain-mod".into(),
            dir: scratch.0.join("plain-mod"),
            commit: None,
        };
        assert_eq!(
            apply_all(&scratch.checkout(), COMMIT, false, &[plain]).unwrap(),
            0
        );
    }
}
