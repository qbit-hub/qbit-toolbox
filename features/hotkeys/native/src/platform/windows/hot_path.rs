use crossbeam_queue::ArrayQueue;

use crate::runtime::{Metrics, ModifierState};

use super::{
    compiled::{ActionRequest, NativeAction, NativeCompiledKeymap},
    keys::{LogicalKey, PhysicalKey, modifier_from_raw},
};

/// Recursion-identification tag for input emitted by Qbit. This is not a
/// security or authentication boundary.
#[cfg(target_pointer_width = "64")]
pub const QBIT_INJECTION_TAG: usize = 0x5142_4954_484B_0001;
/// Recursion-identification tag for input emitted by Qbit. This is not a
/// security or authentication boundary.
#[cfg(target_pointer_width = "32")]
pub const QBIT_INJECTION_TAG: usize = 0x5142_0001;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InputOrigin {
    Physical,
    QbitInjected,
    ThirdPartyInjected,
}

pub fn classify_origin(
    injected: bool,
    lower_integrity_injected: bool,
    extra_info: usize,
) -> InputOrigin {
    if extra_info == QBIT_INJECTION_TAG {
        InputOrigin::QbitInjected
    } else if injected || lower_integrity_injected {
        InputOrigin::ThirdPartyInjected
    } else {
        InputOrigin::Physical
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum KeyEventKind {
    Down,
    Up,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RawEvent {
    pub kind: KeyEventKind,
    pub vk_code: u32,
    pub scan_code: u32,
    pub extended: bool,
    pub origin: InputOrigin,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Decision {
    Pass,
    Consume,
}

#[derive(Clone, Copy)]
struct KeySlot(usize);

impl KeySlot {
    fn from_event(event: RawEvent) -> Option<Self> {
        if (1..=255).contains(&event.scan_code) {
            Some(Self(
                event.scan_code as usize * 2 + usize::from(event.extended),
            ))
        } else if event.vk_code <= 255 {
            Some(Self(512 + event.vk_code as usize))
        } else {
            None
        }
    }
}

pub struct HotPathState {
    modifiers: ModifierState,
    down: [u64; 16],
    consumed: [u64; 16],
}

impl Default for HotPathState {
    fn default() -> Self {
        Self {
            modifiers: ModifierState::empty(),
            down: [0; 16],
            consumed: [0; 16],
        }
    }
}

impl HotPathState {
    pub fn modifiers(&self) -> ModifierState {
        self.modifiers
    }

    pub fn process(
        &mut self,
        event: RawEvent,
        keymap: &NativeCompiledKeymap,
        queue: &ArrayQueue<ActionRequest>,
        metrics: &Metrics,
    ) -> Decision {
        metrics.record_event_seen();
        if event.origin != InputOrigin::Physical {
            return Decision::Pass;
        }

        // Reject malformed raw input before it can modify physical state.
        // A zero scan code is valid only with a nonzero, bounded VK fallback.
        if !(1..=254).contains(&event.vk_code) || event.scan_code > 255 {
            return Decision::Pass;
        }

        if let Some(modifier) = modifier_from_raw(event.vk_code, event.scan_code, event.extended) {
            match event.kind {
                KeyEventKind::Down => self.modifiers.press(modifier),
                KeyEventKind::Up => self.modifiers.release(modifier),
            }
            return Decision::Pass;
        }

        let Some(slot) = KeySlot::from_event(event) else {
            return Decision::Pass;
        };

        match event.kind {
            KeyEventKind::Up => self.key_up(slot),
            KeyEventKind::Down => self.key_down(slot, event, keymap, queue, metrics),
        }
    }

    fn key_up(&mut self, slot: KeySlot) -> Decision {
        bit_clear(&mut self.down, slot);
        if bit_take(&mut self.consumed, slot) {
            Decision::Consume
        } else {
            Decision::Pass
        }
    }

    fn key_down(
        &mut self,
        slot: KeySlot,
        event: RawEvent,
        keymap: &NativeCompiledKeymap,
        queue: &ArrayQueue<ActionRequest>,
        metrics: &Metrics,
    ) -> Decision {
        if bit_contains(&self.down, slot) {
            return if bit_contains(&self.consumed, slot) {
                Decision::Consume
            } else {
                Decision::Pass
            };
        }
        bit_set(&mut self.down, slot);

        let logical = u16::try_from(event.vk_code).ok().map(|vk| LogicalKey {
            vk,
            extended: event.extended,
        });
        let physical = u16::try_from(event.scan_code)
            .ok()
            .map(|scan_code| PhysicalKey {
                scan_code,
                extended: event.extended,
            });
        let Some(request) = keymap.lookup_event(logical, physical, self.modifiers) else {
            return Decision::Pass;
        };
        metrics.record_event_matched();

        match request.action {
            NativeAction::Disable => {
                bit_set(&mut self.consumed, slot);
                Decision::Consume
            }
            NativeAction::EmitShortcut(_) => match queue.push(request) {
                Ok(()) => {
                    bit_set(&mut self.consumed, slot);
                    Decision::Consume
                }
                Err(_) => {
                    metrics.record_queue_overflow();
                    Decision::Pass
                }
            },
        }
    }
}

fn bit_index(slot: KeySlot) -> (usize, u32) {
    (slot.0 / 64, (slot.0 % 64) as u32)
}

fn bit_contains(bits: &[u64; 16], slot: KeySlot) -> bool {
    let (word, bit) = bit_index(slot);
    bits[word] & (1_u64 << bit) != 0
}

fn bit_set(bits: &mut [u64; 16], slot: KeySlot) {
    let (word, bit) = bit_index(slot);
    bits[word] |= 1_u64 << bit;
}

fn bit_clear(bits: &mut [u64; 16], slot: KeySlot) {
    let (word, bit) = bit_index(slot);
    bits[word] &= !(1_u64 << bit);
}

fn bit_take(bits: &mut [u64; 16], slot: KeySlot) -> bool {
    let was_set = bit_contains(bits, slot);
    bit_clear(bits, slot);
    was_set
}
