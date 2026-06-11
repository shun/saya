//! 初期バッファの読み込み（ファイル・標準入力）と初期カーソルの適用を担当する。

use super::*;

pub(super) fn load_initial_buffer<R: Read>(
    input_source: &InputSource,
    reader: &mut R,
) -> Result<InitialBuffer, BootstrapError> {
    match input_source {
        InputSource::File(target_path) => load_path_initial_buffer(target_path),
        InputSource::Stdin => load_stdin_initial_buffer(reader),
        InputSource::Empty => {
            log::debug!("[bootstrap] starting with an empty buffer");
            Ok(InitialBuffer {
                target_path: None,
                text: String::new(),
                source: InitialBufferSource::Empty,
            })
        }
    }
}

pub(super) fn load_path_initial_buffer(
    target_path: &Path,
) -> Result<InitialBuffer, BootstrapError> {
    let started_at = Instant::now();
    log::debug!(
        "[bootstrap] inspecting target path before terminal enter: {}",
        target_path.display()
    );
    let metadata = match fs::metadata(target_path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            log::debug!(
                "[bootstrap] target path does not exist; opening a named empty buffer: {}",
                target_path.display()
            );
            return Ok(InitialBuffer {
                target_path: Some(target_path.to_path_buf()),
                text: String::new(),
                source: InitialBufferSource::NewFile,
            });
        }
        Err(error) => {
            log::debug!(
                "[bootstrap] target path metadata read failed before terminal enter: path={}, error={}",
                target_path.display(),
                error
            );
            return Err(BootstrapError::TargetReadFailed {
                path: target_path.to_path_buf(),
                message: error.to_string(),
            });
        }
    };

    if metadata.is_dir() {
        log::debug!(
            "[bootstrap][dired] target path is a directory; building startup listing: path={}",
            target_path.display()
        );
        let directory_buffer = read_directory_buffer_state_with_options(
            target_path,
            DirectoryBufferListingOptions::default(),
        )
        .map_err(|error| BootstrapError::TargetReadFailed {
            path: target_path.to_path_buf(),
            message: error.to_string(),
        })?;
        log::debug!(
            "[PERF][bootstrap][dired] startup directory listing read: path={}, entries={}, display_bytes={}, elapsed_ms={}",
            target_path.display(),
            directory_buffer.entries.len(),
            directory_buffer.display_text.len(),
            started_at.elapsed().as_millis()
        );
        return Ok(InitialBuffer {
            target_path: Some(target_path.to_path_buf()),
            text: directory_buffer.display_text,
            source: InitialBufferSource::Directory,
        });
    }

    log::debug!(
        "[bootstrap] loading target file contents before terminal enter: {}",
        target_path.display()
    );
    let text =
        fs::read_to_string(target_path).map_err(|error| BootstrapError::TargetReadFailed {
            path: target_path.to_path_buf(),
            message: error.to_string(),
        })?;
    log::debug!(
        "[PERF][bootstrap] target file read: path={}, bytes={}, elapsed_ms={}",
        target_path.display(),
        text.len(),
        started_at.elapsed().as_millis()
    );
    Ok(InitialBuffer {
        target_path: Some(target_path.to_path_buf()),
        text,
        source: InitialBufferSource::File,
    })
}

pub(super) fn load_stdin_initial_buffer<R: Read>(
    reader: &mut R,
) -> Result<InitialBuffer, BootstrapError> {
    let started_at = Instant::now();
    log::debug!("[bootstrap] reading startup buffer contents from stdin");
    let mut initial_text = String::new();
    reader
        .read_to_string(&mut initial_text)
        .map_err(|error| BootstrapError::StdinReadFailed {
            message: error.to_string(),
        })?;
    log::debug!(
        "[PERF][bootstrap] stdin read: bytes={}, elapsed_ms={}",
        initial_text.len(),
        started_at.elapsed().as_millis()
    );
    Ok(InitialBuffer {
        target_path: None,
        text: initial_text,
        source: InitialBufferSource::Stdin,
    })
}

pub(super) fn snapshot_from_light_snapshot(
    light: &CoreLightSnapshot,
    text: String,
) -> CoreSnapshot {
    CoreSnapshot {
        text,
        revision: light.revision,
        dirty: light.dirty,
        mode: light.mode,
        pending_input: light.pending_input.clone(),
        cursor_row: light.cursor_row,
        cursor_col: light.cursor_col,
        pending_host_actions: light.pending_host_actions,
        buffers: light.buffers.clone(),
        windows: light.windows.clone(),
        pum: light.pum.clone(),
    }
}

pub(super) fn apply_initial_cursor(
    core_bridge: &mut CoreBridge,
    initial_cursor: &InitialCursorPosition,
) {
    let command = match initial_cursor {
        InitialCursorPosition::None => return,
        InitialCursorPosition::End => "G".to_string(),
        InitialCursorPosition::Line(line_number) => format!("{line_number}G"),
    };
    log::debug!(
        "[bootstrap] applying initial cursor position: {:?} via command={}",
        initial_cursor,
        command
    );
    let _ = core_bridge.dispatch_key(&command);
}
