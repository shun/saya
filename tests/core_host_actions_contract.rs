use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use saya::app::bootstrap::launch_test_lock;
use saya::core::bridge::CoreBridge;
use saya::core::host_actions::CoreHostActionRuntime;
use saya::core::outcome::{
    ApplicationOutcomeState, NormalizedHostDirective, fold_normalized_outcomes,
};

fn unique_path(name: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time went backwards")
        .as_nanos();
    std::env::temp_dir().join(format!("saya-core-host-actions-{name}-{nanos}"))
}

fn drain_vfs_until_idle(bridge: &mut CoreBridge, runtime: &mut CoreHostActionRuntime) {
    let mut state = ApplicationOutcomeState::default();
    loop {
        let batch = bridge.take_normalized_outcomes();
        if batch.is_empty() {
            break;
        }
        let folded = fold_normalized_outcomes(batch, state);
        state = folded.state;
        let mut handled = false;
        for directive in folded.effects.host_directives {
            if let NormalizedHostDirective::VfsRequest { request, .. } = directive {
                runtime
                    .handle_vfs_request(bridge, request)
                    .expect("VFS request should be handled by local host runtime");
                handled = true;
            }
        }
        if !handled {
            break;
        }
    }
}

fn drain_host_actions_once(bridge: &mut CoreBridge, runtime: &mut CoreHostActionRuntime) -> bool {
    let folded = fold_normalized_outcomes(
        bridge.take_normalized_outcomes(),
        ApplicationOutcomeState::default(),
    );
    let mut handled = false;
    for directive in folded.effects.host_directives {
        handled = true;
        match directive {
            NormalizedHostDirective::VfsRequest { request, .. } => runtime
                .handle_vfs_request(bridge, request)
                .expect("VFS request should be handled"),
            NormalizedHostDirective::JobStart { request, .. } => runtime
                .start_job(bridge, request)
                .expect("job start should be handled"),
            NormalizedHostDirective::JobWrite { vfd, data, .. } => runtime.write_job(vfd, data),
            NormalizedHostDirective::JobStop { job_id, .. } => runtime
                .stop_job(bridge, job_id)
                .expect("job stop should be handled"),
            NormalizedHostDirective::Write { .. }
            | NormalizedHostDirective::Quit { .. }
            | NormalizedHostDirective::Suspend { .. } => {}
        }
    }
    runtime
        .drain_job_events(bridge)
        .expect("job events should be drained");
    handled
}

fn wait_for_job_status(bridge: &mut CoreBridge, runtime: &mut CoreHostActionRuntime, status: &str) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    while std::time::Instant::now() < deadline {
        runtime
            .drain_job_events(bridge)
            .expect("job events should drain");
        bridge
            .apply_ex_command("call append(0, job_status(g:my_job))")
            .expect("status append should succeed");
        if bridge.snapshot().text.lines().next() == Some(status) {
            return;
        }
        bridge
            .apply_ex_command(":%delete _")
            .expect("status scratch line should clear");
        std::thread::sleep(std::time::Duration::from_millis(20));
    }

    panic!("job should report {status}");
}

#[test]
fn local_vfs_host_opens_file_locator_through_core_bridge() {
    let _lock = launch_test_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let target_path = unique_path("open.txt");
    std::fs::write(&target_path, "alpha\nbeta\n").expect("target file");
    let locator = format!("file://{}", target_path.display());
    let mut bridge = CoreBridge::new("").expect("bridge");
    let mut runtime = CoreHostActionRuntime::default();

    bridge
        .apply_ex_command(&format!(":edit {locator}"))
        .expect("edit should queue VFS request");
    drain_vfs_until_idle(&mut bridge, &mut runtime);

    let snapshot = bridge.snapshot();
    assert_eq!(snapshot.text, "alpha\nbeta\n");

    std::fs::remove_file(&target_path).expect("cleanup");
}

#[test]
fn local_vfs_host_opens_directory_locator_as_sorted_listing() {
    let _lock = launch_test_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let root_path = unique_path("directory-open");
    let nested_path = root_path.join("src");
    let readme_path = root_path.join("README.md");
    std::fs::create_dir_all(&nested_path).expect("target directory");
    std::fs::write(&readme_path, "alpha\n").expect("target file");
    let locator = format!("file://{}", root_path.display());
    let mut bridge = CoreBridge::new("").expect("bridge");
    let mut runtime = CoreHostActionRuntime::default();

    bridge
        .apply_ex_command(&format!(":edit {locator}"))
        .expect("edit should queue VFS request");
    drain_vfs_until_idle(&mut bridge, &mut runtime);

    let snapshot = bridge.snapshot();
    assert_eq!(snapshot.text, "README.md\nsrc/\n");

    std::fs::remove_file(&readme_path).expect("cleanup file");
    std::fs::remove_dir(&nested_path).expect("cleanup nested directory");
    std::fs::remove_dir(&root_path).expect("cleanup root directory");
}

#[test]
fn local_vfs_host_opens_empty_directory_as_empty_listing() {
    let _lock = launch_test_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let root_path = unique_path("directory-empty");
    std::fs::create_dir_all(&root_path).expect("target directory");
    let locator = format!("file://{}", root_path.display());
    let mut bridge = CoreBridge::new("").expect("bridge");
    let mut runtime = CoreHostActionRuntime::default();

    bridge
        .apply_ex_command(&format!(":edit {locator}"))
        .expect("edit should queue VFS request");
    drain_vfs_until_idle(&mut bridge, &mut runtime);

    let snapshot = bridge.snapshot();
    assert_eq!(
        snapshot.text, "\n",
        "vim-core-rs projects an empty loaded buffer as one empty line"
    );

    std::fs::remove_dir(&root_path).expect("cleanup root directory");
}

#[test]
fn local_vfs_host_preserves_space_names_in_directory_listing() {
    let _lock = launch_test_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let root_path = unique_path("directory-space-names");
    let nested_path = root_path.join("src dir");
    let notes_path = root_path.join("daily notes.md");
    std::fs::create_dir_all(&nested_path).expect("target directory");
    std::fs::write(&notes_path, "notes\n").expect("target file");
    let locator = format!("file://{}", root_path.display());
    let mut bridge = CoreBridge::new("").expect("bridge");
    let mut runtime = CoreHostActionRuntime::default();

    bridge
        .apply_ex_command(&format!(":edit {locator}"))
        .expect("edit should queue VFS request");
    drain_vfs_until_idle(&mut bridge, &mut runtime);

    let snapshot = bridge.snapshot();
    assert_eq!(snapshot.text, "daily notes.md\nsrc dir/\n");

    std::fs::remove_file(&notes_path).expect("cleanup file");
    std::fs::remove_dir(&nested_path).expect("cleanup nested directory");
    std::fs::remove_dir(&root_path).expect("cleanup root directory");
}

#[test]
fn local_vfs_host_opens_relative_directory_locator_from_current_directory() {
    let _lock = launch_test_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let root_path = unique_path("directory-relative-root");
    let work_path = root_path.join("work");
    let nested_path = work_path.join("src");
    let readme_path = work_path.join("README.md");
    std::fs::create_dir_all(&nested_path).expect("target directory");
    std::fs::write(&readme_path, "alpha\n").expect("target file");
    let previous_dir = std::env::current_dir().expect("current dir");
    std::env::set_current_dir(&root_path).expect("enter test root");
    let mut bridge = CoreBridge::new("").expect("bridge");
    let mut runtime = CoreHostActionRuntime::default();

    bridge
        .apply_ex_command(":edit work")
        .expect("edit should queue VFS request");
    drain_vfs_until_idle(&mut bridge, &mut runtime);

    std::env::set_current_dir(previous_dir).expect("restore current dir");
    let snapshot = bridge.snapshot();
    assert_eq!(snapshot.text, "README.md\nsrc/\n");

    std::fs::remove_file(&readme_path).expect("cleanup file");
    std::fs::remove_dir(&nested_path).expect("cleanup nested directory");
    std::fs::remove_dir(&work_path).expect("cleanup work directory");
    std::fs::remove_dir(&root_path).expect("cleanup root directory");
}

#[test]
fn local_vfs_host_saves_file_locator_through_core_bridge() {
    let _lock = launch_test_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let target_path = unique_path("save.txt");
    std::fs::write(&target_path, "alpha\n").expect("target file");
    let locator = format!("file://{}", target_path.display());
    let mut bridge = CoreBridge::new("").expect("bridge");
    let mut runtime = CoreHostActionRuntime::default();

    bridge
        .apply_ex_command(&format!(":edit {locator}"))
        .expect("edit should queue VFS request");
    drain_vfs_until_idle(&mut bridge, &mut runtime);
    bridge.dispatch_key("iX").expect("edit VFS buffer");
    bridge.dispatch_key("\x1b").expect("leave insert mode");

    bridge
        .apply_ex_command(":write")
        .expect("write should queue VFS save request");
    drain_vfs_until_idle(&mut bridge, &mut runtime);

    assert_eq!(
        std::fs::read_to_string(&target_path).expect("saved file"),
        "Xalpha"
    );

    std::fs::remove_file(&target_path).expect("cleanup");
}

#[test]
fn job_host_starts_process_and_reports_finished_status_to_core() {
    let _lock = launch_test_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let mut bridge = CoreBridge::new("").expect("bridge");
    let mut runtime = CoreHostActionRuntime::default();

    bridge
        .apply_ex_command("let g:my_job = job_start(['/bin/echo', 'hello'])")
        .expect("job_start should queue JobStart host action");
    assert!(drain_host_actions_once(&mut bridge, &mut runtime));

    wait_for_job_status(&mut bridge, &mut runtime, "dead");
}

#[test]
fn job_host_stops_running_process_and_reports_dead_status_to_core() {
    let _lock = launch_test_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let mut bridge = CoreBridge::new("").expect("bridge");
    let mut runtime = CoreHostActionRuntime::default();

    bridge
        .apply_ex_command("let g:my_job = job_start(['/bin/sh', '-c', 'sleep 5'])")
        .expect("job_start should queue JobStart host action");
    assert!(drain_host_actions_once(&mut bridge, &mut runtime));

    bridge
        .apply_ex_command("call job_stop(g:my_job)")
        .expect("job_stop should queue JobStop host action");
    assert!(drain_host_actions_once(&mut bridge, &mut runtime));

    wait_for_job_status(&mut bridge, &mut runtime, "dead");
}
