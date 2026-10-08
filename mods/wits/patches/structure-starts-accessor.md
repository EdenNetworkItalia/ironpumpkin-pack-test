# structure-starts-accessor.patch

What it does: adds `GlobalStructureCache::structure_starts()` to
`crates/pumpkin-world/src/generation/structure/placement.rs`. The method returns a copy of every
structure start that the world generator has computed so far, with its structure key.

Why a NeoForge event or API is not enough: WITS reads the structure starts of a chunk with
vanilla `StructureManager.startsForStructure`, a plain vanilla call with no event around it.
IronPumpkin has no structure manager. It keeps the structure starts only in the private
`structure_starts` field of `GlobalStructureCache`. The patch is a read accessor on that field:
the compile-time equivalent of an access transformer. It changes no behaviour of the server.

Commit binding: the patch is written for IronPumpkin commit
`b83ce68e593d7e8e13ee7dbf3ccdf29271d493d3`, as `[package.metadata.ironpumpkin] commit` in
`Cargo.toml` declares. Regenerate it for any other commit.

Candidate API: a structure manager on the world (`startsForStructure`, `getStructureAt`) in the
NeoForge-shaped API would remove this patch.
