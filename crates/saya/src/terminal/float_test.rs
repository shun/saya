use super::*;

#[test]
fn terminal_key_bytes_preserves_shift_enter_as_modified_enter_sequence() {
    assert_eq!(terminal_key_bytes(&KeyInput::ShiftEnter), "\x1b[13;2u");
}
