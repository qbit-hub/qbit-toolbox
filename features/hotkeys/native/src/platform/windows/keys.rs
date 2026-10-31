use std::fmt;

use windows::Win32::UI::Input::KeyboardAndMouse::*;

use crate::domain::{KeyIdentity, KeyToken};
use crate::runtime::ConcreteModifier;

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct LogicalKey {
    pub vk: u16,
    pub extended: bool,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct PhysicalKey {
    pub scan_code: u16,
    pub extended: bool,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum NativeKey {
    Logical(LogicalKey),
    Physical(PhysicalKey),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum KeyRole {
    Trigger,
    Target,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum KeyIdentityKind {
    Logical,
    Physical,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum KeyTranslationError {
    UnsupportedToken {
        token: String,
        identity: KeyIdentityKind,
        role: KeyRole,
    },
    UnsupportedPhysicalKey {
        token: String,
        role: KeyRole,
    },
    UnsupportedPrimaryModifier {
        token: String,
        role: KeyRole,
    },
}

impl fmt::Display for KeyTranslationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedToken {
                token,
                identity,
                role,
            } => {
                write!(
                    formatter,
                    "unsupported {identity:?} {role:?} key token {token}"
                )
            }
            Self::UnsupportedPhysicalKey { token, role } => {
                write!(formatter, "unsupported physical {role:?} key token {token}")
            }
            Self::UnsupportedPrimaryModifier { token, role } => {
                write!(
                    formatter,
                    "modifier token {token} cannot be a {role:?} primary"
                )
            }
        }
    }
}

impl std::error::Error for KeyTranslationError {}

pub fn translate(identity: &KeyIdentity, role: KeyRole) -> Result<NativeKey, KeyTranslationError> {
    let (token, kind) = match identity {
        KeyIdentity::Logical(token) => (token, KeyIdentityKind::Logical),
        KeyIdentity::Physical(token) => (token, KeyIdentityKind::Physical),
    };
    if is_modifier_token(token.as_str()) {
        return Err(KeyTranslationError::UnsupportedPrimaryModifier {
            token: token.as_str().to_owned(),
            role,
        });
    }
    match kind {
        KeyIdentityKind::Logical => logical_key(token, role).map(NativeKey::Logical),
        KeyIdentityKind::Physical => physical_key(token, role).map(NativeKey::Physical),
    }
}

fn is_modifier_token(token: &str) -> bool {
    matches!(
        token,
        "Ctrl"
            | "LeftCtrl"
            | "RightCtrl"
            | "Shift"
            | "LeftShift"
            | "RightShift"
            | "Alt"
            | "LeftAlt"
            | "RightAlt"
            | "Meta"
            | "LeftMeta"
            | "RightMeta"
    )
}

fn unsupported(token: &KeyToken, identity: KeyIdentityKind, role: KeyRole) -> KeyTranslationError {
    KeyTranslationError::UnsupportedToken {
        token: token.as_str().to_owned(),
        identity,
        role,
    }
}

fn logical_key(token: &KeyToken, role: KeyRole) -> Result<LogicalKey, KeyTranslationError> {
    let (vk, extended) = match token.as_str() {
        "Escape" => (VK_ESCAPE.0, false),
        "Tab" => (VK_TAB.0, false),
        "CapsLock" => (VK_CAPITAL.0, false),
        "Space" => (VK_SPACE.0, false),
        "Enter" => (VK_RETURN.0, false),
        "NumpadEnter" => (VK_RETURN.0, true),
        "Backspace" => (VK_BACK.0, false),
        "Insert" => (VK_INSERT.0, true),
        "Delete" => (VK_DELETE.0, true),
        "Home" => (VK_HOME.0, true),
        "End" => (VK_END.0, true),
        "PageUp" => (VK_PRIOR.0, true),
        "PageDown" => (VK_NEXT.0, true),
        "ArrowUp" => (VK_UP.0, true),
        "ArrowDown" => (VK_DOWN.0, true),
        "ArrowLeft" => (VK_LEFT.0, true),
        "ArrowRight" => (VK_RIGHT.0, true),
        "PrintScreen" => (VK_SNAPSHOT.0, true),
        "ScrollLock" => (VK_SCROLL.0, false),
        "Pause" => (VK_PAUSE.0, false),
        "NumLock" => (VK_NUMLOCK.0, false),
        "NumpadAdd" => (VK_ADD.0, false),
        "NumpadSubtract" => (VK_SUBTRACT.0, false),
        "NumpadMultiply" => (VK_MULTIPLY.0, false),
        "NumpadDivide" => (VK_DIVIDE.0, true),
        "NumpadDecimal" => (VK_DECIMAL.0, false),
        "Minus" => (VK_OEM_MINUS.0, false),
        "Equal" => (VK_OEM_PLUS.0, false),
        "BracketLeft" => (VK_OEM_4.0, false),
        "BracketRight" => (VK_OEM_6.0, false),
        "Backslash" => (VK_OEM_5.0, false),
        "IntlBackslash" => (VK_OEM_102.0, false),
        "Semicolon" => (VK_OEM_1.0, false),
        "Quote" => (VK_OEM_7.0, false),
        "Backquote" => (VK_OEM_3.0, false),
        "Comma" => (VK_OEM_COMMA.0, false),
        "Period" => (VK_OEM_PERIOD.0, false),
        "Slash" => (VK_OEM_2.0, false),
        "ContextMenu" => (VK_APPS.0, true),
        value => return logical_alphanumeric(value, token, role),
    };
    Ok(LogicalKey { vk, extended })
}

fn logical_alphanumeric(
    value: &str,
    token: &KeyToken,
    role: KeyRole,
) -> Result<LogicalKey, KeyTranslationError> {
    let vk = match value {
        "KeyA" => VK_A.0,
        "KeyB" => VK_B.0,
        "KeyC" => VK_C.0,
        "KeyD" => VK_D.0,
        "KeyE" => VK_E.0,
        "KeyF" => VK_F.0,
        "KeyG" => VK_G.0,
        "KeyH" => VK_H.0,
        "KeyI" => VK_I.0,
        "KeyJ" => VK_J.0,
        "KeyK" => VK_K.0,
        "KeyL" => VK_L.0,
        "KeyM" => VK_M.0,
        "KeyN" => VK_N.0,
        "KeyO" => VK_O.0,
        "KeyP" => VK_P.0,
        "KeyQ" => VK_Q.0,
        "KeyR" => VK_R.0,
        "KeyS" => VK_S.0,
        "KeyT" => VK_T.0,
        "KeyU" => VK_U.0,
        "KeyV" => VK_V.0,
        "KeyW" => VK_W.0,
        "KeyX" => VK_X.0,
        "KeyY" => VK_Y.0,
        "KeyZ" => VK_Z.0,
        "Digit0" => VK_0.0,
        "Digit1" => VK_1.0,
        "Digit2" => VK_2.0,
        "Digit3" => VK_3.0,
        "Digit4" => VK_4.0,
        "Digit5" => VK_5.0,
        "Digit6" => VK_6.0,
        "Digit7" => VK_7.0,
        "Digit8" => VK_8.0,
        "Digit9" => VK_9.0,
        "Numpad0" => VK_NUMPAD0.0,
        "Numpad1" => VK_NUMPAD1.0,
        "Numpad2" => VK_NUMPAD2.0,
        "Numpad3" => VK_NUMPAD3.0,
        "Numpad4" => VK_NUMPAD4.0,
        "Numpad5" => VK_NUMPAD5.0,
        "Numpad6" => VK_NUMPAD6.0,
        "Numpad7" => VK_NUMPAD7.0,
        "Numpad8" => VK_NUMPAD8.0,
        "Numpad9" => VK_NUMPAD9.0,
        "F1" => VK_F1.0,
        "F2" => VK_F2.0,
        "F3" => VK_F3.0,
        "F4" => VK_F4.0,
        "F5" => VK_F5.0,
        "F6" => VK_F6.0,
        "F7" => VK_F7.0,
        "F8" => VK_F8.0,
        "F9" => VK_F9.0,
        "F10" => VK_F10.0,
        "F11" => VK_F11.0,
        "F12" => VK_F12.0,
        "F13" => VK_F13.0,
        "F14" => VK_F14.0,
        "F15" => VK_F15.0,
        "F16" => VK_F16.0,
        "F17" => VK_F17.0,
        "F18" => VK_F18.0,
        "F19" => VK_F19.0,
        "F20" => VK_F20.0,
        "F21" => VK_F21.0,
        "F22" => VK_F22.0,
        "F23" => VK_F23.0,
        "F24" => VK_F24.0,
        _ => return Err(unsupported(token, KeyIdentityKind::Logical, role)),
    };
    Ok(LogicalKey {
        vk,
        extended: false,
    })
}

fn physical_key(token: &KeyToken, role: KeyRole) -> Result<PhysicalKey, KeyTranslationError> {
    if matches!(
        token.as_str(),
        "PrintScreen"
            | "Pause"
            | "F13"
            | "F14"
            | "F15"
            | "F16"
            | "F17"
            | "F18"
            | "F19"
            | "F20"
            | "F21"
            | "F22"
            | "F23"
            | "F24"
    ) {
        return Err(KeyTranslationError::UnsupportedPhysicalKey {
            token: token.as_str().to_owned(),
            role,
        });
    }
    let (scan_code, extended) = match token.as_str() {
        "Escape" => (0x01, false),
        "Digit1" => (0x02, false),
        "Digit2" => (0x03, false),
        "Digit3" => (0x04, false),
        "Digit4" => (0x05, false),
        "Digit5" => (0x06, false),
        "Digit6" => (0x07, false),
        "Digit7" => (0x08, false),
        "Digit8" => (0x09, false),
        "Digit9" => (0x0a, false),
        "Digit0" => (0x0b, false),
        "Minus" => (0x0c, false),
        "Equal" => (0x0d, false),
        "Backspace" => (0x0e, false),
        "Tab" => (0x0f, false),
        "KeyQ" => (0x10, false),
        "KeyW" => (0x11, false),
        "KeyE" => (0x12, false),
        "KeyR" => (0x13, false),
        "KeyT" => (0x14, false),
        "KeyY" => (0x15, false),
        "KeyU" => (0x16, false),
        "KeyI" => (0x17, false),
        "KeyO" => (0x18, false),
        "KeyP" => (0x19, false),
        "BracketLeft" => (0x1a, false),
        "BracketRight" => (0x1b, false),
        "Enter" => (0x1c, false),
        "NumpadEnter" => (0x1c, true),
        "KeyA" => (0x1e, false),
        "KeyS" => (0x1f, false),
        "KeyD" => (0x20, false),
        "KeyF" => (0x21, false),
        "KeyG" => (0x22, false),
        "KeyH" => (0x23, false),
        "KeyJ" => (0x24, false),
        "KeyK" => (0x25, false),
        "KeyL" => (0x26, false),
        "Semicolon" => (0x27, false),
        "Quote" => (0x28, false),
        "Backquote" => (0x29, false),
        "Backslash" => (0x2b, false),
        "KeyZ" => (0x2c, false),
        "KeyX" => (0x2d, false),
        "KeyC" => (0x2e, false),
        "KeyV" => (0x2f, false),
        "KeyB" => (0x30, false),
        "KeyN" => (0x31, false),
        "KeyM" => (0x32, false),
        "Comma" => (0x33, false),
        "Period" => (0x34, false),
        "Slash" => (0x35, false),
        "NumpadMultiply" => (0x37, false),
        "Space" => (0x39, false),
        "CapsLock" => (0x3a, false),
        "F1" => (0x3b, false),
        "F2" => (0x3c, false),
        "F3" => (0x3d, false),
        "F4" => (0x3e, false),
        "F5" => (0x3f, false),
        "F6" => (0x40, false),
        "F7" => (0x41, false),
        "F8" => (0x42, false),
        "F9" => (0x43, false),
        "F10" => (0x44, false),
        "NumLock" => (0x45, false),
        "ScrollLock" => (0x46, false),
        "Numpad7" => (0x47, false),
        "Numpad8" => (0x48, false),
        "Numpad9" => (0x49, false),
        "NumpadSubtract" => (0x4a, false),
        "Numpad4" => (0x4b, false),
        "Numpad5" => (0x4c, false),
        "Numpad6" => (0x4d, false),
        "NumpadAdd" => (0x4e, false),
        "Numpad1" => (0x4f, false),
        "Numpad2" => (0x50, false),
        "Numpad3" => (0x51, false),
        "Numpad0" => (0x52, false),
        "NumpadDecimal" => (0x53, false),
        "IntlBackslash" => (0x56, false),
        "F11" => (0x57, false),
        "F12" => (0x58, false),
        "NumpadDivide" => (0x35, true),
        "Home" => (0x47, true),
        "ArrowUp" => (0x48, true),
        "PageUp" => (0x49, true),
        "ArrowLeft" => (0x4b, true),
        "ArrowRight" => (0x4d, true),
        "End" => (0x4f, true),
        "ArrowDown" => (0x50, true),
        "PageDown" => (0x51, true),
        "Insert" => (0x52, true),
        "Delete" => (0x53, true),
        "ContextMenu" => (0x5d, true),
        _ => return Err(unsupported(token, KeyIdentityKind::Physical, role)),
    };
    Ok(PhysicalKey {
        scan_code,
        extended,
    })
}

pub fn modifier_from_raw(vk: u32, scan: u32, extended: bool) -> Option<ConcreteModifier> {
    match vk {
        value if value == VK_SHIFT.0 as u32 => match scan {
            0x2a => Some(ConcreteModifier::LeftShift),
            0x36 => Some(ConcreteModifier::RightShift),
            _ => None,
        },
        value if value == VK_LSHIFT.0 as u32 => Some(ConcreteModifier::LeftShift),
        value if value == VK_RSHIFT.0 as u32 => Some(ConcreteModifier::RightShift),
        value if value == VK_CONTROL.0 as u32 => Some(if extended {
            ConcreteModifier::RightCtrl
        } else {
            ConcreteModifier::LeftCtrl
        }),
        value if value == VK_LCONTROL.0 as u32 => Some(ConcreteModifier::LeftCtrl),
        value if value == VK_RCONTROL.0 as u32 => Some(ConcreteModifier::RightCtrl),
        value if value == VK_MENU.0 as u32 => Some(if extended {
            ConcreteModifier::RightAlt
        } else {
            ConcreteModifier::LeftAlt
        }),
        value if value == VK_LMENU.0 as u32 => Some(ConcreteModifier::LeftAlt),
        value if value == VK_RMENU.0 as u32 => Some(ConcreteModifier::RightAlt),
        value if value == VK_LWIN.0 as u32 => Some(ConcreteModifier::LeftMeta),
        value if value == VK_RWIN.0 as u32 => Some(ConcreteModifier::RightMeta),
        _ => None,
    }
}
