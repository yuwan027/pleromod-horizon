//! The shaker in the game (`ShakerItem`): what it holds is its `minecraft:container`, a right click in the
//! inventory fills and empties it, and using it in the main hand shakes it for as long as the item says, after
//! which the recipe's result is in the other hand. What happens to the stacks is `machines::shaker`'s.
use crate::glue::HostEnv;
use crate::machines::shaker as logic;
use crate::spec;
use horizon_client::mod_api::*;

/// `ClickType.PICKUP` with the secondary button (`ClickAction.SECONDARY`).
const PICKUP: i32 = 0;
const SECONDARY: i32 = 1;
/// The off hand, as `Inventory` saves it and as `NativeInventoryKind::Player` numbers it.
const OFF_HAND_SAVED: i32 = -106;
const OFF_HAND: usize = 40;
/// `SoundSource.PLAYERS`.
const PLAYERS: usize = 7;

pub fn is(stack: &ItemStack) -> bool {
    *stack.item == *spec::shaker().item
}

fn size() -> usize {
    spec::shaker().constants["ITEMS_SIZE"] as usize
}

/// `ShakerItem.contents`: its places, the empty ones too.
pub fn contents(stack: &ItemStack) -> Vec<Option<ItemStack>> {
    let mut slots = vec![None; size()];
    for (slot, held) in stack.container() {
        if let Some(place) = slots.get_mut(usize::from(*slot)) {
            *place = Some(held.clone()).filter(|s| s.count > 0);
        }
    }
    slots
}

/// The shaker holding `contents` (`ShakerContents.onContentsChanged`).
pub fn with_contents(stack: &ItemStack, contents: &[Option<ItemStack>]) -> ItemStack {
    let mut changed = stack.clone();
    let data = changed.data_mut();
    data.container = contents
        .iter()
        .enumerate()
        .filter_map(|(slot, held)| Some((slot as u8, held.clone().filter(|s| s.count > 0)?)))
        .collect();
    // What a server sent of the stack no longer describes it.
    data.wire = Default::default();
    changed.tidy();
    changed
}

/// The slot of the player's inventory a slot of their own inventory screen is (`InventoryMenu`): the main
/// inventory, the hotbar and the off hand. None for the crafting grid and the armour, which a shaker leaves alone.
fn inventory_slot(menu_slot: i32) -> Option<usize> {
    match menu_slot {
        9..=35 => Some(menu_slot as usize),
        36..=44 => Some(menu_slot as usize - 36),
        45 => Some(OFF_HAND),
        _ => None,
    }
}

/// `Item.overrideStackedOnOther` and `overrideOtherStackedOnMe`: a right click in the player's own inventory with
/// the shaker on the cursor or under it. True when the shaker took the click.
pub fn clicked(host: &mut dyn Host, event: &InventoryClickEvent<'_>) -> bool {
    if event.window != 0 || event.mode != PICKUP || event.button != SECONDARY {
        return false;
    }
    let Some(slot) = inventory_slot(event.slot) else {
        return false;
    };
    let carried = host.menu_cursor(event.player);
    let under = if slot == OFF_HAND {
        host.player_equipment(event.player, EquipmentSlot::OffHand)
    } else {
        let Some(stacks) = host.player_stacks(event.player) else {
            return false;
        };
        stacks.get(slot).cloned().flatten()
    };
    let limit = logic::limit(spec::shaker());
    let (mut new_carried, mut new_under) = (carried.clone(), under.clone());
    let took = {
        let env = &mut HostEnv::new(host, [0; 3], None);
        match (&carried, &under) {
            // The shaker on the cursor, over a slot that holds something it may take.
            (Some(shaker), Some(other)) if is(shaker) && !is(other) => {
                let mut held = contents(shaker);
                let took = logic::onto_slot(&mut held, &mut new_under, limit, env);
                new_carried = Some(with_contents(shaker, &held));
                took
            }
            // The shaker in the slot: the cursor's goes in, or its last thing comes out.
            (other, Some(shaker)) if is(shaker) && other.as_ref().is_none_or(|o| !is(o)) => {
                let mut held = contents(shaker);
                let took = logic::onto_shaker(&mut held, &mut new_carried, limit, env);
                new_under = Some(with_contents(shaker, &held));
                took
            }
            _ => false,
        }
    };
    if !took {
        return false;
    }
    // The server's players are out of reach while the click is heard: the click is refused, and what it does
    // is done in the level's next tick (`apply`), whose sync shows it.
    if let Ok(mut pending) = horizon_os::sync::lock(&PENDING) {
        pending.push(Pending { player: event.player, slot, under, new_under, carried, new_carried });
    }
    true
}

/// A click the shaker took, to be done with the players in reach.
struct Pending {
    player: u128,
    slot: usize,
    under: Option<ItemStack>,
    new_under: Option<ItemStack>,
    carried: Option<ItemStack>,
    new_carried: Option<ItemStack>,
}

static PENDING: std::sync::Mutex<Vec<Pending>> = std::sync::Mutex::new(Vec::new());

/// The clicks the shaker took (`clicked`): each slot and cursor as it says, when they still hold what they held.
pub fn apply(host: &mut dyn Host) {
    let taken = horizon_os::sync::lock(&PENDING).map(|mut pending| std::mem::take(&mut *pending)).unwrap_or_default();
    for click in taken {
        let player = click.player;
        if host.set_inventory_slot(player, NativeInventoryKind::Player, click.slot, click.under.as_ref(), click.new_under).is_ok() {
            let _ = host.set_inventory_slot(player, NativeInventoryKind::Cursor, 0, click.carried.as_ref(), click.new_carried);
        }
    }
}

/// The recipes of the shaker as the world has them.
fn recipes(env: &mut HostEnv<'_>) -> std::rc::Rc<[spec::Recipe2]> {
    use crate::shared::Env;
    env.recipes("shaker")
}

/// `ShakerItem.use`: only the main hand shakes, and only what makes something.
pub fn may_start(host: &mut dyn Host, stack: &ItemStack, main: bool) -> bool {
    if !main {
        return false;
    }
    // The window's client predicts the use without the server's copy of what the shaker holds: the server says
    // whether the shake starts, as it does for a client of a server.
    if host.acting_player().is_none() {
        return true;
    }
    let offhand = host.carried_stack(OFF_HAND_SAVED).map(|s| s.clone());
    let env = &mut HostEnv::new(host, [0; 3], None);
    let (known, held) = (recipes(env), contents(stack));
    known.iter().any(|recipe| logic::matches(recipe, &held, &offhand, env))
}

/// `ShakerItem.finishUsingItem`: the shake is over. The shaker as it is afterwards, when something was made.
pub fn finish(host: &mut dyn Host, stack: &ItemStack) -> Option<ItemStack> {
    let player = host.acting_player()?;
    let offhand = host.carried_stack(OFF_HAND_SAVED).map(|s| s.clone());
    let shaken = {
        let env = &mut HostEnv::new(host, [0; 3], None);
        let known = recipes(env);
        logic::craft(&known, &contents(stack), &offhand, spec::shaker(), env)?
    };
    if shaken.offhand != offhand
        && let (Some(made), Some(held)) = (&shaken.offhand, host.carried_stack(OFF_HAND_SAVED))
    {
        // `player.setItemInHand(InteractionHand.OFF_HAND, output)`.
        *held = made.clone();
    }
    for stack in shaken.given {
        // `giveOrDrop`: into the inventory, and what has no room there at the player's feet.
        if let Some(left) = host.give_stack(player.uuid, stack) {
            let mut nbt = Compound::new();
            nbt.insert(crate::JavaString::from("Item"), crate::glue::stack_tag(&left));
            let dimension = host.dimension();
            host.summon(dimension, "minecraft:item", player.position, nbt);
        }
    }
    // `level.playSound(null, player.blockPosition(), SoundEvents.BREWING_STAND_BREW, SoundSource.PLAYERS)`.
    let at = player.position.map(f64::floor).map(|v| v + 0.5);
    host.play_sound("minecraft:block.brewing_stand.brew", at, PLAYERS, 1.0, 1.0);
    Some(with_contents(stack, &shaken.contents))
}

/// `Item.getUseAnimation`, as the extraction read it.
pub fn animation() -> Option<UseAnimation> {
    Some(match spec::shaker().use_animation.as_str() {
        "drink" => UseAnimation::Drink,
        "eat" => UseAnimation::Eat,
        "block" => UseAnimation::Block,
        "bow" => UseAnimation::Bow,
        _ => return None,
    })
}
