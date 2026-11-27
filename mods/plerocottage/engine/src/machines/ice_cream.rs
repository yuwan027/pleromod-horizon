//! `IceCreamMachineBlockEntity`: one press makes one ice cream from the fluid in its tank, what
//! lies in its two places and a cone.
use super::*;

pub struct IceCream;

fn find<'a>(unit: &Unit, spec: &Spec, recipes: &'a [Recipe]) -> Option<&'a Recipe> {
    let items = &unit.items;
    let container = &items[spec.slot("CONTAINER_SLOT")];
    let right = &items[spec.slot("RIGHT_INPUT_SLOT")];
    shared::first(recipes, |r| {
        let fluid = r.fluid_stack.as_ref();
        let filled = unit.tank.as_ref().is_some_and(|(held, amount)| {
            fluid.is_some_and(|f| *held == f.fluid && *amount >= f.amount)
        });
        let contained = if r.container.is_empty() {
            container.as_ref().is_some_and(|c| shared::is(&c.item, "IceCreamCone"))
        } else {
            shared::holds(container, &r.container)
        };
        filled
            && contained
            && items[spec.slot("OUTPUT_SLOT")].is_none()
            && r.input.first().is_some_and(|p| shared::test(&items[spec.slot("LEFT_INPUT_SLOT")], p))
            && (r.is_tow_flavor != 0) == right.is_some()
            && (right.is_none() || r.input.get(1).is_some_and(|p| shared::test(right, p)))
    })
}

pub fn craft(unit: &mut Unit, env: &mut dyn Env, spec: &Spec) {
    let recipes = env.recipes("ice_cream");
    let Some(recipe) = find(unit, spec, &recipes) else {
        return;
    };
    unit.drain(recipe.fluid_stack.as_ref().map_or(0, |f| f.amount));
    let inputs = spec.slots("INPUT_SLOTS");
    let used = &inputs[..if recipe.is_tow_flavor != 0 { 3 } else { 2 }];
    let left = shared::consume(&mut unit.items, used, 1, None, env);
    let out = output(recipe);
    unit.items[spec.slot("OUTPUT_SLOT")] = Some(ItemStack::new(&out.0, out.1));
    shared::insert(&mut unit.items, inputs, left, env);
    env.sound("ice_cream_machine_done");
}

impl Kind for IceCream {
    fn spec(&self) -> &'static Spec {
        spec::ice_cream_machine()
    }

    fn tick(&self, unit: &mut Unit, env: &mut dyn Env) {
        // The mod pours when the slot is set; here it is looked at each tick.
        fryer::pour(unit, env, self.spec());
    }

    fn packet(&self, unit: &mut Unit, env: &mut dyn Env, packet: &str) {
        if packet == "IceCreamCraftPacket" {
            craft(unit, env, self.spec());
        }
    }
}
