//! Traducción de códigos HID de teclado de iOS (`UIKeyboardHIDUsage`) a tipos de winit.
//!
//! Los valores corresponden a la USB HID Usage Page 0x07 (Keyboard/Keypad), que es la
//! numeración que usa `UIKeyboardHIDUsage` en UIKit.

use crate::keyboard::{KeyCode, ModifiersState, NamedKey};

/// Bits de `UIKeyModifierFlags` (UIKit).
pub(crate) const MOD_SHIFT: isize = 1 << 17;
pub(crate) const MOD_CONTROL: isize = 1 << 18;
pub(crate) const MOD_ALT: isize = 1 << 19;
pub(crate) const MOD_COMMAND: isize = 1 << 20;

/// Traduce un usage HID a un [`KeyCode`] físico. Devuelve `None` para usages no soportados.
pub(crate) fn key_code_from_hid(usage: isize) -> Option<KeyCode> {
    let code = match usage {
        0x04 => KeyCode::KeyA,
        0x05 => KeyCode::KeyB,
        0x06 => KeyCode::KeyC,
        0x07 => KeyCode::KeyD,
        0x08 => KeyCode::KeyE,
        0x09 => KeyCode::KeyF,
        0x0A => KeyCode::KeyG,
        0x0B => KeyCode::KeyH,
        0x0C => KeyCode::KeyI,
        0x0D => KeyCode::KeyJ,
        0x0E => KeyCode::KeyK,
        0x0F => KeyCode::KeyL,
        0x10 => KeyCode::KeyM,
        0x11 => KeyCode::KeyN,
        0x12 => KeyCode::KeyO,
        0x13 => KeyCode::KeyP,
        0x14 => KeyCode::KeyQ,
        0x15 => KeyCode::KeyR,
        0x16 => KeyCode::KeyS,
        0x17 => KeyCode::KeyT,
        0x18 => KeyCode::KeyU,
        0x19 => KeyCode::KeyV,
        0x1A => KeyCode::KeyW,
        0x1B => KeyCode::KeyX,
        0x1C => KeyCode::KeyY,
        0x1D => KeyCode::KeyZ,
        0x1E => KeyCode::Digit1,
        0x1F => KeyCode::Digit2,
        0x20 => KeyCode::Digit3,
        0x21 => KeyCode::Digit4,
        0x22 => KeyCode::Digit5,
        0x23 => KeyCode::Digit6,
        0x24 => KeyCode::Digit7,
        0x25 => KeyCode::Digit8,
        0x26 => KeyCode::Digit9,
        0x27 => KeyCode::Digit0,
        0x28 => KeyCode::Enter,
        0x29 => KeyCode::Escape,
        0x2A => KeyCode::Backspace,
        0x2B => KeyCode::Tab,
        0x2C => KeyCode::Space,
        0x2D => KeyCode::Minus,
        0x2E => KeyCode::Equal,
        0x2F => KeyCode::BracketLeft,
        0x30 => KeyCode::BracketRight,
        0x31 => KeyCode::Backslash,
        0x33 => KeyCode::Semicolon,
        0x34 => KeyCode::Quote,
        0x35 => KeyCode::Backquote,
        0x36 => KeyCode::Comma,
        0x37 => KeyCode::Period,
        0x38 => KeyCode::Slash,
        0x39 => KeyCode::CapsLock,
        0x3A => KeyCode::F1,
        0x3B => KeyCode::F2,
        0x3C => KeyCode::F3,
        0x3D => KeyCode::F4,
        0x3E => KeyCode::F5,
        0x3F => KeyCode::F6,
        0x40 => KeyCode::F7,
        0x41 => KeyCode::F8,
        0x42 => KeyCode::F9,
        0x43 => KeyCode::F10,
        0x44 => KeyCode::F11,
        0x45 => KeyCode::F12,
        0x49 => KeyCode::Insert,
        0x4A => KeyCode::Home,
        0x4B => KeyCode::PageUp,
        0x4C => KeyCode::Delete,
        0x4D => KeyCode::End,
        0x4E => KeyCode::PageDown,
        0x4F => KeyCode::ArrowRight,
        0x50 => KeyCode::ArrowLeft,
        0x51 => KeyCode::ArrowDown,
        0x52 => KeyCode::ArrowUp,
        0x64 => KeyCode::IntlBackslash,
        0xE0 => KeyCode::ControlLeft,
        0xE1 => KeyCode::ShiftLeft,
        0xE2 => KeyCode::AltLeft,
        0xE3 => KeyCode::SuperLeft,
        0xE4 => KeyCode::ControlRight,
        0xE5 => KeyCode::ShiftRight,
        0xE6 => KeyCode::AltRight,
        0xE7 => KeyCode::SuperRight,
        _ => return None,
    };
    Some(code)
}

/// Traduce un usage HID a un [`NamedKey`]. Solo cubre teclas que no producen texto; el resto
/// (incluido Space, que genera texto) devuelve `None`.
pub(crate) fn named_key_from_hid(usage: isize) -> Option<NamedKey> {
    let key = match usage {
        0x28 => NamedKey::Enter,
        0x29 => NamedKey::Escape,
        0x2A => NamedKey::Backspace,
        0x2B => NamedKey::Tab,
        0x39 => NamedKey::CapsLock,
        0x3A => NamedKey::F1,
        0x3B => NamedKey::F2,
        0x3C => NamedKey::F3,
        0x3D => NamedKey::F4,
        0x3E => NamedKey::F5,
        0x3F => NamedKey::F6,
        0x40 => NamedKey::F7,
        0x41 => NamedKey::F8,
        0x42 => NamedKey::F9,
        0x43 => NamedKey::F10,
        0x44 => NamedKey::F11,
        0x45 => NamedKey::F12,
        0x49 => NamedKey::Insert,
        0x4A => NamedKey::Home,
        0x4B => NamedKey::PageUp,
        0x4C => NamedKey::Delete,
        0x4D => NamedKey::End,
        0x4E => NamedKey::PageDown,
        0x4F => NamedKey::ArrowRight,
        0x50 => NamedKey::ArrowLeft,
        0x51 => NamedKey::ArrowDown,
        0x52 => NamedKey::ArrowUp,
        0xE0 | 0xE4 => NamedKey::Control,
        0xE1 | 0xE5 => NamedKey::Shift,
        0xE2 | 0xE6 => NamedKey::Alt,
        0xE3 | 0xE7 => NamedKey::Super,
        _ => return None,
    };
    Some(key)
}

/// Indica si el usage HID corresponde a una tecla modificadora (0xE0..=0xE7).
pub(crate) fn is_modifier_hid(usage: isize) -> bool {
    (0xE0..=0xE7).contains(&usage)
}

/// Convierte flags de `UIKeyModifierFlags` a un [`ModifiersState`] de winit
/// (Command se mapea a SUPER).
pub(crate) fn modifiers_from_flags(flags: isize) -> ModifiersState {
    let mut state = ModifiersState::empty();
    if flags & MOD_SHIFT != 0 {
        state.insert(ModifiersState::SHIFT);
    }
    if flags & MOD_CONTROL != 0 {
        state.insert(ModifiersState::CONTROL);
    }
    if flags & MOD_ALT != 0 {
        state.insert(ModifiersState::ALT);
    }
    if flags & MOD_COMMAND != 0 {
        state.insert(ModifiersState::SUPER);
    }
    state
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_a_from_hid() {
        assert_eq!(key_code_from_hid(0x04), Some(KeyCode::KeyA));
    }

    #[test]
    fn arrow_up_from_hid() {
        assert_eq!(key_code_from_hid(0x52), Some(KeyCode::ArrowUp));
        assert_eq!(named_key_from_hid(0x52), Some(NamedKey::ArrowUp));
    }

    #[test]
    fn space_is_not_named() {
        assert_eq!(named_key_from_hid(0x2C), None);
    }

    #[test]
    fn super_left_is_modifier() {
        assert!(is_modifier_hid(0xE3));
        assert!(!is_modifier_hid(0x04));
    }

    #[test]
    fn modifiers_shift_and_command() {
        let state = modifiers_from_flags(MOD_SHIFT | MOD_COMMAND);
        assert!(state.contains(ModifiersState::SHIFT));
        assert!(state.contains(ModifiersState::SUPER));
        assert!(!state.contains(ModifiersState::CONTROL));
    }
}
