//! Port of WITS (What Is This Structure) by TelepathicGrunt. `/wits` lists the structures at the
//! caller's position, `/witsop <dimension> <location>` the structures at any position.

use std::borrow::Cow;

use ironpumpkin_mods::{ModInit, NativeMod, register_mod};
use pumpkin_core::{
    command::{
        argument_builder::{ArgumentBuilder, argument, command},
        argument_types::{coordinates::vec3::Vec3ArgumentType, resource_key::ResourceKeyArgument},
        context::command_context::CommandContext,
        errors::error_types::CommandErrorType,
        node::{CommandExecutor, CommandExecutorResult},
    },
    world::World,
};
use pumpkin_data::translation::java::{CHAT_COORDINATES, CHAT_COPY_CLICK};
use pumpkin_macros::translate_cross;
use pumpkin_util::{
    PermissionLvl,
    identifier::Identifier,
    math::position::BlockPos,
    permission::{Permission, PermissionDefault},
    text::{TextComponent, click::ClickEvent, color::NamedColor, hover::HoverEvent},
};

const ID: &str = "wits";
const WITS_DESCRIPTION: &str = "Lists the structures at your location.";
const WITSOP_DESCRIPTION: &str = "Lists the structures at a location in a dimension.";

static DIMENSION_REGISTRY: &Identifier = &Identifier::vanilla_static("dimension");
static INVALID_DIMENSION: CommandErrorType<1> =
    CommandErrorType::new("argument.dimension.invalid", "argument.dimension.invalid");

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
                argument("dimension", ResourceKeyArgument(DIMENSION_REGISTRY))
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
        let dimension = ResourceKeyArgument::get_registry_key(
            context,
            "dimension",
            DIMENSION_REGISTRY,
            &INVALID_DIMENSION,
        )?
        .identifier
        .to_string();
        let worlds = context.source.server().worlds.load();
        let world = worlds
            .iter()
            .find(|world| world.dimension.minecraft_name == dimension)
            .ok_or_else(|| {
                INVALID_DIMENSION.create_without_context(TextComponent::text(dimension.clone()))
            })?;
        let pos = BlockPos::floored_v(Vec3ArgumentType::get_vector3(context, "location")?);
        list_structures_at(context, world, pos, false);
        Ok(1)
    }
}

/// The structures whose start bounding box contains `pos`, as `StructureManager.startsForStructure`
/// and `BoundingBox.isInside` select them in the original.
fn structures_at(world: &World, pos: BlockPos) -> Vec<&'static str> {
    let world_gen = world.level.world_gen.load_full();
    let Some(cache) = world_gen.global_structure_cache() else {
        return Vec::new();
    };
    let mut names: Vec<&'static str> = cache
        .structure_starts()
        .into_iter()
        .filter(|(_, start)| start.get_bounding_box().contains_pos(&pos.0))
        .map(|(structure, _)| structure.to_name())
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
            .add_child(copy_on_click(key).color_named(NamedColor::Gold));
    }
    context.source.send_feedback(message, broadcast);
}

/// `ComponentUtils.copyOnClickText`: the text, copied to the clipboard on click.
fn copy_on_click(text: String) -> TextComponent {
    TextComponent::text(text.clone())
        .click_event(ClickEvent::CopyToClipboard {
            value: Cow::Owned(text.clone()),
        })
        .hover_event(HoverEvent::show_text(translate_cross!(
            CHAT_COPY_CLICK,
            CHAT_COPY_CLICK
        )))
        .insertion(text)
}
