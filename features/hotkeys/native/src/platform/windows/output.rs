//! Safe, fixed-capacity input encoding. Only the FFI adapter calls SendInput.
use super::{
    compiled::NativeOutput,
    hot_path::QBIT_INJECTION_TAG,
    keys::{NativeKey, PhysicalKey},
};
use crate::domain::Modifier;
use crate::runtime::{ConcreteModifier, Metrics, ModifierState, PlanStep, plan_keystroke};

pub const CAPACITY: usize = 34;
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct EncodedInput {
    pub vk: u16,
    pub scan: u16,
    pub extended: bool,
    pub scan_mode: bool,
    pub up: bool,
    pub tag: usize,
}
pub struct InputBatch {
    events: [EncodedInput; CAPACITY],
    len: usize,
}
impl InputBatch {
    pub fn events(&self) -> &[EncodedInput] {
        &self.events[..self.len]
    }
}

pub fn encode(
    output: NativeOutput,
    physical: ModifierState,
) -> Result<InputBatch, crate::runtime::PlannerError> {
    let mut modifiers = [Modifier::Ctrl; 8];
    let mut len = 0;
    for modifier in output.modifiers() {
        modifiers[len] = modifier;
        len += 1;
    }
    let plan = plan_keystroke(physical, &modifiers[..len])?;
    let mut batch = InputBatch {
        events: [EncodedInput::default(); CAPACITY],
        len: 0,
    };
    for step in plan.steps() {
        let (key, up) = match step {
            PlanStep::PrimaryDown => (output.primary, false),
            PlanStep::PrimaryUp => (output.primary, true),
            PlanStep::PressModifier(modifier) => (modifier_key(modifier), false),
            PlanStep::ReleaseModifier(modifier) => (modifier_key(modifier), true),
        };
        let (vk, scan, extended, scan_mode) = match key {
            NativeKey::Logical(key) => (key.vk, 0, key.extended, false),
            NativeKey::Physical(key) => (0, key.scan_code, key.extended, true),
        };
        batch.events[batch.len] = EncodedInput {
            vk,
            scan,
            extended,
            scan_mode,
            up,
            tag: QBIT_INJECTION_TAG,
        };
        batch.len += 1;
    }
    Ok(batch)
}
fn modifier_key(modifier: ConcreteModifier) -> NativeKey {
    use ConcreteModifier::*;
    let (scan_code, extended) = match modifier {
        LeftCtrl => (0x1d, false),
        RightCtrl => (0x1d, true),
        LeftShift => (0x2a, false),
        RightShift => (0x36, false),
        LeftAlt => (0x38, false),
        RightAlt => (0x38, true),
        LeftMeta => (0x5b, true),
        RightMeta => (0x5c, true),
    };
    NativeKey::Physical(PhysicalKey {
        scan_code,
        extended,
    })
}

pub trait Injector: Send + Sync + 'static {
    fn send(&self, events: &[EncodedInput]) -> usize;
}
pub fn execute(
    injector: &dyn Injector,
    output: NativeOutput,
    physical: ModifierState,
    metrics: &Metrics,
) {
    match encode(output, physical) {
        Ok(batch) if injector.send(batch.events()) == batch.events().len() => {
            metrics.record_action_emitted()
        }
        _ => metrics.record_injection_failure(),
    }
}
