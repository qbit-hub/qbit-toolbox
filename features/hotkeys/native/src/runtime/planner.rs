use crate::domain::{Modifier, ModifierFamily};

use super::{ConcreteModifier, ModifierState};

const PLAN_CAPACITY: usize = 34;
const FAMILIES: [ModifierFamily; 4] = [
    ModifierFamily::Ctrl,
    ModifierFamily::Shift,
    ModifierFamily::Alt,
    ModifierFamily::Meta,
];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PlanStep {
    ReleaseModifier(ConcreteModifier),
    PressModifier(ConcreteModifier),
    PrimaryDown,
    PrimaryUp,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PlannerError {
    DuplicateTargetModifier(Modifier),
    AmbiguousTargetModifierFamily(ModifierFamily),
    PlanCapacityExceeded,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct KeystrokePlan {
    steps: [Option<PlanStep>; PLAN_CAPACITY],
    len: usize,
}

impl KeystrokePlan {
    pub fn steps(&self) -> impl Iterator<Item = PlanStep> + '_ {
        self.steps[..self.len].iter().copied().flatten()
    }

    fn push(&mut self, step: PlanStep) -> Result<(), PlannerError> {
        let Some(slot) = self.steps.get_mut(self.len) else {
            return Err(PlannerError::PlanCapacityExceeded);
        };
        *slot = Some(step);
        self.len += 1;
        Ok(())
    }
}

/// Produces a platform-neutral, allocation-free modifier transition plan.
pub fn plan_keystroke(
    physical: ModifierState,
    target: &[Modifier],
) -> Result<KeystrokePlan, PlannerError> {
    let mut wanted = ModifierState::empty();
    let mut generic_families = [false; FAMILIES.len()];

    for &modifier in target {
        if let Some(concrete) = ConcreteModifier::from_domain_modifier(modifier) {
            if wanted.contains(concrete) {
                return Err(PlannerError::DuplicateTargetModifier(modifier));
            }
            let family_index = family_index(concrete.family());
            if generic_families[family_index] {
                return Err(PlannerError::AmbiguousTargetModifierFamily(
                    concrete.family(),
                ));
            }
            wanted.press(concrete);
        } else {
            let family = generic_modifier_family(modifier);
            let family_index = family_index(family);
            if generic_families[family_index] {
                return Err(PlannerError::DuplicateTargetModifier(modifier));
            }
            if wanted.contains_family(family) {
                return Err(PlannerError::AmbiguousTargetModifierFamily(family));
            }
            generic_families[family_index] = true;
        }
    }

    for family in FAMILIES {
        if generic_families[family_index(family)] {
            if physical.contains_family(family) {
                for modifier in ConcreteModifier::ORDER {
                    if modifier.family() == family && physical.contains(modifier) {
                        wanted.press(modifier);
                    }
                }
            } else {
                wanted.press(left_modifier(family));
            }
        }
    }

    let mut plan = KeystrokePlan {
        steps: [None; PLAN_CAPACITY],
        len: 0,
    };
    let mut temporarily_released = ModifierState::empty();
    let mut synthetically_pressed = ModifierState::empty();

    for modifier in ConcreteModifier::ORDER.into_iter().rev() {
        if physical.contains(modifier) && !wanted.contains(modifier) {
            plan.push(PlanStep::ReleaseModifier(modifier))?;
            temporarily_released.press(modifier);
        }
    }
    for modifier in ConcreteModifier::ORDER {
        if wanted.contains(modifier) && !physical.contains(modifier) {
            plan.push(PlanStep::PressModifier(modifier))?;
            synthetically_pressed.press(modifier);
        }
    }

    plan.push(PlanStep::PrimaryDown)?;
    plan.push(PlanStep::PrimaryUp)?;

    for modifier in ConcreteModifier::ORDER.into_iter().rev() {
        if synthetically_pressed.contains(modifier) {
            plan.push(PlanStep::ReleaseModifier(modifier))?;
        }
    }
    for modifier in ConcreteModifier::ORDER {
        if temporarily_released.contains(modifier) {
            plan.push(PlanStep::PressModifier(modifier))?;
        }
    }

    Ok(plan)
}

fn family_index(family: ModifierFamily) -> usize {
    match family {
        ModifierFamily::Ctrl => 0,
        ModifierFamily::Shift => 1,
        ModifierFamily::Alt => 2,
        ModifierFamily::Meta => 3,
    }
}

fn generic_modifier_family(modifier: Modifier) -> ModifierFamily {
    match modifier {
        Modifier::Ctrl => ModifierFamily::Ctrl,
        Modifier::Shift => ModifierFamily::Shift,
        Modifier::Alt => ModifierFamily::Alt,
        Modifier::Meta => ModifierFamily::Meta,
        _ => unreachable!("concrete modifiers are handled before this conversion"),
    }
}

fn left_modifier(family: ModifierFamily) -> ConcreteModifier {
    match family {
        ModifierFamily::Ctrl => ConcreteModifier::LeftCtrl,
        ModifierFamily::Shift => ConcreteModifier::LeftShift,
        ModifierFamily::Alt => ConcreteModifier::LeftAlt,
        ModifierFamily::Meta => ConcreteModifier::LeftMeta,
    }
}
