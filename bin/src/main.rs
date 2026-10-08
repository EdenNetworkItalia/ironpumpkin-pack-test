// `cargo xtask` generates `mods.rs` from modpack.toml with one `use <mod> as _;` per mod. A mod
// crate that the binary does not reference is not linked, and its mod does not load.
mod mods;

fn main() {
    pumpkin::run();
}
