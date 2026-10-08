//! The `modpack.toml` schema and its validation.

use std::collections::BTreeMap;

use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawModpack {
    pack: RawPack,
    ironpumpkin: RawIronPumpkin,
    #[serde(default)]
    mods: BTreeMap<String, RawMod>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawPack {
    name: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
struct RawIronPumpkin {
    git: String,
    commit: String,
    #[serde(default)]
    allow_drift: bool,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawMod {
    path: Option<String>,
    git: Option<String>,
    rev: Option<String>,
    version: Option<String>,
}

#[derive(Debug, PartialEq, Eq)]
pub struct Modpack {
    pub name: String,
    pub git: String,
    pub commit: String,
    pub allow_drift: bool,
    /// Keyed by mod id, which is also the package name of the mod crate. The map order is the
    /// mod-id order in which patches apply.
    pub mods: BTreeMap<String, ModSource>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum ModSource {
    /// Relative to the blueprint root, or absolute.
    Path(String),
    Git {
        url: String,
        rev: String,
    },
    Version(String),
}

pub fn parse(text: &str) -> Result<Modpack, String> {
    let raw: RawModpack = toml::from_str(text).map_err(|e| format!("modpack.toml: {e}"))?;
    check_name("pack name", &raw.pack.name)?;
    if raw.pack.name == "xtask" {
        return Err("modpack.toml: the pack name `xtask` would overwrite the build tool".into());
    }
    if raw.ironpumpkin.git.trim().is_empty() {
        return Err("modpack.toml: `ironpumpkin.git` is empty".into());
    }
    if !is_full_commit(&raw.ironpumpkin.commit) {
        return Err(format!(
            "modpack.toml: `ironpumpkin.commit` must be a full 40-character commit hash, got `{}`",
            raw.ironpumpkin.commit
        ));
    }
    let mut mods = BTreeMap::new();
    for (id, raw_mod) in raw.mods {
        check_name("mod id", &id)?;
        if id == raw.pack.name || id == "pumpkin" || id == "xtask" {
            return Err(format!(
                "modpack.toml: mod id `{id}` clashes with the pack binary or the server crate"
            ));
        }
        let source = match raw_mod {
            RawMod {
                path: Some(path),
                git: None,
                rev: None,
                version: None,
            } => ModSource::Path(path),
            RawMod {
                path: None,
                git: Some(url),
                rev: Some(rev),
                version: None,
            } => ModSource::Git { url, rev },
            RawMod {
                path: None,
                git: None,
                rev: None,
                version: Some(version),
            } => ModSource::Version(version),
            _ => {
                return Err(format!(
                    "modpack.toml: mod `{id}` needs exactly one source: `path`, `git` with `rev`, \
                     or `version`"
                ));
            }
        };
        mods.insert(id, source);
    }
    Ok(Modpack {
        name: raw.pack.name,
        git: raw.ironpumpkin.git,
        commit: raw.ironpumpkin.commit,
        allow_drift: raw.ironpumpkin.allow_drift,
        mods,
    })
}

pub fn is_full_commit(commit: &str) -> bool {
    commit.len() == 40
        && commit
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

fn check_name(what: &str, name: &str) -> Result<(), String> {
    let valid = name.starts_with(|c: char| c.is_ascii_lowercase())
        && name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_');
    if valid {
        Ok(())
    } else {
        Err(format!(
            "modpack.toml: {what} `{name}` must start with a lowercase letter and contain only \
             lowercase letters, digits, `-` and `_`"
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const HEAD: &str = r#"
[pack]
name = "my-pack"

[ironpumpkin]
git = "https://example.invalid/IronPumpkin"
commit = "64661ef1796a891ce586bd163097e3d49de0ed4a"
"#;

    #[test]
    fn parses_every_source_kind() {
        let text = format!(
            "{HEAD}
[mods.b-mod]
git = \"https://example.invalid/b\"
rev = \"v1\"

[mods.a-mod]
path = \"mods/a\"

[mods.c-mod]
version = \"0.3\"
"
        );
        let pack = parse(&text).unwrap();
        assert_eq!(pack.name, "my-pack");
        assert!(!pack.allow_drift);
        let ids: Vec<&str> = pack.mods.keys().map(String::as_str).collect();
        assert_eq!(ids, ["a-mod", "b-mod", "c-mod"]);
        assert_eq!(pack.mods["a-mod"], ModSource::Path("mods/a".into()));
        assert_eq!(
            pack.mods["b-mod"],
            ModSource::Git {
                url: "https://example.invalid/b".into(),
                rev: "v1".into()
            }
        );
        assert_eq!(pack.mods["c-mod"], ModSource::Version("0.3".into()));
    }

    #[test]
    fn rejects_ambiguous_or_unpinned_sources() {
        for body in [
            "path = \"a\"\nversion = \"1\"",
            "git = \"https://example.invalid/a\"",
            "rev = \"abc\"",
            "",
        ] {
            let text = format!("{HEAD}\n[mods.a-mod]\n{body}\n");
            assert!(parse(&text).is_err(), "accepted: {body}");
        }
    }

    #[test]
    fn rejects_short_commit_and_bad_names() {
        let short = HEAD.replace("64661ef1796a891ce586bd163097e3d49de0ed4a", "64661ef17");
        assert!(parse(&short).is_err());
        let bad_pack = HEAD.replace("my-pack", "My Pack");
        assert!(parse(&bad_pack).is_err());
        assert!(parse(&HEAD.replace("my-pack", "xtask")).is_err());
        let clash = format!("{HEAD}\n[mods.my-pack]\npath = \"a\"\n");
        assert!(parse(&clash).is_err());
        let unknown = format!("{HEAD}\n[mods.a-mod]\npath = \"a\"\nbranch = \"main\"\n");
        assert!(parse(&unknown).is_err());
    }
}
