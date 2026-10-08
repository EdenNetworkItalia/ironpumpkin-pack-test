//! Port of WITS (What Is This Structure) by TelepathicGrunt. `/wits` lists the structures at the
//! caller's position, `/witsop <dimension> <location>` the structures at any position.

use ironpumpkin_mods::{
    ModInit, NativeMod,
    command::{
        argument_builder::{ArgumentBuilder, argument, command},
        argument_types::{coordinates::vec3::Vec3ArgumentType, dimension::DimensionArgument},
        context::command_context::CommandContext,
        node::{CommandExecutor, CommandExecutorResult},
    },
    math::position::BlockPos,
    permission::{Permission, PermissionDefault, PermissionLvl},
    pumpkin_data::translation::java::CHAT_COORDINATES,
    register_mod,
    text::{TextComponent, color::NamedColor, translate_cross},
    world::World,
};

const ID: &str = "wits";
const WITS_DESCRIPTION: &str = "Lists the structures at your location.";
const WITSOP_DESCRIPTION: &str = "Lists the structures at a location in a dimension.";

struct Wits;

impl NativeMod for Wits {
    fn id(&self) -> &'static str {
        ID
    }

    fn display_name(&self) -> &'static str {
        "Wits"
    }

    fn version(&self) -> &'static str {
        env!("CARGO_PKG_VERSION")
    }

    fn init(&self, cx: &mut ModInit) {
        cx.register_permission(Permission::new(
            &format!("{ID}:command.wits"),
            WITS_DESCRIPTION,
            PermissionDefault::Allow,
        ));
        cx.register_permission(Permission::new(
            &format!("{ID}:command.witsop"),
            WITSOP_DESCRIPTION,
            PermissionDefault::Op(PermissionLvl::Two),
        ));
        cx.register_command(
            command("wits", WITS_DESCRIPTION).executes(WitsExecutor),
            "command.wits",
        );
        cx.register_command(
            command("witsop", WITSOP_DESCRIPTION).then(
                argument("dimension", DimensionArgument)
                    .then(argument("location", Vec3ArgumentType::Default).executes(WitsOpExecutor)),
            ),
            "command.witsop",
        );
    }
}

register_mod!(Wits);

struct WitsExecutor;

impl CommandExecutor for WitsExecutor {
    fn execute(&self, context: &CommandContext) -> CommandExecutorResult {
        // Like the original: a non-player caller is answered for 0 0 0, not for its own position.
        let pos = context
            .source
            .player_or_none()
            .map_or(BlockPos::new(0, 0, 0), |player| {
                BlockPos::floored_v(player.position())
            });
        list_structures_at(context, context.source.world(), pos, true);
        Ok(1)
    }
}

struct WitsOpExecutor;

impl CommandExecutor for WitsOpExecutor {
    fn execute(&self, context: &CommandContext) -> CommandExecutorResult {
        let world = DimensionArgument::get_dimension(context, "dimension")?;
        let pos = BlockPos::floored_v(Vec3ArgumentType::get_vector3(context, "location")?);
        list_structures_at(context, &world, pos, false);
        Ok(1)
    }
}

/// The structures whose start bounding box contains `pos`, as `StructureManager.startsForStructure`
/// and `BoundingBox.isInside` select them in the original.
fn structures_at(world: &World, pos: BlockPos) -> Vec<&'static str> {
    let mut names: Vec<&'static str> = world
        .structure_starts_at(&pos)
        .into_iter()
        .map(|start| start.structure.to_name())
        .collect();
    names.sort_unstable();
    names
}

fn list_structures_at(
    context: &CommandContext,
    world: &World,
    pos: BlockPos,
    caller_position: bool,
) {
    let structures = structures_at(world, pos);
    let broadcast = !context.source.executed_by_player();
    if structures.is_empty() {
        let text = if caller_position {
            "There's no structures at your location."
        } else {
            "There's no structures at the location."
        };
        context
            .source
            .send_feedback(TextComponent::text(text), broadcast);
        return;
    }

    let mut message = if caller_position {
        TextComponent::text("Structure(s) at your location:")
    } else {
        TextComponent::text("Structure(s) at ")
            .add_child(translate_cross!(
                CHAT_COORDINATES,
                CHAT_COORDINATES,
                TextComponent::text(pos.0.x.to_string()),
                TextComponent::text(pos.0.y.to_string()),
                TextComponent::text(pos.0.z.to_string())
            ))
            .add_child(TextComponent::text(":"))
    };
    for name in structures {
        let key = format!("minecraft:{name}");
        message = message
            .add_child(TextComponent::text("\n -").color_named(NamedColor::White))
            .add_child(TextComponent::copy_on_click_text(key).color_named(NamedColor::Gold));
    }
    context.source.send_feedback(message, broadcast);
}
