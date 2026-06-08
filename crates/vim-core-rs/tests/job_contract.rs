use vim_core_rs::{CoreHostAction, VimCoreSession};

#[test]
fn test_job_start_is_intercepted_and_sent_to_host() {
    let mut session = VimCoreSession::new("").unwrap();

    // Execute a simple job_start command.
    let result = session
        .execute_ex_command("call job_start(['echo', 'hello'])")
        .unwrap();

    let mut found_job_start = false;
    for action in result.host_actions {
        if let CoreHostAction::JobStart(req) = action {
            found_job_start = true;
            assert_eq!(req.argv, vec!["echo", "hello"]);
            assert_eq!(req.job_id, 1);
            // vfd should be assigned
            assert!(req.vfd_in >= 512);
            assert!(req.vfd_out >= 512);
            assert!(req.vfd_err >= 512);
            break;
        }
    }

    assert!(found_job_start, "Did not receive CoreHostAction::JobStart");
}
