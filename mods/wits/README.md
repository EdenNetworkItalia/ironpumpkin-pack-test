# WITS for IronPumpkin

A native IronPumpkin port of [WITS (What Is This Structure)](https://github.com/TelepathicGrunt/WITS)
1.3.1 by TelepathicGrunt. The mod id is `wits` and the display name is `Wits`, as in the original.

| Command | Permission | Default | Answer |
|---|---|---|---|
| `wits` | `wits:command.wits` | everyone (the original requires level 0) | the structures at the caller's block position; `0 0 0` when the caller is not a player |
| `witsop <dimension> <location>` | `wits:command.witsop` | operators of level 2 (as the original) | the structures at `<location>` in `<dimension>` |

The answer is "There's no structures at your location." (or "at the location."), or
"Structure(s) at your location:" followed by one structure id per line, green in gold square
brackets, copied to the clipboard on click. The server sends the answer to the operators too when
the caller is not a player, as the original does.

## Mapping from the NeoForge mod

| Original | Port |
|---|---|
| `@Mod("wits")` | `impl NativeMod for Wits`, `register_mod!(Wits)` |
| `NeoForge.EVENT_BUS.addListener(RegisterCommandsEvent)` | `ModInit::register_command` in `NativeMod::init` |
| `Commands.hasPermission(LEVEL_ALL)`, `LEVEL_GAMEMASTERS` | `ModInit::register_permission` with `PermissionDefault::Allow` and `PermissionDefault::Op(PermissionLvl::Two)` |
| `DimensionArgument.dimension()`, `DimensionArgument.getDimension` | `DimensionArgument`, `DimensionArgument::get_dimension` |
| `Vec3Argument.vec3()` | `Vec3ArgumentType::Default` |
| `ServerLevel.structureManager().startsForStructure(chunk, s -> true)` and `BoundingBox.isInside` | `World::structure_starts_at` |
| `ComponentUtils.copyOnClickText` | `TextComponent::copy_on_click_text` |

## Unsupported

- Positions in unloaded chunks. `World::structure_starts_at` reads loaded chunks only, so
  `/witsop` answers "no structures" for a position whose chunk is not loaded. After a restart, a
  structure is found only when the chunk that owns its start is loaded too. Vanilla loads both
  chunks.
- Worlds that vanilla opens after IronPumpkin generated them. IronPumpkin saves the piece boxes
  but not the jigsaw piece data, so vanilla treats villages, outposts, bastions, ancient cities,
  trail ruins and trial chambers in those chunks as invalid.
- Structures from data packs. IronPumpkin generates only the vanilla structures, so every id is in
  the `minecraft` namespace.
- Flat worlds. The IronPumpkin flat generator has no structure cache, so the answer is always
  "no structures".

## Licence

The port is distributed under GPL-3.0, like the server it links. The original mod is under the
MIT licence:

```text
MIT License

Copyright (c) 2023 TelepathicGrunt

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
