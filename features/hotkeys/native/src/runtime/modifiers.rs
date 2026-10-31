use crate::domain::{Modifier, ModifierFamily};

/// The explicit modifier order used by the runtime protocol.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
#[repr(u8)]
pub enum ConcreteModifier {
    LeftCtrl,
    RightCtrl,
    LeftShift,
    RightShift,
    LeftAlt,
    RightAlt,
    LeftMeta,
    RightMeta,
}

impl ConcreteModifier {
    pub const ORDER: [Self; 8] = [
        Self::LeftCtrl,
        Self::RightCtrl,
        Self::LeftShift,
        Self::RightShift,
        Self::LeftAlt,
        Self::RightAlt,
        Self::LeftMeta,
        Self::RightMeta,
    ];

    pub const fn bit(self) -> u16 {
        1 << (self as u8)
    }

    pub const fn family(self) -> ModifierFamily {
        match self {
            Self::LeftCtrl | Self::RightCtrl => ModifierFamily::Ctrl,
            Self::LeftShift | Self::RightShift => ModifierFamily::Shift,
            Self::LeftAlt | Self::RightAlt => ModifierFamily::Alt,
            Self::LeftMeta | Self::RightMeta => ModifierFamily::Meta,
        }
    }

    /// Converts a sided domain modifier. Generic modifiers are requirements,
    /// and consequently have no concrete state representation.
    pub const fn from_domain_modifier(modifier: Modifier) -> Option<Self> {
        match modifier {
            Modifier::LeftCtrl => Some(Self::LeftCtrl),
            Modifier::RightCtrl => Some(Self::RightCtrl),
            Modifier::LeftShift => Some(Self::LeftShift),
            Modifier::RightShift => Some(Self::RightShift),
            Modifier::LeftAlt => Some(Self::LeftAlt),
            Modifier::RightAlt => Some(Self::RightAlt),
            Modifier::LeftMeta => Some(Self::LeftMeta),
            Modifier::RightMeta => Some(Self::RightMeta),
            Modifier::Ctrl | Modifier::Shift | Modifier::Alt | Modifier::Meta => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub struct ModifierState(u16);

impl ModifierState {
    const VALID_BITS: u16 = 0x00ff;

    pub const fn empty() -> Self {
        Self(0)
    }

    pub const fn from_bits(bits: u16) -> Self {
        Self(bits & Self::VALID_BITS)
    }

    pub const fn bits(self) -> u16 {
        self.0
    }

    pub const fn contains(self, modifier: ConcreteModifier) -> bool {
        self.0 & modifier.bit() != 0
    }

    pub fn press(&mut self, modifier: ConcreteModifier) {
        self.0 |= modifier.bit();
    }

    pub fn release(&mut self, modifier: ConcreteModifier) {
        self.0 &= !modifier.bit();
    }

    pub const fn family_mask(family: ModifierFamily) -> u16 {
        match family {
            ModifierFamily::Ctrl => {
                ConcreteModifier::LeftCtrl.bit() | ConcreteModifier::RightCtrl.bit()
            }
            ModifierFamily::Shift => {
                ConcreteModifier::LeftShift.bit() | ConcreteModifier::RightShift.bit()
            }
            ModifierFamily::Alt => {
                ConcreteModifier::LeftAlt.bit() | ConcreteModifier::RightAlt.bit()
            }
            ModifierFamily::Meta => {
                ConcreteModifier::LeftMeta.bit() | ConcreteModifier::RightMeta.bit()
            }
        }
    }

    pub const fn contains_family(self, family: ModifierFamily) -> bool {
        self.0 & Self::family_mask(family) != 0
    }

    pub fn iter(self) -> impl Iterator<Item = ConcreteModifier> {
        ConcreteModifier::ORDER
            .into_iter()
            .filter(move |modifier| self.contains(*modifier))
    }
}
