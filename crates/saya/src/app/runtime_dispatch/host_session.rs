//! `MainRuntimeHostSession`（TypeScript ランタイムのホスト側セッション実装）。

use super::*;

pub struct MainRuntimeHostSession<'a> {
    outcome: &'a mut crate::app::bootstrap::BootstrapOutcome,
    session_state: &'a mut crate::app::session::EditorSessionState,
    pub(super) runtime_input_prompt: Option<&'a mut Option<RuntimeInputPromptUiState>>,
    floating_window_manager: Option<&'a mut FloatingWindowManager>,
    completion_float_manager: Option<&'a mut CompletionFloatManager>,
    lsp_diagnostic_store: Option<&'a mut LspDiagnosticStore>,
    pub(super) terminal_float_manager: Option<&'a mut TerminalFloatManager>,
    pub(super) panel_manager: Option<&'a mut PanelManager>,
    lsif_bridge: Option<&'a LsifBridgeHandle>,
}

impl<'a> MainRuntimeHostSession<'a> {
    pub fn new(
        outcome: &'a mut crate::app::bootstrap::BootstrapOutcome,
        session_state: &'a mut crate::app::session::EditorSessionState,
    ) -> Self {
        Self {
            outcome,
            session_state,
            runtime_input_prompt: None,
            floating_window_manager: None,
            completion_float_manager: None,
            lsp_diagnostic_store: None,
            terminal_float_manager: None,
            panel_manager: None,
            lsif_bridge: None,
        }
    }

    pub fn new_with_lsp_session(
        outcome: &'a mut crate::app::bootstrap::BootstrapOutcome,
        session_state: &'a mut crate::app::session::EditorSessionState,
        lsif_bridge: Option<&'a LsifBridgeHandle>,
    ) -> Self {
        Self {
            outcome,
            session_state,
            runtime_input_prompt: None,
            floating_window_manager: None,
            completion_float_manager: None,
            lsp_diagnostic_store: None,
            terminal_float_manager: None,
            panel_manager: None,
            lsif_bridge,
        }
    }

    pub fn new_with_floating_windows(
        outcome: &'a mut crate::app::bootstrap::BootstrapOutcome,
        session_state: &'a mut crate::app::session::EditorSessionState,
        floating_window_manager: &'a mut FloatingWindowManager,
        completion_float_manager: &'a mut CompletionFloatManager,
        lsp_diagnostic_store: &'a mut LspDiagnosticStore,
        terminal_float_manager: &'a mut TerminalFloatManager,
        panel_manager: &'a mut PanelManager,
        lsif_bridge: Option<&'a LsifBridgeHandle>,
    ) -> Self {
        Self {
            outcome,
            session_state,
            runtime_input_prompt: None,
            floating_window_manager: Some(floating_window_manager),
            completion_float_manager: Some(completion_float_manager),
            lsp_diagnostic_store: Some(lsp_diagnostic_store),
            terminal_float_manager: Some(terminal_float_manager),
            panel_manager: Some(panel_manager),
            lsif_bridge,
        }
    }

    pub fn new_with_runtime_input(
        outcome: &'a mut crate::app::bootstrap::BootstrapOutcome,
        session_state: &'a mut crate::app::session::EditorSessionState,
        runtime_input_prompt: &'a mut Option<RuntimeInputPromptUiState>,
    ) -> Self {
        Self {
            outcome,
            session_state,
            runtime_input_prompt: Some(runtime_input_prompt),
            floating_window_manager: None,
            completion_float_manager: None,
            lsp_diagnostic_store: None,
            terminal_float_manager: None,
            panel_manager: None,
            lsif_bridge: None,
        }
    }
}

impl RuntimeHostSession for MainRuntimeHostSession<'_> {
    fn current_buffer_snapshot(&mut self) -> ReadonlyBufferSnapshot {
        let started_at = std::time::Instant::now();
        let snapshot = self.outcome.core_bridge.light_snapshot();
        let active_buffer_id = snapshot
            .buffers
            .iter()
            .find(|buffer| buffer.is_active)
            .map(|buffer| buffer.id as u64)
            .unwrap_or(1);
        let current_line_range = self.outcome.core_bridge.buffer_line_range(
            active_buffer_id as i32,
            snapshot.cursor_row,
            1,
        );
        let text_started_at = std::time::Instant::now();
        let text = self.outcome.core_bridge.buffer_text();
        log::debug!(
            "[PERF][main][runtime] fetched full current buffer snapshot text: buffer_id={}, bytes={}, elapsed_ms={}",
            active_buffer_id,
            text.len(),
            text_started_at.elapsed().as_millis()
        );
        let snapshot = ReadonlyBufferSnapshot {
            id: active_buffer_id,
            path: self.session_state.target_path().cloned(),
            line_count: current_line_range
                .as_ref()
                .map(|range| range.total_line_count)
                .unwrap_or(1),
            cursor_row: snapshot.cursor_row,
            cursor_col: snapshot.cursor_col,
            current_line: current_line_range
                .and_then(|range| range.lines.into_iter().next())
                .unwrap_or_default(),
            text,
        };
        log::debug!(
            "[PERF][main][runtime] built full current buffer snapshot: buffer_id={}, elapsed_ms={}",
            active_buffer_id,
            started_at.elapsed().as_millis()
        );
        snapshot
    }

    fn current_buffer_metadata_snapshot(&mut self) -> ReadonlyBufferSnapshot {
        let started_at = std::time::Instant::now();
        let snapshot = self.outcome.core_bridge.light_snapshot();
        let active_buffer = snapshot
            .buffers
            .iter()
            .find(|buffer| buffer.is_active)
            .cloned();
        let active_buffer_id = active_buffer
            .as_ref()
            .map(|buffer| buffer.id as u64)
            .unwrap_or(1);
        let current_line_range = self.outcome.core_bridge.buffer_line_range(
            active_buffer_id as i32,
            snapshot.cursor_row,
            1,
        );
        let snapshot = ReadonlyBufferSnapshot {
            id: active_buffer_id,
            path: self.session_state.target_path().cloned(),
            line_count: current_line_range
                .as_ref()
                .map(|range| range.total_line_count)
                .unwrap_or(1),
            cursor_row: snapshot.cursor_row,
            cursor_col: snapshot.cursor_col,
            current_line: current_line_range
                .and_then(|range| range.lines.into_iter().next())
                .unwrap_or_default(),
            text: String::new(),
        };
        log::debug!(
            "[PERF][main][runtime] built lightweight current buffer metadata snapshot: buffer_id={}, buffer_name={:?}, document_id={:?}, session_target_path={:?}, snapshot_path={:?}, cursor=({},{}), elapsed_ms={}",
            active_buffer_id,
            active_buffer.as_ref().map(|buffer| &buffer.name),
            active_buffer
                .as_ref()
                .and_then(|buffer| buffer.document_id.as_deref()),
            self.session_state.target_path(),
            snapshot.path,
            snapshot.cursor_row,
            snapshot.cursor_col,
            started_at.elapsed().as_millis()
        );
        snapshot
    }

    fn current_selection_snapshot(
        &mut self,
    ) -> Option<crate::runtime::live::ReadonlySelectionSnapshot> {
        let snapshot = self.outcome.core_bridge.light_snapshot();
        let active_buffer_id = snapshot
            .buffers
            .iter()
            .find(|buffer| buffer.is_active)
            .map(|buffer| buffer.id as u64)
            .unwrap_or(1);
        let selection = self.outcome.core_bridge.current_visual_selection()?;
        let line_count = selection
            .end_row
            .saturating_sub(selection.start_row)
            .saturating_add(1);
        let text = self
            .outcome
            .core_bridge
            .buffer_line_range(active_buffer_id as i32, selection.start_row, line_count)
            .map(|range| range.lines.join("\n"))
            .unwrap_or_default();
        let mode = match selection.mode {
            CoreMode::VisualLine => "visualLine",
            CoreMode::VisualBlock => "visualBlock",
            _ => "visual",
        }
        .to_string();
        Some(crate::runtime::live::ReadonlySelectionSnapshot {
            mode,
            start_line: selection.start_row,
            start_column: selection.start_col,
            end_line: selection.end_row,
            end_column: selection.end_col,
            text,
        })
    }

    fn current_window_snapshot(&mut self) -> ReadonlyWindowSnapshot {
        let snapshot = self.outcome.core_bridge.light_snapshot();
        let active_window_id = snapshot
            .active_window_id()
            .map(|window_id| window_id as u64)
            .expect("runtime current window should resolve from active window id");
        ReadonlyWindowSnapshot {
            id: active_window_id,
        }
    }

    fn open_float(
        &mut self,
        request: RuntimeFloatOpenRequest,
    ) -> Result<RuntimeFloatSnapshot, RuntimeCommandError> {
        execute_runtime_window_open_float(
            request,
            self.outcome,
            self.floating_window_manager.as_deref_mut(),
            self.terminal_float_manager.as_deref_mut(),
        )
    }

    fn close_float(&mut self, id: u64) -> Result<bool, RuntimeCommandError> {
        execute_runtime_window_close_float(
            id,
            self.floating_window_manager.as_deref_mut(),
            self.terminal_float_manager.as_deref_mut(),
        )
    }

    fn focus_float(&mut self, id: u64) -> Result<bool, RuntimeCommandError> {
        let manager = self.floating_window_manager.as_deref_mut().ok_or_else(|| {
            RuntimeCommandError::CommandFailed {
                name: "window.focus".to_string(),
                message: "floating window manager is not available".to_string(),
            }
        })?;
        Ok(manager.focus_float(FloatingWindowId(id)))
    }

    fn list_float_snapshots(&mut self) -> Result<Vec<RuntimeFloatSnapshot>, RuntimeCommandError> {
        let manager = self.floating_window_manager.as_deref().ok_or_else(|| {
            RuntimeCommandError::CommandFailed {
                name: "window.floats".to_string(),
                message: "floating window manager is not available".to_string(),
            }
        })?;
        Ok(runtime_float_snapshots(manager))
    }

    fn open_panel(
        &mut self,
        request: RuntimePanelOpenRequest,
    ) -> Result<RuntimePanelSnapshot, RuntimeCommandError> {
        execute_runtime_panel_open(
            request,
            self.panel_manager.as_deref_mut(),
            self.terminal_float_manager.as_deref_mut(),
        )
    }

    fn focus_panel(&mut self, id: String) -> Result<bool, RuntimeCommandError> {
        let manager = self.panel_manager.as_deref_mut().ok_or_else(|| {
            RuntimeCommandError::CommandFailed {
                name: "panel.focus".to_string(),
                message: "panel manager is not available".to_string(),
            }
        })?;
        Ok(manager.focus(&id))
    }

    fn unfocus_panel(&mut self) -> Result<bool, RuntimeCommandError> {
        let manager = self.panel_manager.as_deref_mut().ok_or_else(|| {
            RuntimeCommandError::CommandFailed {
                name: "panel.unfocus".to_string(),
                message: "panel manager is not available".to_string(),
            }
        })?;
        Ok(manager.unfocus())
    }

    fn close_panel(&mut self, id: String) -> Result<bool, RuntimeCommandError> {
        execute_runtime_panel_close(
            id,
            self.panel_manager.as_deref_mut(),
            self.terminal_float_manager.as_deref_mut(),
        )
    }

    fn list_panel_snapshots(&mut self) -> Result<Vec<RuntimePanelSnapshot>, RuntimeCommandError> {
        let manager =
            self.panel_manager
                .as_deref()
                .ok_or_else(|| RuntimeCommandError::CommandFailed {
                    name: "panel.list".to_string(),
                    message: "panel manager is not available".to_string(),
                })?;
        Ok(manager
            .snapshots()
            .into_iter()
            .map(runtime_panel_snapshot)
            .collect())
    }

    fn send_panel_text(&mut self, id: String, text: String) -> Result<bool, RuntimeCommandError> {
        let manager = self.panel_manager.as_deref_mut().ok_or_else(|| {
            RuntimeCommandError::CommandFailed {
                name: "panel.send".to_string(),
                message: "panel manager is not available".to_string(),
            }
        })?;
        let terminal_id =
            manager
                .send(&id, &text)
                .map_err(|message| RuntimeCommandError::CommandFailed {
                    name: "panel.send".to_string(),
                    message,
                })?;
        if let Some(terminal_id) = terminal_id {
            let terminal_manager = self.terminal_float_manager.as_deref_mut().ok_or_else(|| {
                RuntimeCommandError::CommandFailed {
                    name: "panel.send".to_string(),
                    message: "terminal manager is not available".to_string(),
                }
            })?;
            terminal_manager
                .write_bytes(terminal_id, text.as_bytes())
                .map_err(|error| RuntimeCommandError::CommandFailed {
                    name: "panel.send".to_string(),
                    message: format!("failed to send panel terminal input: {error:?}"),
                })?;
        }
        Ok(terminal_id.is_some())
    }

    fn current_editor_snapshot(&mut self) -> ReadonlyEditorSnapshot {
        let snapshot = self.outcome.core_bridge.light_snapshot();
        ReadonlyEditorSnapshot {
            mode: runtime_mode_from_core(snapshot.mode),
        }
    }

    fn current_filer_entry(
        &mut self,
    ) -> Result<Option<RuntimeFilerCurrentEntry>, crate::runtime::live::RuntimeFilerError> {
        let Some(directory_buffer) = self.session_state.directory_buffer() else {
            log::debug!(
                "[main][runtime] current filer entry requested outside directory buffer: target_path={:?}",
                self.session_state.target_path()
            );
            return Ok(None);
        };
        let snapshot = self.outcome.core_bridge.light_snapshot();
        let Some(entry) = self
            .session_state
            .current_directory_entry(snapshot.cursor_row)
        else {
            log::debug!(
                "[main][runtime] current filer entry missing for cursor row: root_path={}, cursor_row={}, entries={}",
                directory_buffer.root_path.display(),
                snapshot.cursor_row,
                directory_buffer.entries.len()
            );
            return Ok(None);
        };
        log::debug!(
            "[main][runtime] current filer entry resolved: root_path={}, cursor_row={}, entry_id={}, name={}, path={}, kind={:?}",
            directory_buffer.root_path.display(),
            snapshot.cursor_row,
            entry.id,
            entry.name,
            entry.path.display(),
            entry.kind
        );
        Ok(Some(RuntimeFilerCurrentEntry {
            id: entry.id,
            name: entry.name.clone(),
            path: entry.path.to_string_lossy().into_owned(),
            kind: runtime_filer_kind_from_directory_entry(entry.kind),
            root_path: directory_buffer.root_path.to_string_lossy().into_owned(),
            display_text: entry.display_text.clone(),
        }))
    }

    fn list_filer_entries(
        &mut self,
        path: std::path::PathBuf,
        options: RuntimeFilerListOptions,
    ) -> Result<Vec<RuntimeFilerEntry>, RuntimeFilerError> {
        log::debug!(
            "[main][runtime][filer] list requested through host session: path={}, show_hidden={}, sort_by={:?}, filter={:?}",
            path.display(),
            options.show_hidden,
            options.sort_by,
            options.filter
        );
        let listing_options = directory_buffer_listing_options_from_runtime(options);
        let entries = self
            .session_state
            .refresh_directory_buffer_listing(path.clone(), listing_options)
            .map_err(|error| RuntimeFilerError::ReadFailed {
                path: path.clone(),
                message: error.to_string(),
            })?;
        let display_text = self
            .session_state
            .directory_buffer()
            .map(|directory_buffer| directory_buffer.display_text.clone())
            .unwrap_or_default();
        self.outcome
            .core_bridge
            .replace_buffer_text(&display_text)
            .map_err(|error| RuntimeFilerError::ReadFailed {
                path: path.clone(),
                message: format!("failed to project filer listing into buffer: {error:?}"),
            })?;
        self.outcome.target_path = Some(path.clone());
        log::debug!(
            "[main][runtime][filer] projected directory listing into active buffer: path={}, entries={}, text_len={}",
            path.display(),
            entries.len(),
            display_text.len()
        );
        Ok(directory_entries_for_runtime_entries(entries))
    }

    fn execute_filer_operation(
        &mut self,
        operation: RuntimeFilerOperation,
    ) -> Result<RuntimeFilerOperationReport, RuntimeFilerError> {
        execute_runtime_filer_operation(operation, self.outcome, self.session_state)
    }

    fn execute_host_command(
        &mut self,
        name: &str,
    ) -> Result<RuntimeCommandEffect, RuntimeCommandError> {
        log::info!(
            "[main][host_command] executing runtime host command through application session owner: {}",
            name
        );
        execute_runtime_host_command_with_floats(
            name,
            self.outcome,
            self.session_state,
            self.floating_window_manager.as_deref_mut(),
            self.completion_float_manager.as_deref_mut(),
            self.lsp_diagnostic_store.as_deref_mut(),
            self.terminal_float_manager.as_deref_mut(),
        )
    }

    fn request_input_prompt(
        &mut self,
        request: RuntimeInputPromptRequest,
    ) -> Result<RuntimeInputPromptHostResponse, RuntimeCommandError> {
        let Some(slot) = self.runtime_input_prompt.as_deref_mut() else {
            log::debug!(
                "[main][runtime_input] prompt requested without active TUI prompt slot: title={}",
                request.title
            );
            return Ok(RuntimeInputPromptHostResponse::Completed(
                RuntimeInputPromptResponse::Cancelled,
            ));
        };
        if slot.is_some() {
            return Err(RuntimeCommandError::CommandFailed {
                name: "input.prompt".to_string(),
                message: "another runtime input prompt is already active".to_string(),
            });
        }
        log::info!(
            "[main][runtime_input] prompt start: title={}, placeholder_present={}",
            request.title,
            request.placeholder.is_some()
        );
        *slot = Some(RuntimeInputPromptUiState::new(request));
        Ok(RuntimeInputPromptHostResponse::Pending)
    }

    fn execute_lsif_request(
        &mut self,
        request: LspRuntimeBridgeRequest,
    ) -> Result<LspRuntimeBridgeResponse, RuntimeCommandError> {
        let Some(bridge) = self.lsif_bridge else {
            return Err(RuntimeCommandError::CommandFailed {
                name: "lsif.request".to_string(),
                message: format!(
                    "LSIF bridge is not configured for runtime method {}",
                    request.method
                ),
            });
        };
        log::info!(
            "[main][lsif] executing runtime LSIF request through index cache: method={}, language={}, document={}",
            request.method,
            request.language_id,
            request
                .text_document
                .as_ref()
                .map(|document| document.uri.as_str())
                .unwrap_or("<none>")
        );
        bridge
            .cache
            .lock()
            .map_err(|_| RuntimeCommandError::CommandFailed {
                name: "lsif.request".to_string(),
                message: "LSIF index cache mutex poisoned".to_string(),
            })
            .and_then(|mut cache| cache.execute_request(request, &bridge.diagnostic_events))
    }

    fn show_completion(
        &mut self,
        request: CompletionShowRequest,
    ) -> Result<bool, RuntimeCommandError> {
        let floating_window_manager =
            self.floating_window_manager.as_deref_mut().ok_or_else(|| {
                RuntimeCommandError::CommandFailed {
                    name: "completion.show".to_string(),
                    message: "floating window manager is not available".to_string(),
                }
            })?;
        let completion_manager = self
            .completion_float_manager
            .as_deref_mut()
            .ok_or_else(|| RuntimeCommandError::CommandFailed {
                name: "completion.show".to_string(),
                message: "completion manager is not available".to_string(),
            })?;
        let snapshot = self.outcome.core_bridge.light_snapshot();
        let window_id = snapshot.active_window_id().unwrap_or(1);
        let shown = completion_manager.show_typed(
            floating_window_manager,
            window_id,
            snapshot.cursor_row,
            snapshot.cursor_col,
            request,
        );
        log::debug!(
            "[main][completion] typed completion show applied: window_id={}, cursor=({},{}), shown={}",
            window_id,
            snapshot.cursor_row,
            snapshot.cursor_col,
            shown
        );
        Ok(shown)
    }

    fn close_completion(&mut self) -> Result<bool, RuntimeCommandError> {
        let floating_window_manager =
            self.floating_window_manager.as_deref_mut().ok_or_else(|| {
                RuntimeCommandError::CommandFailed {
                    name: "completion.close".to_string(),
                    message: "floating window manager is not available".to_string(),
                }
            })?;
        let completion_manager = self
            .completion_float_manager
            .as_deref_mut()
            .ok_or_else(|| RuntimeCommandError::CommandFailed {
                name: "completion.close".to_string(),
                message: "completion manager is not available".to_string(),
            })?;
        let snapshot = self.outcome.core_bridge.light_snapshot();
        let restore_window_id = snapshot.active_window_id();
        let closed = completion_manager.close(floating_window_manager, restore_window_id);
        log::debug!(
            "[main][completion] typed completion close applied: restore_window_id={:?}, closed={}",
            restore_window_id,
            closed
        );
        Ok(closed)
    }
}
