use saya::command_line_editor::{CommandLineEdit, command_line_edit_action_for_key};
use saya::input_router::KeyInput;

#[test]
fn left_home_end_and_ctrl_variants_move_the_command_line_cursor() {
    let mut edit = CommandLineEdit::default();
    edit.insert_char('w');
    edit.insert_char('r');
    edit.insert_char('i');
    edit.insert_char('t');
    edit.insert_char('e');

    edit.move_left();
    edit.move_left();
    assert_eq!(edit.cursor_byte_index(), 3);

    edit.move_to_start();
    assert_eq!(edit.cursor_byte_index(), 0);

    edit.move_right();
    assert_eq!(edit.cursor_byte_index(), 1);

    edit.move_to_end();
    assert_eq!(edit.cursor_byte_index(), "write".len());
}

#[test]
fn inserting_and_deleting_at_the_cursor_preserves_char_boundaries() {
    let mut edit = CommandLineEdit::from_buffer("wte");
    edit.move_left();
    edit.move_left();
    edit.insert_char('r');
    edit.insert_char('i');

    assert_eq!(edit.buffer(), "write");
    assert_eq!(edit.cursor_byte_index(), 3);

    edit.backspace();
    assert_eq!(edit.buffer(), "wrte");
    assert_eq!(edit.cursor_byte_index(), 2);

    edit.delete();
    assert_eq!(edit.buffer(), "wre");
    assert_eq!(edit.cursor_byte_index(), 2);
}

#[test]
fn multibyte_cursor_movement_uses_utf8_character_boundaries() {
    let mut edit = CommandLineEdit::from_buffer("あb");

    edit.move_left();
    assert_eq!(edit.cursor_byte_index(), "あ".len());

    edit.backspace();
    assert_eq!(edit.buffer(), "b");
    assert_eq!(edit.cursor_byte_index(), 0);
}

#[test]
fn key_mapping_covers_vim_command_line_cursor_keys() {
    assert!(command_line_edit_action_for_key(&KeyInput::Left).is_some());
    assert!(command_line_edit_action_for_key(&KeyInput::Right).is_some());
    assert!(command_line_edit_action_for_key(&KeyInput::Home).is_some());
    assert!(command_line_edit_action_for_key(&KeyInput::End).is_some());
    assert!(command_line_edit_action_for_key(&KeyInput::Ctrl('b')).is_some());
    assert!(command_line_edit_action_for_key(&KeyInput::Ctrl('f')).is_some());
    assert!(command_line_edit_action_for_key(&KeyInput::Ctrl('a')).is_some());
    assert!(command_line_edit_action_for_key(&KeyInput::Ctrl('e')).is_some());
}
