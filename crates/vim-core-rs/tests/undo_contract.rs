use vim_core_rs::VimCoreSession;

#[test]
fn test_undo_tree_and_jump() {
    let mut session = VimCoreSession::new("initial text\n").expect("session should initialize");

    // Get current buffer ID
    let buffers = session.buffers();
    assert!(!buffers.is_empty());
    let buf_id = buffers[0].id;

    // Initially, there should be an undo tree with at least the initial state, or empty
    let _tree = session.get_undo_tree(buf_id).expect("should get undo tree");

    // Do some edits to create undo history
    session
        .execute_normal_command("oline 2\x1b")
        .expect("insert successful");
    assert_eq!(session.buffer_text(buf_id).unwrap(), "initial text\nline 2");

    let tree_after_1 = session.get_undo_tree(buf_id).expect("should get undo tree");
    assert!(!tree_after_1.nodes.is_empty(), "should have undo nodes");
    let seq_after_1 = tree_after_1.seq_last;

    session
        .execute_normal_command("oline 3\x1b")
        .expect("insert successful");
    assert_eq!(
        session.buffer_text(buf_id).unwrap(),
        "initial text\nline 2\nline 3"
    );

    let tree_after_2 = session.get_undo_tree(buf_id).expect("should get undo tree");
    let seq_after_2 = tree_after_2.seq_last;
    assert!(seq_after_2 > seq_after_1);

    // Jump back to seq_after_1
    session
        .undo_jump(buf_id, seq_after_1)
        .expect("jump should succeed");

    // Check if buffer text is restored
    assert_eq!(session.buffer_text(buf_id).unwrap(), "initial text\nline 2");

    let tree_after_jump = session.get_undo_tree(buf_id).expect("should get undo tree");
    assert_eq!(tree_after_jump.seq_cur, seq_after_1);

    // Create an alternate branch
    session
        .execute_normal_command("oalt line 3\x1b")
        .expect("insert successful");
    assert_eq!(
        session.buffer_text(buf_id).unwrap(),
        "initial text\nline 2\nalt line 3"
    );

    let tree_alt = session.get_undo_tree(buf_id).expect("should get undo tree");
    // Should have nodes now and seq_last should be different from seq_after_2
    assert!(tree_alt.seq_last > seq_after_2);

    // Jump back to seq_after_2 (the other branch)
    session
        .undo_jump(buf_id, seq_after_2)
        .expect("jump should succeed");
    assert_eq!(
        session.buffer_text(buf_id).unwrap(),
        "initial text\nline 2\nline 3"
    );
}
