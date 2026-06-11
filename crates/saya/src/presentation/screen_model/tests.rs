use std::path::PathBuf;

use vim_core_rs::{
    CoreBufferInfo, CoreBufferRevision, CoreBufferSourceKind, CoreInputRequestKind, CoreMode,
    CorePendingInput, CoreSnapshot, CoreSyntaxChunk, CoreWindowInfo,
};

use super::*;
use crate::core::bridge::CoreBridge;
use crate::core::notification_prompt::InputPromptStatus;
use crate::features::search::capability::SearchCapabilityContract;
use crate::features::search::query::{
    SearchMatch, SearchMatchKind, SearchQueryMode, SearchVisibleRows, SearchVisibleState,
};

use crate::support::session_guard::test_lock as session_test_lock;

// ---- タスク 6.1: file name と mode を描画モデルへ投影するテスト ----

#[test]
fn projects_file_name_from_session_target_path() {
    let _lock = session_test_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());

    let bridge = CoreBridge::new("hello\n").expect("core bridge");
    let snapshot = bridge.snapshot();
    let session_state = EditorSessionState::new(Some(PathBuf::from("/tmp/hello.txt")));

    let model = project(&ProjectionInput::new(&snapshot, &session_state, None));

    assert_eq!(
        model.file_name, "/tmp/hello.txt",
        "session の target_path がファイル名として投影されること"
    );
}

#[test]
fn projects_default_file_name_when_no_target_path() {
    let _lock = session_test_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());

    let bridge = CoreBridge::new("").expect("core bridge");
    let snapshot = bridge.snapshot();
    let session_state = EditorSessionState::new(None);

    let model = project(&ProjectionInput::new(&snapshot, &session_state, None));

    assert_eq!(
        model.file_name, "[新規]",
        "ターゲットパスなしの場合はデフォルト名が使われること"
    );
}

#[test]
fn projects_normal_mode_label() {
    let _lock = session_test_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());

    let bridge = CoreBridge::new("text\n").expect("core bridge");
    let snapshot = bridge.snapshot();
    assert_eq!(snapshot.mode, CoreMode::Normal);

    let session_state = EditorSessionState::new(None);
    let model = project(&ProjectionInput::new(&snapshot, &session_state, None));

    assert_eq!(
        model.mode_label, "NORMAL",
        "ノーマルモードのラベルが NORMAL であること"
    );
}

#[test]
fn projects_insert_mode_label() {
    let _lock = session_test_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());

    let mut bridge = CoreBridge::new("text\n").expect("core bridge");
    bridge.dispatch_key("i").expect("insert mode");
    let snapshot = bridge.snapshot();
    assert_eq!(snapshot.mode, CoreMode::Insert);

    let session_state = EditorSessionState::new(None);
    let model = project(&ProjectionInput::new(&snapshot, &session_state, None));

    assert_eq!(
        model.mode_label, "INSERT",
        "インサートモードのラベルが INSERT であること"
    );
}

#[test]
fn projects_cursor_style_from_core_mode() {
    let _lock = session_test_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());

    let bridge = CoreBridge::new("text\n").expect("core bridge");
    let base_snapshot = bridge.snapshot();
    let session_state = EditorSessionState::new(None);

    let cases = [
        (CoreMode::Normal, ScreenCursorStyle::Block),
        (CoreMode::Insert, ScreenCursorStyle::SteadyBar),
        (CoreMode::Replace, ScreenCursorStyle::UnderScore),
        (CoreMode::Visual, ScreenCursorStyle::Block),
        (CoreMode::CommandLine, ScreenCursorStyle::SteadyBar),
    ];

    for (mode, expected_style) in cases {
        let mut snapshot = base_snapshot.clone();
        snapshot.mode = mode;
        let model = project(&ProjectionInput::new(&snapshot, &session_state, None));
        assert_eq!(
            model.cursor_style, expected_style,
            "mode {mode:?} should project cursor style {expected_style:?}"
        );
    }
}

#[test]
fn active_cursor_style_prefers_command_line_overlay() {
    let pane = ScreenModel {
        window_id: 1,
        buffer_id: 1,
        rect: PaneRect {
            x: 0,
            y: 0,
            width: 20,
            height: 4,
        },
        file_name: "sample.txt".to_string(),
        mode_label: "NORMAL".to_string(),
        status_line: "test.txt | NORMAL".to_string(),
        cursor_style: ScreenCursorStyle::Block,
        dirty: false,
        lines: vec!["alpha".to_string()],
        line_projections: vec![],
        cursor_row: 0,
        cursor_col: 0,
        visual_selection: None,
        search_overlays: vec![],
        syntax_chunks: vec![],
        markdown_style_ranges: vec![],
        filer_style_ranges: vec![],
        resolved_theme: crate::presentation::theme::ResolvedTheme::default(),
        message_line: None,
        command_cursor_col: None,
        is_active: true,
    };
    let mut workspace = WorkspaceScreenModel {
        panes: vec![pane],
        floats: vec![],
        active_window_id: 1,
        message_line: resolve_workspace_message_line(Vec::<MessageLineCandidate>::new()),
        message_area_height: 5,
        message_scroll_offset: 0,
        prompt_line: None,
        pager_prompt: None,
        suppressed_prompt_hints: vec![],
        bell: None,
        command_line: None,
    };

    assert_eq!(workspace.active_cursor_style(), ScreenCursorStyle::Block);

    workspace.command_line = Some(CommandLineModel {
        text: ":write".to_string(),
        cursor_col: 6,
    });

    assert_eq!(
        workspace.active_cursor_style(),
        ScreenCursorStyle::SteadyBar
    );
}

#[test]
fn file_name_and_mode_are_never_empty_at_startup() {
    let _lock = session_test_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());

    let bridge = CoreBridge::new("").expect("core bridge");
    let snapshot = bridge.snapshot();
    let session_state = EditorSessionState::new(None);

    let model = project(&ProjectionInput::new(&snapshot, &session_state, None));

    assert!(
        !model.file_name.is_empty(),
        "起動直後でもファイル名は空でないこと"
    );
    assert!(
        !model.mode_label.is_empty(),
        "起動直後でもモードラベルは空でないこと"
    );
}

// ---- タスク 6.2: dirty 状態とカーソル位置を描画モデルへ投影するテスト ----

#[test]
fn projects_dirty_false_for_clean_buffer() {
    let _lock = session_test_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());

    let bridge = CoreBridge::new("clean\n").expect("core bridge");
    let snapshot = bridge.snapshot();
    let session_state = EditorSessionState::new(None);

    let model = project(&ProjectionInput::new(&snapshot, &session_state, None));

    assert!(!model.dirty, "未編集バッファは dirty=false であること");
}

#[test]
fn projects_dirty_true_after_edit() {
    let _lock = session_test_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());

    let mut bridge = CoreBridge::new("text\n").expect("core bridge");
    bridge.dispatch_key("i").expect("insert mode");
    bridge.dispatch_key("X").expect("insert X");
    bridge.dispatch_key("\x1b").expect("normal mode");
    let snapshot = bridge.snapshot();
    assert!(snapshot.dirty);

    let session_state = EditorSessionState::new(None);
    let model = project(&ProjectionInput::new(&snapshot, &session_state, None));

    assert!(model.dirty, "編集後のバッファは dirty=true であること");
}

#[test]
fn projects_cursor_position_at_origin() {
    let _lock = session_test_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());

    let bridge = CoreBridge::new("abc\n").expect("core bridge");
    let snapshot = bridge.snapshot();
    let session_state = EditorSessionState::new(None);

    let model = project(&ProjectionInput::new(&snapshot, &session_state, None));

    assert_eq!(model.cursor_row, 0, "初期カーソル行は 0");
    assert_eq!(model.cursor_col, 0, "初期カーソル列は 0");
}

#[test]
fn projects_cursor_position_after_movement() {
    let _lock = session_test_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());

    let mut bridge = CoreBridge::new("abcde\nfghij\n").expect("core bridge");
    bridge.dispatch_key("jll").expect("j, ll for movement");
    let snapshot = bridge.snapshot();
    let session_state = EditorSessionState::new(None);

    let model = project(&ProjectionInput::new(&snapshot, &session_state, None));

    assert_eq!(model.cursor_row, 1, "カーソル行が移動後に反映されること");
    assert_eq!(model.cursor_col, 2, "カーソル列が移動後に反映されること");
}

#[test]
fn projects_visible_slice_and_relative_cursor_row_when_viewport_applied() {
    let _lock = session_test_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());

    let mut bridge = CoreBridge::new("line1\nline2\nline3\nline4\nline5\n").expect("core bridge");
    bridge.dispatch_key("jjj").expect("move to fourth line");
    let snapshot = bridge.snapshot();
    let session_state = EditorSessionState::new(None);

    let model = project(&ProjectionInput::new(&snapshot, &session_state, None).with_viewport(2, 2));

    assert_eq!(model.lines, vec!["line3", "line4"]);
    assert_eq!(model.cursor_row, 1, "viewport 内の相対行へ変換されること");
}

#[test]
fn projects_syntax_chunks_to_visible_display_columns_without_changing_line_text() {
    let _lock = session_test_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());

    let bridge = CoreBridge::new("fn\tmain\n").expect("core bridge");
    let snapshot = bridge.snapshot();
    let session_state = EditorSessionState::new_with_tab_size_and_line_numbers(None, 4, true);
    let mut syntax_lines = BTreeMap::new();
    syntax_lines.insert(
        0,
        vec![CoreSyntaxChunk {
            start_col: 3,
            end_col: 7,
            syn_id: 11,
            name: Some("Identifier".to_string()),
        }],
    );

    let model = project(
        &ProjectionInput::new(&snapshot, &session_state, None)
            .with_syntax_lines(Some(&syntax_lines)),
    );

    assert_eq!(
        model.lines,
        vec!["   1 fn  main"],
        "syntax projection must not change rendered text"
    );
    assert_eq!(
        model.syntax_chunks,
        vec![ScreenSyntaxChunk {
            row: 0,
            start_col: 9,
            end_col_exclusive: 13,
            syn_id: 11,
            name: Some("Identifier".to_string()),
            language: None,
            tree_sitter: None,
        }]
    );
}

#[test]
fn projects_syntax_chunks_through_markdown_rich_display_mapping() {
    let _lock = session_test_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());

    let source = "# Title\n";
    let bridge = CoreBridge::new(source).expect("core bridge");
    let snapshot = bridge.snapshot();
    let session_state = EditorSessionState::new(None);
    let markdown_map = MarkdownDocumentMap::parse(source);
    let mut syntax_lines = BTreeMap::new();
    syntax_lines.insert(
        0,
        vec![CoreSyntaxChunk {
            start_col: 2,
            end_col: 7,
            syn_id: 11,
            name: Some("Title".to_string()),
        }],
    );
    let mut input = ProjectionInput::new(&snapshot, &session_state, None)
        .with_markdown_document_map(Some(&markdown_map))
        .with_syntax_lines(Some(&syntax_lines));
    input.is_active = false;

    let model = project(&input);

    assert_eq!(model.line_projections[0].display_text, "Title");
    assert_eq!(
        model.syntax_chunks,
        vec![ScreenSyntaxChunk {
            row: 0,
            start_col: 0,
            end_col_exclusive: 5,
            syn_id: 11,
            name: Some("Title".to_string()),
            language: None,
            tree_sitter: None,
        }],
        "syntax chunks should be projected through Markdown rich display-space"
    );
}

#[test]
fn projects_syntax_chunk_language_from_document_id_when_buffer_name_is_stale() {
    let _lock = session_test_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());

    let bridge = CoreBridge::new("fn main() {}\n").expect("core bridge");
    let mut snapshot = bridge.snapshot();
    snapshot.buffers[0].name = "/tmp/project".to_string();
    snapshot.buffers[0].document_id = Some("file:///tmp/project/src/main.rs".to_string());
    let session_state = EditorSessionState::new(None);
    let mut syntax_lines = BTreeMap::new();
    syntax_lines.insert(
        0,
        vec![CoreSyntaxChunk {
            start_col: 0,
            end_col: 2,
            syn_id: 11,
            name: Some("Statement".to_string()),
        }],
    );

    let model = project(
        &ProjectionInput::new(&snapshot, &session_state, None)
            .with_syntax_lines(Some(&syntax_lines)),
    );

    assert_eq!(
        model
            .syntax_chunks
            .first()
            .and_then(|chunk| chunk.language.as_deref()),
        Some("rust"),
        "syntax chunk language should use document_id file path before stale buffer.name"
    );
}

#[test]
fn projects_markdown_fenced_code_syntax_with_embedded_language_metadata() {
    let _lock = session_test_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());

    let source = "```go\nfunc main() {}\n```\n";
    let bridge = CoreBridge::new(source).expect("core bridge");
    let snapshot = bridge.snapshot();
    let session_state = EditorSessionState::new(None);
    let markdown_map = MarkdownDocumentMap::parse(source);
    let mut syntax_lines = BTreeMap::new();
    syntax_lines.insert(
        1,
        vec![CoreSyntaxChunk {
            start_col: 0,
            end_col: 4,
            syn_id: 11,
            name: Some("Function".to_string()),
        }],
    );
    let mut input = ProjectionInput::new(&snapshot, &session_state, None)
        .with_markdown_document_map(Some(&markdown_map))
        .with_syntax_lines(Some(&syntax_lines));
    input.is_active = false;

    let model = project(&input);

    assert_eq!(model.line_projections[1].display_text, "func main() {}");
    assert_eq!(
        model.syntax_chunks[0].language.as_deref(),
        Some("go"),
        "syntax chunks inside ```go fenced code should carry embedded language metadata"
    );
}

#[test]
fn projects_filer_entry_kind_and_marked_styles_from_directory_metadata() {
    let _lock = session_test_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());

    let root = std::env::temp_dir().join(format!("saya-filer-theme-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("src")).expect("mkdir");
    std::fs::write(root.join("README.md"), "hello").expect("file");
    let bridge = CoreBridge::new("src/\nREADME.md\n").expect("core bridge");
    let mut snapshot = bridge.snapshot();
    snapshot.buffers[0].name = root.display().to_string();
    let mut session_state = EditorSessionState::new(Some(root.clone()));
    let directory = session_state
        .directory_buffer()
        .expect("directory buffer should initialize")
        .clone();
    let src = directory
        .entries
        .iter()
        .find(|entry| entry.name == "src")
        .expect("src entry")
        .clone();
    session_state.mark_directory_entry(&src);

    let model = project(&ProjectionInput::new(&snapshot, &session_state, None));

    assert!(
        model
            .filer_style_ranges
            .iter()
            .any(|range| { range.row == 1 && range.key == FilerSemanticStyleKey::File })
    );
    assert!(
        model
            .filer_style_ranges
            .iter()
            .any(|range| { range.row == 0 && range.key == FilerSemanticStyleKey::Directory })
    );
    assert!(
        model
            .filer_style_ranges
            .iter()
            .any(|range| { range.row == 0 && range.key == FilerSemanticStyleKey::Marked })
    );
    std::fs::remove_dir_all(root).expect("cleanup");
}

#[test]
fn projects_filer_styles_only_for_matching_directory_buffer_pane() {
    let _lock = session_test_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());

    let root =
        std::env::temp_dir().join(format!("saya-filer-pane-isolation-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("src")).expect("mkdir");
    std::fs::write(root.join("AGENTS.md"), "# Agents\n").expect("file");
    let bridge = CoreBridge::new("# AGENTS.md\nsrc/\n").expect("core bridge");
    let mut snapshot = bridge.snapshot();
    snapshot.buffers[0].name = root.join("AGENTS.md").display().to_string();
    let session_state = EditorSessionState::new(Some(root.clone()));

    let model = project(&ProjectionInput::new(&snapshot, &session_state, None));

    assert!(
        model.filer_style_ranges.is_empty(),
        "regular markdown panes should not inherit dired styles from another buffer"
    );
    std::fs::remove_dir_all(root).expect("cleanup");
}

#[test]
fn projects_filer_styles_after_directory_root_changes_with_stale_buffer_name() {
    let _lock = session_test_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());

    let root = std::env::temp_dir().join(format!("saya-filer-up-{}", std::process::id()));
    let child = root.join("child");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&child).expect("mkdir");
    let bridge = CoreBridge::new("child/\n").expect("core bridge");
    let mut snapshot = bridge.snapshot();
    snapshot.buffers[0].name = child.display().to_string();
    let session_state = EditorSessionState::new(Some(root.clone()));
    let line_range = CoreBufferLineRange {
        buffer_id: 1,
        source_revision: CoreBufferRevision { value: 1 },
        start_row: 0,
        line_count: 1,
        total_line_count: 1,
        lines: vec!["child/".to_string()],
    };
    let mut input =
        ProjectionInput::new(&snapshot, &session_state, None).with_line_range(Some(&line_range));
    input.is_active = false;

    let model = project(&input);

    assert!(
        model
            .filer_style_ranges
            .iter()
            .any(|range| range.row == 0 && range.key == FilerSemanticStyleKey::Directory),
        "dired listing should keep filer styles after moving to a parent directory"
    );
    std::fs::remove_dir_all(root).expect("cleanup");
}

#[test]
fn projects_syntax_chunks_against_raw_active_markdown_rows() {
    let _lock = session_test_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());

    let source = "# Title\n";
    let bridge = CoreBridge::new(source).expect("core bridge");
    let snapshot = bridge.snapshot();
    let session_state = EditorSessionState::new(None);
    let markdown_map = MarkdownDocumentMap::parse(source);
    let mut syntax_lines = BTreeMap::new();
    syntax_lines.insert(
        0,
        vec![CoreSyntaxChunk {
            start_col: 2,
            end_col: 7,
            syn_id: 11,
            name: Some("Title".to_string()),
        }],
    );

    let model = project(
        &ProjectionInput::new(&snapshot, &session_state, None)
            .with_markdown_document_map(Some(&markdown_map))
            .with_syntax_lines(Some(&syntax_lines)),
    );

    assert_eq!(model.line_projections[0].display_text, "# Title");
    assert_eq!(
        model.syntax_chunks,
        vec![ScreenSyntaxChunk {
            row: 0,
            start_col: 2,
            end_col_exclusive: 7,
            syn_id: 11,
            name: Some("Title".to_string()),
            language: None,
            tree_sitter: None,
        }],
        "active Markdown rows should keep syntax chunks aligned with raw text"
    );
}

#[cfg(feature = "tree-sitter-syntax")]
#[test]
fn projects_prepared_tree_sitter_chunks_without_core_syntax_chunks() {
    use vim_core_rs::{
        CoreSyntaxCategory, CoreSyntaxModifier, CoreTextPosition, CoreTextRange,
        CoreTreeSitterBudgetStatus, CoreTreeSitterChunk, CoreTreeSitterProvenance,
        CoreTreeSitterRangeSyntax, CoreTreeSitterStatus,
    };

    let _lock = session_test_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());

    let bridge = CoreBridge::new("fn main() {}\n").expect("core bridge");
    let snapshot = bridge.snapshot();
    let active_buffer = snapshot
        .buffers
        .iter()
        .find(|buffer| buffer.is_active)
        .expect("active buffer");
    let session_state = EditorSessionState::new(None);
    let covered_range = CoreTextRange {
        start: CoreTextPosition { row: 0, col: 0 },
        end: CoreTextPosition {
            row: usize::MAX,
            col: usize::MAX,
        },
    };
    let syntax = CoreTreeSitterRangeSyntax {
        buffer_id: active_buffer.id,
        source_revision: active_buffer.source_revision,
        provenance: CoreTreeSitterProvenance {
            language_id: "rust".to_string(),
            package_id: "tree-sitter-rust".to_string(),
            package_version: "0.24.2".to_string(),
            parser_version: "14".to_string(),
            query_version: "saya-test".to_string(),
        },
        status: CoreTreeSitterStatus::Prepared,
        has_error: false,
        covered_ranges: vec![covered_range],
        error_ranges: vec![],
        budget_status: CoreTreeSitterBudgetStatus::WithinBudget,
        chunks: vec![CoreTreeSitterChunk {
            range: CoreTextRange {
                start: CoreTextPosition { row: 0, col: 0 },
                end: CoreTextPosition { row: 0, col: 2 },
            },
            capture_name: "keyword".to_string(),
            category: CoreSyntaxCategory::Keyword,
            modifiers: vec![CoreSyntaxModifier::Definition],
        }],
        embedded_regions: vec![],
    };

    let model = project(
        &ProjectionInput::new(&snapshot, &session_state, None)
            .with_tree_sitter_syntax(Some(&syntax)),
    );

    assert_eq!(
        model.syntax_chunks,
        vec![ScreenSyntaxChunk {
            row: 0,
            start_col: 0,
            end_col_exclusive: 2,
            syn_id: 0,
            name: None,
            language: Some("rust".to_string()),
            tree_sitter: Some(ScreenTreeSitterSyntax {
                category: ScreenSyntaxCategory::Keyword,
                modifiers: vec![ScreenSyntaxModifier::Definition],
                capture_name: "keyword".to_string(),
            }),
        }],
        "Tree-sitter render data must stay separate from Vim CoreSyntaxChunk"
    );
}

#[cfg(feature = "tree-sitter-syntax")]
#[test]
fn projects_embedded_tree_sitter_chunks_with_fenced_language_metadata() {
    use vim_core_rs::{
        CoreEmbeddedBlockKind, CoreEmbeddedRegion, CoreEmbeddedRegionSource,
        CoreLanguageResolutionSource, CoreLanguageResolutionStatus, CoreLanguageRole,
        CoreResolutionConfidence, CoreResolvedLanguage, CoreSyntaxCategory, CoreSyntaxModifier,
        CoreTextPosition, CoreTextRange, CoreTreeSitterBudgetStatus, CoreTreeSitterChunk,
        CoreTreeSitterProvenance, CoreTreeSitterRangeSyntax, CoreTreeSitterStatus,
    };

    let _lock = session_test_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());

    let source = "```typescript\nfunction add(a: number): number { return a; }\n```\n";
    let bridge = CoreBridge::new(source).expect("core bridge");
    let snapshot = bridge.snapshot();
    let active_buffer = snapshot
        .buffers
        .iter()
        .find(|buffer| buffer.is_active)
        .expect("active buffer");
    let session_state = EditorSessionState::new(None);
    let markdown_map = MarkdownDocumentMap::parse(source);
    let visible_range = CoreTextRange {
        start: CoreTextPosition { row: 0, col: 0 },
        end: CoreTextPosition { row: 3, col: 0 },
    };
    let content_range = CoreTextRange {
        start: CoreTextPosition { row: 1, col: 0 },
        end: CoreTextPosition { row: 2, col: 0 },
    };
    let syntax = CoreTreeSitterRangeSyntax {
        buffer_id: active_buffer.id,
        source_revision: active_buffer.source_revision,
        provenance: CoreTreeSitterProvenance {
            language_id: "markdown".to_string(),
            package_id: "tree-sitter-markdown".to_string(),
            package_version: "tree-sitter-md-0.5.3".to_string(),
            parser_version: "tree-sitter-md-block-0.5.3".to_string(),
            query_version: "saya-test".to_string(),
        },
        status: CoreTreeSitterStatus::Prepared,
        has_error: false,
        covered_ranges: vec![visible_range],
        error_ranges: vec![],
        budget_status: CoreTreeSitterBudgetStatus::WithinBudget,
        chunks: vec![CoreTreeSitterChunk {
            range: CoreTextRange {
                start: CoreTextPosition { row: 1, col: 0 },
                end: CoreTextPosition { row: 1, col: 8 },
            },
            capture_name: "keyword".to_string(),
            category: CoreSyntaxCategory::Keyword,
            modifiers: vec![CoreSyntaxModifier::Definition],
        }],
        embedded_regions: vec![CoreEmbeddedRegion {
            range: visible_range,
            content_range,
            source: CoreEmbeddedRegionSource::MarkdownFence,
            raw_info_string: Some("typescript".to_string()),
            normalized_info_string: Some("typescript".to_string()),
            normalized_kind: CoreEmbeddedBlockKind::Syntax,
            resolved_language: Some(CoreResolvedLanguage {
                range: visible_range,
                role: CoreLanguageRole::EmbeddedRegion,
                status: CoreLanguageResolutionStatus::Resolved,
                language_id: Some("typescript".to_string()),
                package_id: Some("tree-sitter-typescript".to_string()),
                package_version: Some("0.23.2".to_string()),
                kind: CoreEmbeddedBlockKind::Syntax,
                confidence: CoreResolutionConfidence::Exact,
                source: CoreLanguageResolutionSource::MarkdownInfoString,
            }),
        }],
    };

    let model = project(
        &ProjectionInput::new(&snapshot, &session_state, None)
            .with_markdown_document_map(Some(&markdown_map))
            .with_tree_sitter_syntax(Some(&syntax)),
    );

    let embedded_chunk = model
        .syntax_chunks
        .iter()
        .find(|chunk| chunk.tree_sitter.is_some())
        .expect("embedded Tree-sitter chunk should project");
    assert_eq!(
        embedded_chunk.language.as_deref(),
        Some("typescript"),
        "embedded fenced-code Tree-sitter chunks must use the fence language, not the Markdown root language"
    );
}

#[cfg(feature = "tree-sitter-syntax")]
#[test]
fn skips_tree_sitter_chunks_when_result_is_not_fresh_prepared_data() {
    use vim_core_rs::{
        CoreBufferRevision, CoreSyntaxCategory, CoreTextPosition, CoreTextRange,
        CoreTreeSitterBudgetStatus, CoreTreeSitterChunk, CoreTreeSitterProvenance,
        CoreTreeSitterRangeSyntax, CoreTreeSitterStatus,
    };

    let _lock = session_test_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());

    let bridge = CoreBridge::new("fn main() {}\n").expect("core bridge");
    let snapshot = bridge.snapshot();
    let active_buffer = snapshot
        .buffers
        .iter()
        .find(|buffer| buffer.is_active)
        .expect("active buffer");
    let session_state = EditorSessionState::new(None);
    let covered_range = CoreTextRange {
        start: CoreTextPosition { row: 0, col: 0 },
        end: CoreTextPosition {
            row: usize::MAX,
            col: usize::MAX,
        },
    };
    let base_syntax = CoreTreeSitterRangeSyntax {
        buffer_id: active_buffer.id,
        source_revision: active_buffer.source_revision,
        provenance: CoreTreeSitterProvenance {
            language_id: "rust".to_string(),
            package_id: "tree-sitter-rust".to_string(),
            package_version: "0.24.2".to_string(),
            parser_version: "14".to_string(),
            query_version: "saya-test".to_string(),
        },
        status: CoreTreeSitterStatus::Prepared,
        has_error: false,
        covered_ranges: vec![covered_range],
        error_ranges: vec![],
        budget_status: CoreTreeSitterBudgetStatus::WithinBudget,
        chunks: vec![CoreTreeSitterChunk {
            range: CoreTextRange {
                start: CoreTextPosition { row: 0, col: 0 },
                end: CoreTextPosition { row: 0, col: 2 },
            },
            capture_name: "keyword".to_string(),
            category: CoreSyntaxCategory::Keyword,
            modifiers: vec![],
        }],
        embedded_regions: vec![],
    };

    let stale_revision = {
        let mut syntax = base_syntax.clone();
        syntax.source_revision = CoreBufferRevision {
            value: active_buffer.source_revision.value.saturating_sub(1),
        };
        syntax
    };
    let stale_status = {
        let mut syntax = base_syntax.clone();
        syntax.status = CoreTreeSitterStatus::Stale;
        syntax
    };
    let parser_error = {
        let mut syntax = base_syntax.clone();
        syntax.has_error = true;
        syntax
    };
    let error_range = {
        let mut syntax = base_syntax.clone();
        syntax.error_ranges = vec![CoreTextRange {
            start: CoreTextPosition { row: 0, col: 0 },
            end: CoreTextPosition { row: 0, col: 2 },
        }];
        syntax
    };
    let budget_exceeded = {
        let mut syntax = base_syntax.clone();
        syntax.budget_status = CoreTreeSitterBudgetStatus::GlobalBudgetExceeded;
        syntax
    };
    let uncovered = {
        let mut syntax = base_syntax;
        syntax.covered_ranges.clear();
        syntax
    };

    for (case, syntax) in [
        ("stale revision", stale_revision),
        ("stale status", stale_status),
        ("parser error", parser_error),
        ("error range", error_range),
        ("budget exceeded", budget_exceeded),
        ("uncovered visible range", uncovered),
    ] {
        let model = project(
            &ProjectionInput::new(&snapshot, &session_state, None)
                .with_tree_sitter_syntax(Some(&syntax)),
        );

        assert!(
            model.syntax_chunks.is_empty(),
            "{case} Tree-sitter data must not be drawn as fresh highlight"
        );
    }
}

#[test]
fn markdown_projection_preserves_raw_offsets_for_multibyte_and_concealed_markers() {
    let _lock = session_test_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());

    let source = "# あ*強*\n";
    let bridge = CoreBridge::new(source).expect("core bridge");
    let snapshot = bridge.snapshot();
    let session_state = EditorSessionState::new(None);
    let markdown_map = MarkdownDocumentMap::parse(source);
    let mut input = ProjectionInput::new(&snapshot, &session_state, None)
        .with_markdown_document_map(Some(&markdown_map));
    input.is_active = false;

    let model = project(&input);
    let row = &model.line_projections[0];

    assert_eq!(row.raw_text, "# あ*強*");
    assert_eq!(row.display_text, "あ強");
    assert_eq!(row.logical_to_display_col(0), 0);
    assert_eq!(row.logical_to_display_col(2), 0);
    assert_eq!(row.logical_to_display_col(5), 2);
    assert_eq!(row.logical_to_display_col(9), 4);
    assert_eq!(row.display_to_logical_col(0), Some(2));
    assert_eq!(row.display_to_logical_col(2), Some(6));
    assert_eq!(
        snapshot.text, source,
        "Markdown projection must not mutate the raw buffer text"
    );
}

#[test]
fn markdown_projection_maps_tabs_from_raw_byte_to_display_cells() {
    let _lock = session_test_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());

    let source = "# a\tb\n";
    let bridge = CoreBridge::new(source).expect("core bridge");
    let snapshot = bridge.snapshot();
    let session_state = EditorSessionState::new_with_tab_size(None, 4);
    let markdown_map = MarkdownDocumentMap::parse(source);
    let mut input = ProjectionInput::new(&snapshot, &session_state, None)
        .with_markdown_document_map(Some(&markdown_map));
    input.is_active = false;

    let model = project(&input);
    let row = &model.line_projections[0];

    // タブ stop は Vim 互換に raw text 上の content col 起算で計算する。
    // raw="# a\tb" だと '\t' は content_col=3 にあり、tab_size=4 なら次の
    // tab stop は 4 → 幅 1 セル。コンセルされた "# " は表示 0 cell として消費。
    assert_eq!(row.raw_text, "# a\tb");
    assert_eq!(row.display_text, "a b");
    assert_eq!(row.logical_to_display_col(3), 1);
    assert_eq!(row.logical_to_display_col(4), 2);
    assert_eq!(row.display_to_logical_col(1), Some(3));
    assert_eq!(row.display_to_logical_col(2), Some(4));
}

#[test]
fn markdown_projection_keeps_line_number_gutter_outside_raw_mapping() {
    let _lock = session_test_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());

    let source = "# Title\n";
    let bridge = CoreBridge::new(source).expect("core bridge");
    let snapshot = bridge.snapshot();
    let session_state =
        EditorSessionState::new_with_tab_size_and_line_numbers_and_number_width(None, 8, true, 4);
    let markdown_map = MarkdownDocumentMap::parse(source);
    let mut input = ProjectionInput::new(&snapshot, &session_state, None)
        .with_markdown_document_map(Some(&markdown_map));
    input.is_active = false;

    let model = project(&input);
    let row = &model.line_projections[0];

    assert_eq!(model.lines[0], "   1 # Title");
    assert_eq!(row.line_start_col, 5);
    assert_eq!(row.display_text, "Title");
    assert_eq!(row.logical_to_display_col(2), 5);
    assert_eq!(row.display_to_logical_col(4), None);
    assert_eq!(row.display_to_logical_col(5), Some(2));
}

#[test]
fn markdown_projection_replaces_checkbox_marker_with_wide_display_glyph() {
    let _lock = session_test_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());

    let source = "- [x] done\n";
    let bridge = CoreBridge::new(source).expect("core bridge");
    let snapshot = bridge.snapshot();
    let session_state = EditorSessionState::new(None);
    let markdown_map = MarkdownDocumentMap::parse(source);
    let mut input = ProjectionInput::new(&snapshot, &session_state, None)
        .with_markdown_document_map(Some(&markdown_map));
    input.is_active = false;

    let model = project(&input);
    let row = &model.line_projections[0];

    assert_eq!(row.raw_text, "- [x] done");
    assert_eq!(row.display_text, "• ✅ done");
    assert_eq!(row.logical_to_display_col(2), 2);
    assert_eq!(row.logical_to_display_col(5), 4);
    assert_eq!(row.display_to_logical_col(2), Some(2));
    assert_eq!(row.display_to_logical_col(3), Some(2));
    assert!(
        row.spans.iter().any(|span| matches!(
            span.kind,
            ScreenDisplaySpanKind::MarkdownReplacement { ref text } if text == "✅"
        )),
        "checkbox marker should be represented as an explicit replacement span"
    );
}

#[test]
fn markdown_projection_replaces_unordered_list_marker_with_bullet() {
    let _lock = session_test_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());

    let source = "- item\n";
    let bridge = CoreBridge::new(source).expect("core bridge");
    let snapshot = bridge.snapshot();
    let session_state = EditorSessionState::new(None);
    let markdown_map = MarkdownDocumentMap::parse(source);
    let mut input = ProjectionInput::new(&snapshot, &session_state, None)
        .with_markdown_document_map(Some(&markdown_map));
    input.is_active = false;

    let model = project(&input);
    let row = &model.line_projections[0];

    assert_eq!(row.raw_text, "- item");
    assert_eq!(row.display_text, "• item");
    assert_eq!(row.logical_to_display_col(0), 0);
    assert_eq!(row.logical_to_display_col(2), 2);
    assert!(
        row.spans.iter().any(|span| matches!(
            span.kind,
            ScreenDisplaySpanKind::MarkdownReplacement { ref text } if text == "• "
        )),
        "unordered list marker should be represented as an explicit replacement span"
    );
}

#[test]
fn markdown_projection_renders_table_block_with_aligned_columns_when_not_raw() {
    let _lock = session_test_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());

    let source = "| Name | Value |\n|---|---:|\n| *short* | 10 |\n| longer | 200 |\n";
    let bridge = CoreBridge::new(source).expect("core bridge");
    let snapshot = bridge.snapshot();
    let session_state = EditorSessionState::new(None);
    let markdown_map = MarkdownDocumentMap::parse(source);
    let mut input = ProjectionInput::new(&snapshot, &session_state, None)
        .with_markdown_document_map(Some(&markdown_map));
    input.is_active = false;

    let model = project(&input);

    assert_eq!(
        model
            .line_projections
            .iter()
            .map(|row| row.display_text.as_str())
            .collect::<Vec<_>>(),
        vec![
            "│ Name   │ Value │",
            "│────────│───────│",
            "│ short  │    10 │",
            "│ longer │   200 │",
            "",
        ]
    );
    assert_eq!(
        snapshot.text, source,
        "Markdown table projection must not mutate the raw buffer text"
    );
}

#[test]
fn markdown_projection_wraps_table_cells_to_available_width() {
    let _lock = session_test_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());

    let source = "| Col | Description |\n|---|---|\n| a | one two three four five six |\n";
    let bridge = CoreBridge::new(source).expect("core bridge");
    let snapshot = bridge.snapshot();
    let session_state = EditorSessionState::new(None);
    let markdown_map = MarkdownDocumentMap::parse(source);
    let mut input = ProjectionInput::new(&snapshot, &session_state, None)
        .with_markdown_document_map(Some(&markdown_map));
    input.is_active = false;
    input.rect = PaneRect {
        x: 0,
        y: 0,
        width: 24,
        height: 24,
    };

    let model = project(&input);

    let rendered = model
        .line_projections
        .iter()
        .map(|row| row.display_text.as_str())
        .collect::<Vec<_>>();

    assert_eq!(
        rendered,
        vec![
            "│ Col │ Description    │",
            "│─────│────────────────│",
            "│ a   │ one two three  │",
            "│     │ four five six  │",
            "",
        ],
        "wide table cells must wrap to the available pane width"
    );

    for row in &rendered {
        assert!(
            display_width(row, 1) <= usize::from(input.rect.width),
            "rendered table row must not exceed pane width: {row:?}"
        );
    }
}

#[test]
fn markdown_projection_renders_html_br_inside_table_cell_as_display_line_break() {
    let _lock = session_test_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());

    let source = "| TH | TH |\n|---|---|\n| TD<br>aa | |\n";
    let bridge = CoreBridge::new(source).expect("core bridge");
    let snapshot = bridge.snapshot();
    let session_state = EditorSessionState::new(None);
    let markdown_map = MarkdownDocumentMap::parse(source);
    let mut input = ProjectionInput::new(&snapshot, &session_state, None)
        .with_markdown_document_map(Some(&markdown_map));
    input.is_active = false;

    let model = project(&input);

    assert_eq!(
        model
            .line_projections
            .iter()
            .map(|row| row.display_text.as_str())
            .collect::<Vec<_>>(),
        vec![
            "│ TH │ TH │",
            "│────│────│",
            "│ TD │    │",
            "│ aa │    │",
            "",
        ]
    );
    assert_eq!(
        model.lines,
        vec![
            "| TH | TH |".to_string(),
            "|---|---|".to_string(),
            "| TD<br>aa | |".to_string(),
            "| TD<br>aa | |".to_string(),
            "".to_string(),
        ],
        "display lines should expand alongside multiline table projections"
    );
}

#[test]
fn markdown_projection_keeps_line_number_gutter_when_table_cell_br_expands_rows() {
    let _lock = session_test_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());

    let source = "| TH | TH |\n|---|---|\n| TD<br>aa | |\n";
    let bridge = CoreBridge::new(source).expect("core bridge");
    let snapshot = bridge.snapshot();
    let session_state =
        EditorSessionState::new_with_tab_size_and_line_numbers_and_number_width(None, 8, true, 4);
    let markdown_map = MarkdownDocumentMap::parse(source);
    let mut input = ProjectionInput::new(&snapshot, &session_state, None)
        .with_markdown_document_map(Some(&markdown_map));
    input.is_active = false;

    let model = project(&input);

    assert_eq!(
        model.lines,
        vec![
            "   1 | TH | TH |".to_string(),
            "   2 |---|---|".to_string(),
            "   3 | TD<br>aa | |".to_string(),
            "   3 | TD<br>aa | |".to_string(),
            "".to_string(),
        ],
        "expanded table display rows must keep the line-number gutter source"
    );
    assert_eq!(
        model
            .line_projections
            .iter()
            .map(|row| row.display_text.as_str())
            .collect::<Vec<_>>(),
        vec![
            "│ TH │ TH │",
            "│────│────│",
            "│ TD │    │",
            "│ aa │    │",
            "",
        ]
    );
}

#[test]
fn markdown_projection_offsets_cursor_row_after_rendered_table_expands_display_rows() {
    let _lock = session_test_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());

    let source = "| A | B |\n|---|---|\n| x | y |\n# After\n";
    let bridge = CoreBridge::new(source).expect("core bridge");
    let mut snapshot = bridge.snapshot();
    snapshot.cursor_row = 3;
    let session_state = EditorSessionState::new(None);
    let markdown_map = MarkdownDocumentMap::parse(source);
    let mut input = ProjectionInput::new(&snapshot, &session_state, None)
        .with_markdown_document_map(Some(&markdown_map));
    input.cursor_row = 3;

    let model = project(&input);

    assert_eq!(
        model
            .line_projections
            .iter()
            .map(|row| row.display_text.as_str())
            .collect::<Vec<_>>(),
        vec!["│ A │ B │", "│───│───│", "│ x │ y │", "# After", "",]
    );
    assert_eq!(
        model.cursor_row, 3,
        "cursor row should stay aligned when table rendering preserves source row count"
    );
}

#[test]
fn markdown_projection_respects_viewport_absolute_rows() {
    let _lock = session_test_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());

    let source = "alpha\n# Beta\n*gamma*\n";
    let bridge = CoreBridge::new(source).expect("core bridge");
    let snapshot = bridge.snapshot();
    let session_state = EditorSessionState::new(None);
    let markdown_map = MarkdownDocumentMap::parse(source);

    let model = project(
        &ProjectionInput::new(&snapshot, &session_state, None)
            .with_markdown_document_map(Some(&markdown_map))
            .with_viewport(1, 2),
    );

    assert_eq!(model.lines, vec!["# Beta", "*gamma*"]);
    assert_eq!(
        model
            .line_projections
            .iter()
            .map(|row| (row.absolute_row, row.display_text.as_str()))
            .collect::<Vec<_>>(),
        vec![(1, "Beta"), (2, "gamma")]
    );
}

#[test]
fn active_markdown_projection_keeps_cursor_heading_block_raw_and_other_rows_rich() {
    let _lock = session_test_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());

    let source = "# Title\n*body*\n";
    let bridge = CoreBridge::new(source).expect("core bridge");
    let mut snapshot = bridge.snapshot();
    snapshot.cursor_row = 0;
    let session_state = EditorSessionState::new(None);
    let markdown_map = MarkdownDocumentMap::parse(source);
    let mut input = ProjectionInput::new(&snapshot, &session_state, None)
        .with_markdown_document_map(Some(&markdown_map));
    input.cursor_row = 0;

    let model = project(&input);

    assert_eq!(model.line_projections[0].display_text, "# Title");
    assert_eq!(model.line_projections[1].display_text, "body");
    assert_eq!(
        snapshot.text, source,
        "raw block expansion must not mutate the raw buffer text"
    );
}

#[test]
fn active_markdown_projection_keeps_cursor_list_block_raw() {
    let _lock = session_test_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());

    let source = "- [x] done\n# Next\n";
    let bridge = CoreBridge::new(source).expect("core bridge");
    let mut snapshot = bridge.snapshot();
    snapshot.cursor_row = 0;
    let session_state = EditorSessionState::new(None);
    let markdown_map = MarkdownDocumentMap::parse(source);
    let mut input = ProjectionInput::new(&snapshot, &session_state, None)
        .with_markdown_document_map(Some(&markdown_map));
    input.cursor_row = 0;

    let model = project(&input);

    assert_eq!(model.line_projections[0].display_text, "- [x] done");
    assert_eq!(model.line_projections[1].display_text, "Next");
}

#[test]
fn active_markdown_projection_keeps_entire_cursor_fenced_block_raw() {
    let _lock = session_test_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());

    let source = "# Before\n```rust\n*raw*\n```\n# After\n";
    let bridge = CoreBridge::new(source).expect("core bridge");
    let mut snapshot = bridge.snapshot();
    snapshot.cursor_row = 2;
    let session_state = EditorSessionState::new(None);
    let markdown_map = MarkdownDocumentMap::parse(source);
    let mut input = ProjectionInput::new(&snapshot, &session_state, None)
        .with_markdown_document_map(Some(&markdown_map));
    input.cursor_row = 2;

    let model = project(&input);

    assert_eq!(model.line_projections[0].display_text, "Before");
    assert_eq!(model.line_projections[1].display_text, "```rust");
    assert_eq!(model.line_projections[2].display_text, "*raw*");
    assert_eq!(model.line_projections[3].display_text, "```");
    assert_eq!(model.line_projections[4].display_text, "After");
}

#[test]
fn active_markdown_projection_keeps_entire_cursor_table_block_raw() {
    let _lock = session_test_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());

    let source = "| A | B |\n|---|---|\n| *x* | y |\n# After\n";
    let bridge = CoreBridge::new(source).expect("core bridge");
    let mut snapshot = bridge.snapshot();
    snapshot.cursor_row = 2;
    let session_state = EditorSessionState::new(None);
    let markdown_map = MarkdownDocumentMap::parse(source);
    let mut input = ProjectionInput::new(&snapshot, &session_state, None)
        .with_markdown_document_map(Some(&markdown_map));
    input.cursor_row = 2;

    let model = project(&input);

    assert_eq!(
        model
            .line_projections
            .iter()
            .map(|row| row.display_text.as_str())
            .collect::<Vec<_>>(),
        vec!["| A | B |", "|---|---|", "| *x* | y |", "After", ""]
    );
}

#[test]
fn active_markdown_projection_falls_back_to_cursor_row_raw_for_inline_only_markdown() {
    let _lock = session_test_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());

    let source = "*active*\n*rich*\n";
    let bridge = CoreBridge::new(source).expect("core bridge");
    let mut snapshot = bridge.snapshot();
    snapshot.cursor_row = 0;
    let session_state = EditorSessionState::new(None);
    let markdown_map = MarkdownDocumentMap::parse(source);

    let model = project(
        &ProjectionInput::new(&snapshot, &session_state, None)
            .with_markdown_document_map(Some(&markdown_map)),
    );

    assert_eq!(model.line_projections[0].display_text, "*active*");
    assert_eq!(model.line_projections[1].display_text, "rich");
}

#[test]
fn active_markdown_projection_tracks_cursor_movement_between_raw_rows() {
    let _lock = session_test_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());

    let source = "# Title\n*body*\n# After\n";
    let mut bridge = CoreBridge::new(source).expect("core bridge");
    let session_state = EditorSessionState::new(None);
    let markdown_map = MarkdownDocumentMap::parse(source);

    let initial_snapshot = bridge.snapshot();
    assert_eq!(initial_snapshot.cursor_row, 0);
    let initial_model = project(
        &ProjectionInput::new(&initial_snapshot, &session_state, None)
            .with_markdown_document_map(Some(&markdown_map)),
    );

    assert_eq!(initial_model.line_projections[0].display_text, "# Title");
    assert_eq!(initial_model.line_projections[1].display_text, "body");
    assert_eq!(initial_model.line_projections[2].display_text, "After");

    bridge
        .dispatch_key("j")
        .expect("move cursor to inline Markdown row");
    let moved_snapshot = bridge.snapshot();
    assert_eq!(moved_snapshot.cursor_row, 1);
    let moved_model = project(
        &ProjectionInput::new(&moved_snapshot, &session_state, None)
            .with_markdown_document_map(Some(&markdown_map)),
    );

    assert_eq!(moved_model.line_projections[0].display_text, "Title");
    assert_eq!(moved_model.line_projections[1].display_text, "*body*");
    assert_eq!(moved_model.line_projections[2].display_text, "After");
}

#[test]
fn inactive_markdown_projection_keeps_all_rows_rich_even_at_cursor_block() {
    let _lock = session_test_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());

    let source = "# Title\n*body*\n";
    let bridge = CoreBridge::new(source).expect("core bridge");
    let mut snapshot = bridge.snapshot();
    snapshot.cursor_row = 0;
    let session_state = EditorSessionState::new(None);
    let markdown_map = MarkdownDocumentMap::parse(source);
    let mut input = ProjectionInput::new(&snapshot, &session_state, None)
        .with_markdown_document_map(Some(&markdown_map));
    input.is_active = false;

    let model = project(&input);

    assert_eq!(model.line_projections[0].display_text, "Title");
    assert_eq!(model.line_projections[1].display_text, "body");
}

#[test]
fn inactive_markdown_projection_keeps_list_item_rich_even_at_cursor_row() {
    let _lock = session_test_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());

    let source = "- [x] done\n# Next\n";
    let bridge = CoreBridge::new(source).expect("core bridge");
    let mut snapshot = bridge.snapshot();
    snapshot.cursor_row = 0;
    let session_state = EditorSessionState::new(None);
    let markdown_map = MarkdownDocumentMap::parse(source);
    let mut input = ProjectionInput::new(&snapshot, &session_state, None)
        .with_markdown_document_map(Some(&markdown_map));
    input.is_active = false;

    let model = project(&input);

    assert_eq!(model.line_projections[0].display_text, "• ✅ done");
    assert_eq!(model.line_projections[1].display_text, "Next");
}

#[test]
fn markdown_projection_keeps_all_rows_raw_when_markdown_render_is_disabled() {
    let _lock = session_test_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());

    let source = "# Title\n- [x] done\n*tail*\n";
    let bridge = CoreBridge::new(source).expect("core bridge");
    let snapshot = bridge.snapshot();
    let mut session_state = EditorSessionState::new(None);
    session_state
        .apply_presentation_option(
            crate::runtime::options::SayaOptionName::MarkdownRender,
            crate::runtime::options::SayaOptionValue::Boolean(false),
        )
        .expect("markdownrender option should apply");
    let markdown_map = MarkdownDocumentMap::parse(source);

    let model = project(
        &ProjectionInput::new(&snapshot, &session_state, None)
            .with_markdown_document_map(Some(&markdown_map)),
    );

    assert_eq!(
        model
            .line_projections
            .iter()
            .map(|line| line.display_text.as_str())
            .collect::<Vec<_>>(),
        vec!["# Title", "- [x] done", "*tail*", ""],
        "disabled Markdown rendering should keep raw Markdown even when metadata is present"
    );
}

#[test]
fn dirty_and_cursor_update_on_redraw() {
    let _lock = session_test_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());

    let mut bridge = CoreBridge::new("line1\nline2\n").expect("core bridge");

    // 初回投影
    let snapshot1 = bridge.snapshot();
    let session_state = EditorSessionState::new(None);
    let model1 = project(&ProjectionInput::new(&snapshot1, &session_state, None));
    assert!(!model1.dirty);
    assert_eq!(model1.cursor_row, 0);

    // 編集操作後に再投影
    bridge.dispatch_key("j").expect("move down");
    bridge.dispatch_key("i").expect("insert mode");
    bridge.dispatch_key("Z").expect("insert Z");
    bridge.dispatch_key("\x1b").expect("normal mode");

    let snapshot2 = bridge.snapshot();
    let model2 = project(&ProjectionInput::new(&snapshot2, &session_state, None));

    assert!(model2.dirty, "編集後の再描画では dirty=true");
    assert_eq!(model2.cursor_row, 1, "カーソル行が再描画で追随すること");
}

// ---- タスク 6.3: message line を描画モデルへ取り込むテスト ----

#[test]
fn projects_no_message_line_in_normal_state() {
    let _lock = session_test_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());

    let bridge = CoreBridge::new("text\n").expect("core bridge");
    let snapshot = bridge.snapshot();
    let session_state = EditorSessionState::new(None);

    let model = project(&ProjectionInput::new(&snapshot, &session_state, None));

    assert_eq!(model.message_line, None, "通常状態ではメッセージ欄は空");
}

#[test]
fn projects_save_failure_as_message_line() {
    let _lock = session_test_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());

    let bridge = CoreBridge::new("text\n").expect("core bridge");
    let snapshot = bridge.snapshot();
    let mut session_state = EditorSessionState::new(None);
    session_state.record_save_failure("disk full".to_string());

    let model = project(&ProjectionInput::new(&snapshot, &session_state, None));

    assert_eq!(
        model.message_line,
        Some("保存失敗: disk full".to_string()),
        "保存失敗メッセージが message_line に反映されること"
    );
}

#[test]
fn projects_transient_message_over_save_error_in_message_line() {
    let _lock = session_test_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());

    let bridge = CoreBridge::new("text\n").expect("core bridge");
    let snapshot = bridge.snapshot();
    let mut session_state = EditorSessionState::new(None);
    session_state.record_save_failure("old error".to_string());

    let model = project(&ProjectionInput::new(
        &snapshot,
        &session_state,
        Some("未保存の変更があります"),
    ));

    assert_eq!(
        model.message_line,
        Some("未保存の変更があります".to_string()),
        "transient_message が save error より優先されること"
    );
}

#[test]
fn projects_unsaved_warning_as_transient_message() {
    let _lock = session_test_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());

    let bridge = CoreBridge::new("text\n").expect("core bridge");
    let snapshot = bridge.snapshot();
    let session_state = EditorSessionState::new(None);

    let model = project(&ProjectionInput::new(
        &snapshot,
        &session_state,
        Some("未保存の変更があります。:q! で強制終了できます"),
    ));

    assert_eq!(
        model.message_line,
        Some("未保存の変更があります。:q! で強制終了できます".to_string()),
        "未保存警告が transient_message として投影されること"
    );
}

#[test]
fn projects_config_failure_as_transient_message() {
    let _lock = session_test_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());

    let bridge = CoreBridge::new("text\n").expect("core bridge");
    let snapshot = bridge.snapshot();
    let session_state = EditorSessionState::new(None);

    let model = project(&ProjectionInput::new(
        &snapshot,
        &session_state,
        Some("設定の読み込みに失敗しました"),
    ));

    assert_eq!(
        model.message_line,
        Some("設定の読み込みに失敗しました".to_string()),
        "設定失敗メッセージが投影されること"
    );
}

#[test]
fn resolves_message_state_by_fixed_priority_order() {
    let _lock = session_test_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());

    let bridge = CoreBridge::new("text\n").expect("core bridge");
    let snapshot = bridge.snapshot();
    let session_state = EditorSessionState::new(None);

    let priority_input = ProjectionInput::new(&snapshot, &session_state, Some("transient"))
        .with_command_preview(Some("/pattern"))
        .with_core_message(Some("core warning"))
        .with_system_warning(Some("system warning"));
    let resolved = resolve_message_state(&priority_input)
        .expect("command preview should win over all other messages");
    assert_eq!(resolved.kind, ScreenMessageKind::CommandPreview);
    assert_eq!(resolved.text, "/pattern");

    let core_first = ProjectionInput::new(&snapshot, &session_state, Some("transient"))
        .with_core_message(Some("core warning"))
        .with_system_warning(Some("system warning"));
    let resolved = resolve_message_state(&core_first)
        .expect("core message should win when no command preview exists");
    assert_eq!(resolved.kind, ScreenMessageKind::CoreMessage);
    assert_eq!(resolved.text, "core warning");

    let system_first = ProjectionInput::new(&snapshot, &session_state, Some("transient"))
        .with_system_warning(Some("system warning"));
    let resolved = resolve_message_state(&system_first)
        .expect("system warning should win when no higher-priority message exists");
    assert_eq!(resolved.kind, ScreenMessageKind::SystemWarning);
    assert_eq!(resolved.text, "system warning");

    let transient_only = ProjectionInput::new(&snapshot, &session_state, Some("transient"));
    let resolved = resolve_message_state(&transient_only)
        .expect("transient info should be used as the fallback");
    assert_eq!(resolved.kind, ScreenMessageKind::TransientInfo);
    assert_eq!(resolved.text, "transient");
}

#[test]
fn clears_message_line_after_save_success() {
    let _lock = session_test_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());

    let bridge = CoreBridge::new("text\n").expect("core bridge");
    let snapshot = bridge.snapshot();
    let mut session_state = EditorSessionState::new(None);

    // 保存失敗を記録
    session_state.record_save_failure("error".to_string());
    let model1 = project(&ProjectionInput::new(&snapshot, &session_state, None));
    assert!(model1.message_line.is_some());

    // 保存成功を記録
    session_state.record_save_success();
    let model2 = project(&ProjectionInput::new(&snapshot, &session_state, None));
    assert_eq!(
        model2.message_line, None,
        "保存成功後はメッセージ欄がクリアされること"
    );
}

#[test]
fn success_message_takes_priority_when_provided_as_transient() {
    let _lock = session_test_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());

    let bridge = CoreBridge::new("text\n").expect("core bridge");
    let snapshot = bridge.snapshot();
    let session_state = EditorSessionState::new(None);

    let model = project(&ProjectionInput::new(
        &snapshot,
        &session_state,
        Some("保存しました"),
    ));

    assert_eq!(
        model.message_line,
        Some("保存しました".to_string()),
        "成功メッセージが transient として投影されること"
    );
}

// ---- タスク 6.4: 行データとカーソルを terminal 描画へ流せるようにするテスト ----

#[test]
fn projects_text_lines_from_snapshot() {
    let _lock = session_test_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());

    let bridge = CoreBridge::new("line1\nline2\nline3\n").expect("core bridge");
    let snapshot = bridge.snapshot();
    let session_state = EditorSessionState::new(None);

    let model = project(&ProjectionInput::new(&snapshot, &session_state, None));

    assert_eq!(
        model.lines,
        vec!["line1", "line2", "line3"],
        "行データが snapshot から正しく分割されること"
    );
}

#[test]
fn project_limits_visible_lines_to_body_height() {
    let _lock = session_test_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());

    let bridge = CoreBridge::new("line1\nline2\nline3\nline4\n").expect("core bridge");
    let snapshot = bridge.snapshot();
    let session_state = EditorSessionState::new(None);

    let model = project(&ProjectionInput::new(&snapshot, &session_state, None).with_viewport(1, 2));

    assert_eq!(model.lines, vec!["line2", "line3"]);
}

#[test]
fn project_large_viewport_only_materializes_visible_lines() {
    let large_text = (0..1_000_000)
        .map(|line| format!("line{line}\tvalue"))
        .collect::<Vec<_>>()
        .join("\n");
    let snapshot = snapshot_for_projection_text(large_text, 42, 0, 42, 54);
    let mut session_state = EditorSessionState::new(None);
    session_state.set_line_numbers(true);

    let started_at = std::time::Instant::now();
    let model =
        project(&ProjectionInput::new(&snapshot, &session_state, None).with_viewport(42, 12));
    let elapsed = started_at.elapsed();

    assert_eq!(model.lines.len(), 12);
    assert_eq!(model.lines[0], "  43 line42  value");
    assert!(
        elapsed.as_millis() < 40,
        "large viewport projection should avoid full-buffer materialization: elapsed_ms={}",
        elapsed.as_millis()
    );
}

#[test]
fn project_workspace_uses_window_line_range_without_snapshot_text() {
    let snapshot = snapshot_for_projection_text(String::new(), 43, 4, 42, 55);
    let mut session_state = EditorSessionState::new(None);
    session_state.set_line_numbers(true);
    let mut viewport_store = WindowViewportStore::new();
    viewport_store.sync_from_windows(&snapshot.windows);
    let mut line_ranges = BTreeMap::new();
    line_ranges.insert(
        1,
        CoreBufferLineRange {
            buffer_id: 1,
            source_revision: CoreBufferRevision { value: 1 },
            start_row: 42,
            line_count: 12,
            total_line_count: 200_469,
            lines: (42..54)
                .map(|index| format!("range-line-{index}"))
                .collect(),
        },
    );
    let search_states = BTreeMap::new();
    let syntax_lines = BTreeMap::new();
    let markdown_document_maps = BTreeMap::new();

    let model = project_workspace(&WorkspaceProjectionInput {
        snapshot: &snapshot,
        light_snapshot: None,
        line_ranges: &line_ranges,
        session_state: &session_state,
        visual_selection: None,
        search_states: &search_states,
        syntax_lines: &syntax_lines,
        #[cfg(feature = "tree-sitter-syntax")]
        tree_sitter_syntax: &BTreeMap::new(),
        markdown_document_maps: &markdown_document_maps,
        command_preview: None,
        core_message: None,
        notification_prompt: None,
        system_warning: None,
        transient_info: None,
        viewport_store: &viewport_store,
        terminal_width: 80,
        terminal_height: 14,
    })
    .expect("workspace projection");

    assert_eq!(model.panes[0].lines.len(), 12);
    assert_eq!(model.panes[0].lines[0], "    43 range-line-42");
    assert_eq!(model.panes[0].lines[11], "    54 range-line-53");
}

#[test]
fn project_workspace_uses_session_dirty_for_active_status_line() {
    let mut snapshot = snapshot_for_projection_text("saved\n".to_string(), 0, 0, 0, 1);
    snapshot.dirty = true;
    snapshot.buffers[0].dirty = true;
    let session_state = EditorSessionState::new(Some(PathBuf::from("hoge.md")));
    let mut viewport_store = WindowViewportStore::new();
    viewport_store.sync_from_windows(&snapshot.windows);
    let line_ranges = BTreeMap::new();
    let search_states = BTreeMap::new();
    let syntax_lines = BTreeMap::new();
    let markdown_document_maps = BTreeMap::new();

    let model = project_workspace(&WorkspaceProjectionInput {
        snapshot: &snapshot,
        light_snapshot: None,
        line_ranges: &line_ranges,
        session_state: &session_state,
        visual_selection: None,
        search_states: &search_states,
        syntax_lines: &syntax_lines,
        #[cfg(feature = "tree-sitter-syntax")]
        tree_sitter_syntax: &BTreeMap::new(),
        markdown_document_maps: &markdown_document_maps,
        command_preview: None,
        core_message: None,
        notification_prompt: None,
        system_warning: None,
        transient_info: Some("Saved successfully"),
        viewport_store: &viewport_store,
        terminal_width: 80,
        terminal_height: 14,
    })
    .expect("workspace projection");

    assert!(
        !model.panes[0].dirty,
        "active status line should use session dirty after save"
    );
}

#[test]
fn markdown_table_projection_uses_line_range_when_snapshot_text_is_empty() {
    let mut snapshot = snapshot_for_projection_text(String::new(), 0, 0, 1, 12);
    snapshot.buffers[0].name = "hoge.md".to_string();
    let session_state = EditorSessionState::new(Some(PathBuf::from("tmp/hoge.md")));
    let mut line_ranges = BTreeMap::new();
    let source_lines = vec![
        "".to_string(),
        "".to_string(),
        "".to_string(),
        "".to_string(),
        "| TH | TH |".to_string(),
        "| ---- | ---- |".to_string(),
        "| TD | TD |".to_string(),
        "| TD | TD |".to_string(),
        "".to_string(),
    ];
    line_ranges.insert(
        1,
        CoreBufferLineRange {
            buffer_id: 1,
            source_revision: CoreBufferRevision { value: 1 },
            start_row: 0,
            line_count: source_lines.len(),
            total_line_count: source_lines.len(),
            lines: source_lines.clone(),
        },
    );
    let markdown_source = source_lines.join("\n");
    let mut markdown_document_maps = BTreeMap::new();
    markdown_document_maps.insert(1, Arc::new(MarkdownDocumentMap::parse(&markdown_source)));
    let mut viewport_store = WindowViewportStore::new();
    viewport_store.sync_from_windows(&snapshot.windows);
    let search_states = BTreeMap::new();
    let syntax_lines = BTreeMap::new();

    let model = project_workspace(&WorkspaceProjectionInput {
        snapshot: &snapshot,
        light_snapshot: None,
        line_ranges: &line_ranges,
        session_state: &session_state,
        visual_selection: None,
        search_states: &search_states,
        syntax_lines: &syntax_lines,
        #[cfg(feature = "tree-sitter-syntax")]
        tree_sitter_syntax: &BTreeMap::new(),
        markdown_document_maps: &markdown_document_maps,
        command_preview: None,
        core_message: None,
        notification_prompt: None,
        system_warning: None,
        transient_info: None,
        viewport_store: &viewport_store,
        terminal_width: 80,
        terminal_height: 14,
    })
    .expect("workspace projection");

    assert!(
        model.panes[0]
            .line_projections
            .iter()
            .any(|line| line.display_text == "│ TH │ TH │"),
        "table should render from line_range-backed Markdown source"
    );
    assert!(
        model.panes[0]
            .line_projections
            .iter()
            .any(|line| line.display_text == "│────│────│")
    );
}

#[test]
fn markdown_mermaid_projection_does_not_render_partial_line_range_block() {
    let mut snapshot = snapshot_for_projection_text(String::new(), 6, 0, 0, 4);
    snapshot.buffers[0].name = "hoge.md".to_string();
    let session_state = EditorSessionState::new(Some(PathBuf::from("tmp/hoge.md")));
    let full_source_lines = vec![
        "```mermaid".to_string(),
        "stateDiagram-v2".to_string(),
        "    [*] --> Idle".to_string(),
        "    Idle --> Done".to_string(),
        "    Done --> [*]".to_string(),
        "```".to_string(),
        "after".to_string(),
    ];
    let mut line_ranges = BTreeMap::new();
    line_ranges.insert(
        1,
        CoreBufferLineRange {
            buffer_id: 1,
            source_revision: CoreBufferRevision { value: 1 },
            start_row: 0,
            line_count: 2,
            total_line_count: full_source_lines.len(),
            lines: full_source_lines[..2].to_vec(),
        },
    );
    let markdown_source = full_source_lines.join("\n");
    let mut markdown_document_maps = BTreeMap::new();
    markdown_document_maps.insert(1, Arc::new(MarkdownDocumentMap::parse(&markdown_source)));
    let mut viewport_store = WindowViewportStore::new();
    viewport_store.sync_from_windows(&snapshot.windows);
    let search_states = BTreeMap::new();
    let syntax_lines = BTreeMap::new();

    let model = project_workspace(&WorkspaceProjectionInput {
        snapshot: &snapshot,
        light_snapshot: None,
        line_ranges: &line_ranges,
        session_state: &session_state,
        visual_selection: None,
        search_states: &search_states,
        syntax_lines: &syntax_lines,
        #[cfg(feature = "tree-sitter-syntax")]
        tree_sitter_syntax: &BTreeMap::new(),
        markdown_document_maps: &markdown_document_maps,
        command_preview: None,
        core_message: None,
        notification_prompt: None,
        system_warning: None,
        transient_info: None,
        viewport_store: &viewport_store,
        terminal_width: 80,
        terminal_height: 8,
    })
    .expect("workspace projection");

    assert!(
        model.panes[0]
            .line_projections
            .iter()
            .all(|line| line.display_text != "[mermaid diagram]"),
        "partial Mermaid blocks must not render a placeholder with incomplete source"
    );
}

#[test]
fn markdown_mermaid_projection_keeps_full_block_as_body_text() {
    let mut snapshot = snapshot_for_projection_text(String::new(), 1, 0, 0, 6);
    snapshot.buffers[0].name = "hoge.md".to_string();
    let session_state = EditorSessionState::new(Some(PathBuf::from("tmp/hoge.md")));
    let source_lines = vec![
        "```mermaid".to_string(),
        "graph TD".to_string(),
        "  A-->B".to_string(),
        "```".to_string(),
        "after".to_string(),
    ];
    let mut line_ranges = BTreeMap::new();
    line_ranges.insert(
        1,
        CoreBufferLineRange {
            buffer_id: 1,
            source_revision: CoreBufferRevision { value: 1 },
            start_row: 0,
            line_count: source_lines.len(),
            total_line_count: source_lines.len(),
            lines: source_lines.clone(),
        },
    );
    let markdown_source = source_lines.join("\n");
    let mut markdown_document_maps = BTreeMap::new();
    markdown_document_maps.insert(1, Arc::new(MarkdownDocumentMap::parse(&markdown_source)));
    let mut viewport_store = WindowViewportStore::new();
    viewport_store.sync_from_windows(&snapshot.windows);
    let search_states = BTreeMap::new();
    let syntax_lines = BTreeMap::new();

    let model = project_workspace(&WorkspaceProjectionInput {
        snapshot: &snapshot,
        light_snapshot: None,
        line_ranges: &line_ranges,
        session_state: &session_state,
        visual_selection: None,
        search_states: &search_states,
        syntax_lines: &syntax_lines,
        #[cfg(feature = "tree-sitter-syntax")]
        tree_sitter_syntax: &BTreeMap::new(),
        markdown_document_maps: &markdown_document_maps,
        command_preview: None,
        core_message: None,
        notification_prompt: None,
        system_warning: None,
        transient_info: None,
        viewport_store: &viewport_store,
        terminal_width: 80,
        terminal_height: 8,
    })
    .expect("workspace projection");

    assert_eq!(
        model.panes[0].lines,
        vec!["```mermaid", "graph TD", "  A-->B", "```", "after"]
    );
    assert!(
        model.panes[0]
            .line_projections
            .iter()
            .all(|line| line.display_text != "[mermaid diagram]"),
        "Mermaid blocks should stay as body text; image rendering belongs to preview floats"
    );
}

fn snapshot_for_projection_text(
    text: String,
    cursor_row: usize,
    cursor_col: usize,
    topline: usize,
    botline: usize,
) -> CoreSnapshot {
    CoreSnapshot {
        text,
        revision: 1,
        dirty: false,
        mode: CoreMode::Normal,
        pending_input: CorePendingInput::none(),
        cursor_row,
        cursor_col,
        pending_host_actions: 0,
        buffers: vec![CoreBufferInfo {
            id: 1,
            name: "large.log".to_string(),
            source_revision: CoreBufferRevision { value: 1 },
            dirty: false,
            is_active: true,
            source_kind: CoreBufferSourceKind::Local,
            document_id: None,
            pending_vfs_operation: None,
            deferred_close: None,
            last_vfs_error: None,
        }],
        windows: vec![CoreWindowInfo {
            id: 1,
            buf_id: 1,
            row: 0,
            col: 0,
            width: 80,
            height: botline.saturating_sub(topline).max(1),
            topline,
            botline,
            leftcol: 0,
            skipcol: 0,
            cursor_row,
            cursor_col,
            is_active: true,
        }],
        pum: None,
    }
}

#[test]
fn projects_cursor_coordinates_as_u16() {
    let _lock = session_test_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());

    let mut bridge = CoreBridge::new("abcdef\nghijkl\n").expect("core bridge");
    bridge.dispatch_key("jlll").expect("move to row=1, col=3");
    let snapshot = bridge.snapshot();
    let session_state = EditorSessionState::new(None);

    let model = project(&ProjectionInput::new(&snapshot, &session_state, None));

    assert_eq!(model.cursor_row, 1, "カーソル行が u16 として正しく変換");
    assert_eq!(model.cursor_col, 3, "カーソル列が u16 として正しく変換");
}

#[test]
fn projects_display_cursor_col_for_multibyte_character() {
    let _lock = session_test_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());

    let mut bridge = CoreBridge::new("あa\n").expect("core bridge");
    bridge
        .dispatch_key("l")
        .expect("move right over multibyte char");
    let snapshot = bridge.snapshot();
    let session_state = EditorSessionState::new(None);

    assert_eq!(
        snapshot.cursor_col, 3,
        "vim-core-rs の cursor_col は UTF-8 バイト位置で進むこと"
    );

    let model = project(&ProjectionInput::new(&snapshot, &session_state, None));

    assert_eq!(model.cursor_row, 0, "行位置はそのまま反映されること");
    assert_eq!(
        model.cursor_col, 2,
        "全角 1 文字ぶんは terminal 上で 2 セルとして描画されること"
    );
}

#[test]
fn projects_cursor_col_with_line_number_prefix() {
    let _lock = session_test_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());

    let mut bridge = CoreBridge::new("alpha\nbeta\n").expect("core bridge");
    bridge.dispatch_key("jll").expect("move to row=1, col=2");
    let snapshot = bridge.snapshot();
    let session_state =
        EditorSessionState::new_with_tab_size_and_line_numbers_and_number_width(None, 8, true, 4);

    let model = project(&ProjectionInput::new(&snapshot, &session_state, None));

    assert_eq!(model.lines[1], "   2 beta");
    assert_eq!(model.cursor_row, 1);
    assert_eq!(
        model.cursor_col, 7,
        "行番号と区切り分だけ右へ補正されること"
    );
}

#[test]
fn projects_cursor_col_with_line_numbers_and_multibyte_text() {
    let _lock = session_test_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());

    let mut bridge = CoreBridge::new("あa\n").expect("core bridge");
    bridge
        .dispatch_key("l")
        .expect("move right over multibyte char");
    let snapshot = bridge.snapshot();
    let session_state =
        EditorSessionState::new_with_tab_size_and_line_numbers_and_number_width(None, 8, true, 4);

    let model = project(&ProjectionInput::new(&snapshot, &session_state, None));

    assert_eq!(model.lines[0], "   1 あa");
    assert_eq!(
        model.cursor_col, 7,
        "全角表示幅に行番号オフセットが加算されること"
    );
}

#[test]
fn projects_line_numbers_using_configured_minimum_width() {
    let _lock = session_test_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());

    let bridge = CoreBridge::new("alpha\nbeta\n").expect("core bridge");
    let snapshot = bridge.snapshot();
    let session_state =
        EditorSessionState::new_with_tab_size_and_line_numbers_and_number_width(None, 8, true, 4);

    let model = project(&ProjectionInput::new(&snapshot, &session_state, None));

    assert_eq!(model.lines[0], "   1 alpha");
    assert_eq!(model.lines[1], "   2 beta");
}

#[test]
fn projects_visual_selection_with_line_number_gutter_offset() {
    let _lock = session_test_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());

    let bridge = CoreBridge::new("alpha\nbeta\n").expect("core bridge");
    let snapshot = bridge.snapshot();
    let session_state =
        EditorSessionState::new_with_tab_size_and_line_numbers_and_number_width(None, 8, true, 4);
    let visual_selection = VisualSelection {
        mode: CoreMode::VisualLine,
        start_row: 0,
        start_col: 0,
        end_row: 1,
        end_col: 3,
    };

    let model = project(
        &ProjectionInput::new(&snapshot, &session_state, None)
            .with_visual_selection(Some(&visual_selection)),
    );
    let selection = model
        .visual_selection
        .expect("visual selection should be projected");

    assert_eq!(selection.start_col, 5);
    assert_eq!(selection.line_start_col, 5);
    assert_eq!(selection.end_col_exclusive, 9);
}

#[test]
fn projects_visual_line_selection_as_full_lines() {
    let _lock = session_test_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());

    let bridge = CoreBridge::new("aaa\nbbbb\ncc\n").expect("core bridge");
    let snapshot = bridge.snapshot();
    let session_state = EditorSessionState::new(None);
    let visual_selection = VisualSelection {
        mode: CoreMode::VisualLine,
        start_row: 1,
        start_col: 3,
        end_row: 2,
        end_col: 0,
    };

    let model = project(
        &ProjectionInput::new(&snapshot, &session_state, None)
            .with_visual_selection(Some(&visual_selection)),
    );
    let selection = model
        .visual_selection
        .expect("visual selection should be projected");

    assert_eq!(selection.start_row, 1);
    assert_eq!(selection.start_col, 0);
    assert_eq!(selection.line_start_col, 0);
    assert_eq!(selection.end_row, 2);
    assert_eq!(selection.end_col_exclusive, 2);
}

#[test]
fn projects_tab_as_spaces_using_default_tab_size() {
    let _lock = session_test_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());

    let mut bridge = CoreBridge::new("\ta\n").expect("core bridge");
    bridge.dispatch_key("l").expect("move right over tab");
    let snapshot = bridge.snapshot();
    let session_state = EditorSessionState::new(None);

    let model = project(&ProjectionInput::new(&snapshot, &session_state, None));

    assert_eq!(model.lines[0], "        a");
    assert_eq!(model.cursor_col, 8);
}

#[test]
fn projects_tab_using_configured_tab_size() {
    let _lock = session_test_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());

    let mut bridge = CoreBridge::new("\ta\n").expect("core bridge");
    bridge.dispatch_key("l").expect("move right over tab");
    let snapshot = bridge.snapshot();
    let session_state = EditorSessionState::new_with_tab_size(None, 4);

    let model = project(&ProjectionInput::new(&snapshot, &session_state, None));

    assert_eq!(model.lines[0], "    a");
    assert_eq!(model.cursor_col, 4);
}

#[test]
fn cursor_col_aligns_with_projected_cell_for_indented_line_with_gutter() {
    // 真因: タブ展開のセマンティクスがレンダリング(cells)とカーソル算出で食い違うバグ。
    // Vim 流（col-0 起算でタブは常に tab_size 全幅、ガターは単なる左パディング）に揃え、
    // `cursor_col` と「同じ raw_col のセル `display_col`」が一致することを保証する。
    let _lock = session_test_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());

    // バッファ: 行頭タブ + "hello" / カーソルを l キーで raw_col=1 ('h') へ移動。
    let mut bridge = CoreBridge::new("\thello\n").expect("core bridge");
    bridge.dispatch_key("l").expect("move right onto h");
    let snapshot = bridge.snapshot();
    // ガター幅 5 ("   1 ") 相当: number_width=4, line_numbers=true, tab_size=8。
    let session_state =
        EditorSessionState::new_with_tab_size_and_line_numbers_and_number_width(None, 8, true, 4);

    let model = project(&ProjectionInput::new(&snapshot, &session_state, None));

    // 'h' のセルを cells から探す（raw_start_col == 1）。
    let projection = model
        .line_projections
        .iter()
        .find(|projection| projection.absolute_row == 0)
        .expect("line projection for row 0 exists");
    let h_cell = projection
        .cells
        .iter()
        .find(|cell| cell.raw_start_col == 1)
        .copied()
        .expect("cell mapping for raw byte 1 ('h') exists");

    // 期待: tab_size=8 で行頭タブが 8 cells 幅 → 'h' はコンテンツ列 8 = 画面列 13(=5+8)。
    assert_eq!(
        projection.line_start_col, 5,
        "gutter width must be 5 ('   1 ')"
    );
    assert_eq!(
        h_cell.display_col, 13,
        "rendered 'h' must land at screen col 13 (gutter 5 + tab 8)"
    );

    // 真の整合性チェック: カーソル列 == 'h' のセル列。
    assert_eq!(
        model.cursor_col, h_cell.display_col,
        "cursor must land on the same screen column where 'h' is rendered \
         (cursor_col={}, cell.display_col={}, gutter={})",
        model.cursor_col, h_cell.display_col, projection.line_start_col,
    );
}

#[test]
fn screen_model_contains_all_draw_fields() {
    let _lock = session_test_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());

    let mut bridge = CoreBridge::new("first\nsecond\n").expect("core bridge");
    bridge.dispatch_key("i").expect("insert mode");
    bridge.dispatch_key("X").expect("insert X");
    bridge.dispatch_key("\x1b").expect("normal mode");
    let snapshot = bridge.snapshot();

    let session_state = EditorSessionState::new(Some(PathBuf::from("/tmp/test.txt")));

    let model = project(&ProjectionInput::new(
        &snapshot,
        &session_state,
        Some("テストメッセージ"),
    ));

    // 全フィールドがまとめて draw に必要なデータを持つこと
    assert!(!model.file_name.is_empty(), "ファイル名は空でないこと");
    assert!(!model.mode_label.is_empty(), "モードラベルは空でないこと");
    assert!(model.dirty, "編集後は dirty=true であること");
    assert!(!model.lines.is_empty(), "行データは空でないこと");
    assert!(model.message_line.is_some(), "メッセージ欄が存在すること");
}

#[test]
fn redraw_input_limited_to_screen_model_only() {
    // ScreenModel だけで描画に必要な全情報が揃うことを型レベルで検証
    let model = ScreenModel {
        window_id: 1,
        buffer_id: 1,
        rect: PaneRect {
            x: 0,
            y: 0,
            width: 1,
            height: 2,
        },
        file_name: "test.txt".to_string(),
        mode_label: "NORMAL".to_string(),
        status_line: "test.txt | NORMAL".to_string(),
        cursor_style: ScreenCursorStyle::Block,
        dirty: false,
        lines: vec!["hello".to_string()],
        line_projections: vec![],
        cursor_row: 0,
        cursor_col: 0,
        visual_selection: None,
        search_overlays: vec![],
        syntax_chunks: vec![],
        markdown_style_ranges: vec![],
        filer_style_ranges: vec![],
        resolved_theme: crate::presentation::theme::ResolvedTheme::default(),
        message_line: None,
        command_cursor_col: None,
        is_active: true,
    };

    // ScreenModel の各フィールドにアクセスできること（コンパイル時検証）
    let _ = &model.file_name;
    let _ = &model.mode_label;
    let _ = model.dirty;
    let _ = &model.lines;
    let _ = model.cursor_row;
    let _ = model.cursor_col;
    let _ = &model.visual_selection;
    let _ = &model.message_line;

    // CoreSnapshot への直接参照は不要（型の独立性）
    assert_eq!(
        model.mode_label, "NORMAL",
        "ScreenModel だけで描画に必要な全情報が揃うこと"
    );
}

#[test]
fn projects_search_overlay_with_tabs_wide_glyphs_and_gutter_offset() {
    let _lock = session_test_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());

    let bridge = CoreBridge::new("\tあx\n").expect("core bridge");
    let snapshot = bridge.snapshot();
    let active_window_id = snapshot
        .active_window_id()
        .expect("active window should exist");
    let session_state =
        EditorSessionState::new_with_tab_size_and_line_numbers_and_number_width(None, 4, true, 4);
    let search_state = SearchVisibleState {
        capability: SearchCapabilityContract::baseline_ready_contract(),
        window_id: active_window_id,
        visible_rows: SearchVisibleRows {
            start_row: 1,
            end_row: 1,
        },
        mode: SearchQueryMode::Hlsearch,
        pattern: Some("あ".to_string()),
        input_pattern: None,
        hlsearch_enabled: true,
        hlsearch_suspended: false,
        incsearch_active: false,
        matches: vec![SearchMatch {
            kind: SearchMatchKind::Current,
            start_row: 1,
            start_col: 1,
            end_row: 1,
            end_col: 4,
        }],
    };

    let model = project(
        &ProjectionInput::new(&snapshot, &session_state, None)
            .with_search_state(Some(&search_state)),
    );

    assert_eq!(
        model.search_overlays,
        vec![ScreenSearchOverlay {
            row: 0,
            start_col: 9,
            end_col_exclusive: 11,
            kind: SearchMatchKind::Current,
        }],
        "tab と全角文字と行番号オフセットを display-space に正しく投影すること"
    );
}

#[test]
fn projects_search_overlay_against_markdown_rich_projection_with_gutter_offset() {
    let _lock = session_test_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());

    let source = "# Title\n";
    let bridge = CoreBridge::new(source).expect("core bridge");
    let snapshot = bridge.snapshot();
    let active_window_id = snapshot
        .active_window_id()
        .expect("active window should exist");
    let session_state =
        EditorSessionState::new_with_tab_size_and_line_numbers_and_number_width(None, 8, true, 4);
    let markdown_map = MarkdownDocumentMap::parse(source);
    let search_state = SearchVisibleState {
        capability: SearchCapabilityContract::baseline_ready_contract(),
        window_id: active_window_id,
        visible_rows: SearchVisibleRows {
            start_row: 1,
            end_row: 1,
        },
        mode: SearchQueryMode::Hlsearch,
        pattern: Some("Title".to_string()),
        input_pattern: None,
        hlsearch_enabled: true,
        hlsearch_suspended: false,
        incsearch_active: false,
        matches: vec![SearchMatch {
            kind: SearchMatchKind::Current,
            start_row: 1,
            start_col: 2,
            end_row: 1,
            end_col: 7,
        }],
    };
    let mut input = ProjectionInput::new(&snapshot, &session_state, None)
        .with_markdown_document_map(Some(&markdown_map))
        .with_search_state(Some(&search_state));
    input.is_active = false;

    let model = project(&input);

    assert_eq!(model.lines[0], "   1 # Title");
    assert_eq!(model.line_projections[0].line_start_col, 5);
    assert_eq!(model.line_projections[0].display_text, "Title");
    assert_eq!(
        model.search_overlays,
        vec![ScreenSearchOverlay {
            row: 0,
            start_col: 5,
            end_col_exclusive: 10,
            kind: SearchMatchKind::Current,
        }],
        "search overlays should use Markdown projection display-space, not raw marker columns"
    );
}

#[test]
fn projects_search_overlay_clips_to_visible_rows_and_keeps_match_kinds() {
    let _lock = session_test_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());

    let bridge = CoreBridge::new("zero\nalpha\nbeta\nomega\n").expect("core bridge");
    let snapshot = bridge.snapshot();
    let active_window_id = snapshot
        .active_window_id()
        .expect("active window should exist");
    let session_state = EditorSessionState::new(None);
    let search_state = SearchVisibleState {
        capability: SearchCapabilityContract::baseline_ready_contract(),
        window_id: active_window_id,
        visible_rows: SearchVisibleRows {
            start_row: 2,
            end_row: 3,
        },
        hlsearch_enabled: true,
        hlsearch_suspended: false,
        incsearch_active: false,
        mode: SearchQueryMode::Hlsearch,
        pattern: Some("a".to_string()),
        input_pattern: None,
        matches: vec![
            SearchMatch {
                kind: SearchMatchKind::Regular,
                start_row: 2,
                start_col: 0,
                end_row: 2,
                end_col: 5,
            },
            SearchMatch {
                kind: SearchMatchKind::Current,
                start_row: 3,
                start_col: 1,
                end_row: 3,
                end_col: 4,
            },
            SearchMatch {
                kind: SearchMatchKind::Regular,
                start_row: 4,
                start_col: 0,
                end_row: 4,
                end_col: 5,
            },
        ],
    };

    let model = project(
        &ProjectionInput::new(&snapshot, &session_state, None)
            .with_search_state(Some(&search_state))
            .with_viewport(1, 2),
    );

    assert_eq!(
        model.search_overlays,
        vec![
            ScreenSearchOverlay {
                row: 0,
                start_col: 0,
                end_col_exclusive: 5,
                kind: SearchMatchKind::Regular,
            },
            ScreenSearchOverlay {
                row: 1,
                start_col: 1,
                end_col_exclusive: 4,
                kind: SearchMatchKind::Current,
            }
        ],
        "visible rows のみが投影され、current match が区別されること"
    );
}

#[test]
fn ignores_search_overlay_for_different_window_id() {
    let _lock = session_test_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());

    let bridge = CoreBridge::new("alpha\nbeta\n").expect("core bridge");
    let snapshot = bridge.snapshot();
    let session_state = EditorSessionState::new(None);
    let search_state = SearchVisibleState {
        capability: SearchCapabilityContract::baseline_ready_contract(),
        window_id: 999_999,
        visible_rows: SearchVisibleRows {
            start_row: 1,
            end_row: 1,
        },
        hlsearch_enabled: true,
        hlsearch_suspended: false,
        incsearch_active: false,
        mode: SearchQueryMode::Hlsearch,
        pattern: Some("alpha".to_string()),
        input_pattern: None,
        matches: vec![SearchMatch {
            kind: SearchMatchKind::Current,
            start_row: 1,
            start_col: 0,
            end_row: 1,
            end_col: 5,
        }],
    };

    let model = project(
        &ProjectionInput::new(&snapshot, &session_state, None)
            .with_search_state(Some(&search_state)),
    );

    assert!(
        model.search_overlays.is_empty(),
        "別 window の search overlay は投影しないこと"
    );
}

#[test]
fn projection_input_keeps_explicit_failure_when_snapshot_has_no_active_window() {
    let _lock = session_test_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());

    let mut bridge = CoreBridge::new("alpha\nbeta\ngamma\n").expect("core bridge");
    bridge
        .apply_ex_command(":split")
        .expect("split should succeed");
    let mut snapshot = bridge.snapshot();
    for (index, window) in snapshot.windows.iter_mut().enumerate() {
        window.id = 41 + i32::try_from(index).expect("window index fits in i32");
        window.is_active = false;
    }
    snapshot.cursor_row = 7;
    snapshot.cursor_col = 11;
    let session_state = EditorSessionState::new(None);

    let input = ProjectionInput::new(&snapshot, &session_state, None);

    assert_eq!(
        input.window_id, 0,
        "active_window_id() が取れない snapshot では first window を採用せず explicit failure を保つこと"
    );
    assert_eq!(
        input.buffer_id, 0,
        "active window が解決できない場合は first window の buffer を流用しないこと"
    );
    assert_eq!(
        input.rect,
        PaneRect::default(),
        "active window が解決できない場合は geometry fallback を作らないこと"
    );
    assert_eq!(
        input.cursor_row, snapshot.cursor_row,
        "global cursor は snapshot の active cursor contract をそのまま使うこと"
    );
    assert_eq!(input.cursor_col, snapshot.cursor_col);
}

#[test]
fn workspace_projection_does_not_infer_active_pane_from_windows_scan() {
    let _lock = session_test_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());

    let mut bridge = CoreBridge::new("alpha\nbeta\ngamma\n").expect("core bridge");
    bridge
        .apply_ex_command(":split")
        .expect("split should succeed");
    let mut snapshot = bridge.snapshot();
    for (index, window) in snapshot.windows.iter_mut().enumerate() {
        window.id = 71 + i32::try_from(index).expect("window index fits in i32");
        window.is_active = false;
    }
    let session_state = EditorSessionState::new(None);
    let viewport_store = WindowViewportStore::new();
    let search_states = BTreeMap::new();
    let syntax_lines = BTreeMap::new();
    let markdown_document_maps = BTreeMap::new();

    let result = project_workspace(&WorkspaceProjectionInput {
        snapshot: &snapshot,
        light_snapshot: None,
        line_ranges: &BTreeMap::new(),
        session_state: &session_state,
        visual_selection: None,
        search_states: &search_states,
        syntax_lines: &syntax_lines,
        #[cfg(feature = "tree-sitter-syntax")]
        tree_sitter_syntax: &BTreeMap::new(),
        markdown_document_maps: &markdown_document_maps,
        command_preview: None,
        core_message: None,
        notification_prompt: None,
        system_warning: None,
        transient_info: None,
        viewport_store: &viewport_store,
        terminal_width: 80,
        terminal_height: 24,
    });

    assert_eq!(
        result,
        Err(WorkspaceProjectionError::ActiveWindowMissing),
        "active_window_id() が None のときは windows 走査で active pane を推測しないこと"
    );
}

#[test]
fn workspace_projection_uses_snapshot_cursor_for_active_pane_and_window_cursor_for_inactive_pane() {
    let _lock = session_test_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());

    let mut bridge = CoreBridge::new("alpha\nbeta\ngamma\ndelta\n").expect("core bridge");
    bridge
        .apply_ex_command(":split")
        .expect("split should succeed");
    let mut snapshot = bridge.snapshot();
    let active_window_id = snapshot
        .active_window_id()
        .expect("split snapshot should have an active window");
    let mut active_window_snapshot = snapshot
        .window(active_window_id)
        .expect("active window should exist")
        .clone();
    let mut inactive_window_snapshot = snapshot
        .windows
        .iter()
        .find(|window| window.id != active_window_id)
        .expect("inactive window should exist")
        .clone();
    snapshot.cursor_row = 2;
    snapshot.cursor_col = 2;
    active_window_snapshot.cursor_row = 4;
    active_window_snapshot.cursor_col = 1;
    inactive_window_snapshot.cursor_row = 3;
    inactive_window_snapshot.cursor_col = 3;
    snapshot.windows = vec![active_window_snapshot, inactive_window_snapshot];
    let session_state = EditorSessionState::new(None);
    let viewport_store = WindowViewportStore::new();
    let search_states = BTreeMap::new();
    let syntax_lines = BTreeMap::new();
    let markdown_document_maps = BTreeMap::new();

    let model = project_workspace(&WorkspaceProjectionInput {
        snapshot: &snapshot,
        light_snapshot: None,
        line_ranges: &BTreeMap::new(),
        session_state: &session_state,
        visual_selection: None,
        search_states: &search_states,
        syntax_lines: &syntax_lines,
        #[cfg(feature = "tree-sitter-syntax")]
        tree_sitter_syntax: &BTreeMap::new(),
        markdown_document_maps: &markdown_document_maps,
        command_preview: None,
        core_message: None,
        notification_prompt: None,
        system_warning: None,
        transient_info: None,
        viewport_store: &viewport_store,
        terminal_width: 80,
        terminal_height: 24,
    })
    .expect("workspace projection should still build for split snapshots");

    let active_pane = model
        .panes
        .iter()
        .find(|pane| pane.window_id == active_window_id)
        .expect("active pane should exist");
    let inactive_pane = model
        .panes
        .iter()
        .find(|pane| pane.window_id != active_window_id)
        .expect("inactive pane should exist");

    assert_eq!(
        active_pane.cursor_row, 2,
        "active pane は snapshot 全体の cursor_row を使うこと"
    );
    assert_eq!(
        active_pane.cursor_col, 2,
        "active pane は snapshot 全体の cursor_col を使うこと"
    );
    assert_eq!(
        inactive_pane.cursor_row, 3,
        "inactive pane は window metadata の cursor_row を使うこと"
    );
    assert_eq!(
        inactive_pane.cursor_col, 3,
        "inactive pane は window metadata の cursor_col を使うこと"
    );
}

#[test]
fn workspace_projection_keeps_full_height_when_global_rows_are_empty() {
    let _lock = session_test_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());

    let bridge = CoreBridge::new("alpha\nbeta\ngamma\n").expect("core bridge");
    let snapshot = bridge.snapshot();
    let session_state = EditorSessionState::new(None);
    let viewport_store = WindowViewportStore::new();
    let search_states = BTreeMap::new();
    let syntax_lines = BTreeMap::new();
    let markdown_document_maps = BTreeMap::new();

    let model = project_workspace(&WorkspaceProjectionInput {
        snapshot: &snapshot,
        light_snapshot: None,
        line_ranges: &BTreeMap::new(),
        session_state: &session_state,
        visual_selection: None,
        search_states: &search_states,
        syntax_lines: &syntax_lines,
        #[cfg(feature = "tree-sitter-syntax")]
        tree_sitter_syntax: &BTreeMap::new(),
        markdown_document_maps: &markdown_document_maps,
        command_preview: None,
        core_message: None,
        notification_prompt: None,
        system_warning: None,
        transient_info: None,
        viewport_store: &viewport_store,
        terminal_width: 80,
        terminal_height: 24,
    })
    .expect("workspace projection should succeed");

    let expected_height = snapshot.windows[0].height as u16;
    assert_eq!(
        model.panes[0].rect.height, expected_height,
        "message/command が空なら host が pane height を余計に削らないこと"
    );
}

#[test]
fn workspace_projection_passes_markdown_maps_into_pane_line_projections() {
    let _lock = session_test_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());

    let source = "# Title\n";
    let bridge = CoreBridge::new(source).expect("core bridge");
    let snapshot = bridge.snapshot();
    let window_id = snapshot.windows[0].id;
    let session_state = EditorSessionState::new(None);
    let viewport_store = WindowViewportStore::new();
    let search_states = BTreeMap::new();
    let syntax_lines = BTreeMap::new();
    let mut markdown_document_maps = BTreeMap::new();
    markdown_document_maps.insert(window_id, Arc::new(MarkdownDocumentMap::parse(source)));

    let model = project_workspace(&WorkspaceProjectionInput {
        snapshot: &snapshot,
        light_snapshot: None,
        line_ranges: &BTreeMap::new(),
        session_state: &session_state,
        visual_selection: None,
        search_states: &search_states,
        syntax_lines: &syntax_lines,
        #[cfg(feature = "tree-sitter-syntax")]
        tree_sitter_syntax: &BTreeMap::new(),
        markdown_document_maps: &markdown_document_maps,
        command_preview: None,
        core_message: None,
        notification_prompt: None,
        system_warning: None,
        transient_info: None,
        viewport_store: &viewport_store,
        terminal_width: 80,
        terminal_height: 24,
    })
    .expect("workspace projection should succeed");

    let pane = model
        .panes
        .iter()
        .find(|pane| pane.window_id == window_id)
        .expect("pane should exist");
    assert_eq!(pane.line_projections[0].raw_text, "# Title");
    assert_eq!(
        pane.line_projections[0].display_text, "# Title",
        "workspace projection should pass the per-window markdown map and active cursor block should remain raw"
    );
}

#[test]
fn workspace_projection_keeps_active_markdown_raw_expansion_out_of_inactive_panes() {
    let _lock = session_test_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());

    let source = "# Title\n*body*\n";
    let mut bridge = CoreBridge::new(source).expect("core bridge");
    bridge
        .apply_ex_command(":split")
        .expect("split should succeed");
    let mut snapshot = bridge.snapshot();
    snapshot.cursor_row = 0;
    let active_window_id = snapshot
        .active_window_id()
        .expect("split snapshot should have an active window");
    for window in &mut snapshot.windows {
        window.cursor_row = 0;
    }
    let session_state = EditorSessionState::new(None);
    let viewport_store = WindowViewportStore::new();
    let search_states = BTreeMap::new();
    let syntax_lines = BTreeMap::new();
    let markdown_map = Arc::new(MarkdownDocumentMap::parse(source));
    let markdown_document_maps = snapshot
        .windows
        .iter()
        .map(|window| (window.id, Arc::clone(&markdown_map)))
        .collect::<BTreeMap<_, _>>();

    let model = project_workspace(&WorkspaceProjectionInput {
        snapshot: &snapshot,
        light_snapshot: None,
        line_ranges: &BTreeMap::new(),
        session_state: &session_state,
        visual_selection: None,
        search_states: &search_states,
        syntax_lines: &syntax_lines,
        #[cfg(feature = "tree-sitter-syntax")]
        tree_sitter_syntax: &BTreeMap::new(),
        markdown_document_maps: &markdown_document_maps,
        command_preview: None,
        core_message: None,
        notification_prompt: None,
        system_warning: None,
        transient_info: None,
        viewport_store: &viewport_store,
        terminal_width: 80,
        terminal_height: 24,
    })
    .expect("workspace projection should succeed");

    let active_pane = model
        .panes
        .iter()
        .find(|pane| pane.window_id == active_window_id)
        .expect("active pane should exist");
    let inactive_pane = model
        .panes
        .iter()
        .find(|pane| pane.window_id != active_window_id)
        .expect("inactive pane should exist");

    assert_eq!(active_pane.line_projections[0].display_text, "# Title");
    assert_eq!(inactive_pane.line_projections[0].display_text, "Title");
}

#[test]
fn workspace_projection_without_markdown_map_keeps_raw_display_projection() {
    let _lock = session_test_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());

    let source = "# Title\n";
    let bridge = CoreBridge::new(source).expect("core bridge");
    let snapshot = bridge.snapshot();
    let session_state = EditorSessionState::new(None);
    let viewport_store = WindowViewportStore::new();
    let search_states = BTreeMap::new();
    let syntax_lines = BTreeMap::new();
    let markdown_document_maps = BTreeMap::new();

    let model = project_workspace(&WorkspaceProjectionInput {
        snapshot: &snapshot,
        light_snapshot: None,
        line_ranges: &BTreeMap::new(),
        session_state: &session_state,
        visual_selection: None,
        search_states: &search_states,
        syntax_lines: &syntax_lines,
        #[cfg(feature = "tree-sitter-syntax")]
        tree_sitter_syntax: &BTreeMap::new(),
        markdown_document_maps: &markdown_document_maps,
        command_preview: None,
        core_message: None,
        notification_prompt: None,
        system_warning: None,
        transient_info: None,
        viewport_store: &viewport_store,
        terminal_width: 80,
        terminal_height: 24,
    })
    .expect("workspace projection should succeed");

    assert_eq!(model.panes[0].line_projections[0].raw_text, "# Title");
    assert_eq!(
        model.panes[0].line_projections[0].display_text, "# Title",
        "without a markdown map, display projection should remain raw text"
    );
}

#[test]
fn workspace_projection_reserves_only_one_row_for_command_line_without_message() {
    let _lock = session_test_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());

    let bridge = CoreBridge::new("alpha\nbeta\ngamma\n").expect("core bridge");
    let snapshot = bridge.snapshot();
    let session_state = EditorSessionState::new(None);
    let viewport_store = WindowViewportStore::new();
    let search_states = BTreeMap::new();
    let syntax_lines = BTreeMap::new();
    let markdown_document_maps = BTreeMap::new();

    let model = project_workspace(&WorkspaceProjectionInput {
        snapshot: &snapshot,
        light_snapshot: None,
        line_ranges: &BTreeMap::new(),
        session_state: &session_state,
        visual_selection: None,
        search_states: &search_states,
        syntax_lines: &syntax_lines,
        #[cfg(feature = "tree-sitter-syntax")]
        tree_sitter_syntax: &BTreeMap::new(),
        markdown_document_maps: &markdown_document_maps,
        command_preview: Some(":w"),
        core_message: None,
        notification_prompt: None,
        system_warning: None,
        transient_info: None,
        viewport_store: &viewport_store,
        terminal_width: 80,
        terminal_height: 24,
    })
    .expect("workspace projection should succeed");

    let expected_height = snapshot.windows[0].height as u16;
    assert_eq!(
        model.panes[0].rect.height, expected_height,
        "command line だけの時も message row を重複予約せず core の pane height を保つこと"
    );
    assert_eq!(model.visible_message_text(), None);
}

#[test]
fn workspace_message_line_state_preserves_suppressed_notifications_while_command_preview_is_active()
{
    let _lock = session_test_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());

    let bridge = CoreBridge::new("alpha\n").expect("core bridge");
    let snapshot = bridge.snapshot();
    let session_state = EditorSessionState::new(None);
    let viewport_store = WindowViewportStore::new();
    let search_states = BTreeMap::new();
    let syntax_lines = BTreeMap::new();
    let markdown_document_maps = BTreeMap::new();
    let input = WorkspaceProjectionInput {
        snapshot: &snapshot,
        light_snapshot: None,
        line_ranges: &BTreeMap::new(),
        session_state: &session_state,
        visual_selection: None,
        search_states: &search_states,
        syntax_lines: &syntax_lines,
        #[cfg(feature = "tree-sitter-syntax")]
        tree_sitter_syntax: &BTreeMap::new(),
        markdown_document_maps: &markdown_document_maps,
        command_preview: Some(":%s/foo/bar"),
        core_message: Some("core note"),
        notification_prompt: None,
        system_warning: Some("system warning"),
        transient_info: Some("saved"),
        viewport_store: &viewport_store,
        terminal_width: 80,
        terminal_height: 24,
    };

    let state = resolve_workspace_message_line_state(&input);
    assert_eq!(
        state.visible_source(),
        Some(MessageLineSource::CommandPreview)
    );
    assert_eq!(state.visible_text(), Some(":%s/foo/bar"));
    assert_eq!(
        state.suppressed_sources(),
        vec![
            MessageLineSource::SystemWarning,
            MessageLineSource::CoreNotification,
            MessageLineSource::TransientInfo,
        ]
    );

    let model = project_workspace(&input).expect("workspace projection should succeed");
    assert_eq!(
        model.command_line.as_ref().map(|line| line.text.as_str()),
        Some(":%s/foo/bar")
    );
    assert_eq!(model.visible_message_text(), None);
}

#[test]
fn workspace_message_line_state_distinguishes_system_warning_from_core_notification() {
    let _lock = session_test_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());

    let bridge = CoreBridge::new("alpha\n").expect("core bridge");
    let snapshot = bridge.snapshot();
    let session_state = EditorSessionState::new(None);
    let viewport_store = WindowViewportStore::new();
    let search_states = BTreeMap::new();
    let syntax_lines = BTreeMap::new();
    let markdown_document_maps = BTreeMap::new();
    let input = WorkspaceProjectionInput {
        snapshot: &snapshot,
        light_snapshot: None,
        line_ranges: &BTreeMap::new(),
        session_state: &session_state,
        visual_selection: None,
        search_states: &search_states,
        syntax_lines: &syntax_lines,
        #[cfg(feature = "tree-sitter-syntax")]
        tree_sitter_syntax: &BTreeMap::new(),
        markdown_document_maps: &markdown_document_maps,
        command_preview: None,
        core_message: Some("shared text"),
        notification_prompt: None,
        system_warning: Some("shared text"),
        transient_info: None,
        viewport_store: &viewport_store,
        terminal_width: 80,
        terminal_height: 24,
    };

    let state = resolve_workspace_message_line_state(&input);
    assert_eq!(
        state.visible_source(),
        Some(MessageLineSource::SystemWarning)
    );
    assert_eq!(state.visible_text(), Some("shared text"));
    assert_eq!(
        state.suppressed_sources(),
        vec![MessageLineSource::CoreNotification]
    );

    let model = project_workspace(&input).expect("workspace projection should succeed");
    assert_eq!(model.visible_message_text(), Some("shared text"));
    assert_eq!(model.command_line, None);
}

#[test]
fn workspace_projection_summary_reports_windows_active_pane_geometry_and_visible_buffers() {
    let _lock = session_test_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());

    let mut bridge = CoreBridge::new("alpha\nbeta\ngamma\ndelta\n").expect("core bridge");
    bridge
        .apply_ex_command(":split")
        .expect("split should succeed");
    let snapshot = bridge.snapshot();
    let active_window_id = snapshot
        .active_window_id()
        .expect("split snapshot should have an active window");
    let session_state = EditorSessionState::new(None);
    let viewport_store = WindowViewportStore::new();
    let search_states = BTreeMap::new();
    let syntax_lines = BTreeMap::new();
    let markdown_document_maps = BTreeMap::new();

    let model = project_workspace(&WorkspaceProjectionInput {
        snapshot: &snapshot,
        light_snapshot: None,
        line_ranges: &BTreeMap::new(),
        session_state: &session_state,
        visual_selection: None,
        search_states: &search_states,
        syntax_lines: &syntax_lines,
        #[cfg(feature = "tree-sitter-syntax")]
        tree_sitter_syntax: &BTreeMap::new(),
        markdown_document_maps: &markdown_document_maps,
        command_preview: None,
        core_message: None,
        notification_prompt: None,
        system_warning: None,
        transient_info: None,
        viewport_store: &viewport_store,
        terminal_width: 80,
        terminal_height: 24,
    })
    .expect("workspace projection should succeed before summary is built");

    let summary = model.projection_summary();
    let expected_window_ids = model
        .panes
        .iter()
        .map(|pane| pane.window_id)
        .collect::<Vec<_>>();
    let expected_geometry = model
        .panes
        .iter()
        .map(|pane| PaneProjectionGeometry {
            window_id: pane.window_id,
            rect: pane.rect,
        })
        .collect::<Vec<_>>();
    let expected_visible_buffers = model
        .panes
        .iter()
        .map(|pane| pane.buffer_id)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();

    assert_eq!(summary.window_ids, expected_window_ids);
    assert_eq!(summary.active_window_id, active_window_id);
    assert_eq!(summary.pane_geometry, expected_geometry);
    assert_eq!(summary.visible_buffer_ids, expected_visible_buffers);
}

#[test]
fn projection_summary_ignores_message_prompt_and_rollback_lifecycle_state() {
    let pane = ScreenModel {
        window_id: 11,
        buffer_id: 21,
        rect: PaneRect {
            x: 1,
            y: 2,
            width: 30,
            height: 10,
        },
        file_name: "summary.txt".to_string(),
        mode_label: "NORMAL".to_string(),
        status_line: "test.txt | NORMAL".to_string(),
        cursor_style: ScreenCursorStyle::Block,
        dirty: false,
        lines: vec!["alpha".to_string()],
        line_projections: vec![],
        cursor_row: 0,
        cursor_col: 0,
        visual_selection: None,
        search_overlays: vec![],
        syntax_chunks: vec![],
        markdown_style_ranges: vec![],
        filer_style_ranges: vec![],
        resolved_theme: crate::presentation::theme::ResolvedTheme::default(),
        message_line: None,
        command_cursor_col: None,
        is_active: true,
    };
    let base = WorkspaceScreenModel {
        panes: vec![pane.clone()],
        floats: vec![],
        active_window_id: 11,
        message_line: WorkspaceMessageLineState::default(),
        message_area_height: 5,
        message_scroll_offset: 0,
        prompt_line: None,
        pager_prompt: None,
        suppressed_prompt_hints: vec![],
        bell: None,
        command_line: None,
    };
    let with_prompt_and_messages = WorkspaceScreenModel {
        panes: vec![pane],
        floats: vec![],
        active_window_id: 11,
        message_line: WorkspaceMessageLineState {
            visible: Some(MessageLineCandidate::legacy(
                MessageLineSource::CoreNotification,
                "visible message",
            )),
            suppressed: vec![MessageLineCandidate::legacy(
                MessageLineSource::TransientInfo,
                "hidden message",
            )],
        },
        message_area_height: 5,
        message_scroll_offset: 0,
        prompt_line: Some(InputPromptView {
            prompt: "prompt".to_string(),
            input: "typed".to_string(),
            correlation_id: 42,
            input_kind: CoreInputRequestKind::CommandLine,
            status: InputPromptStatus::Active,
        }),
        pager_prompt: None,
        suppressed_prompt_hints: vec![],
        bell: Some(BellIndication { count: 1 }),
        command_line: Some(CommandLineModel {
            text: ":write".to_string(),
            cursor_col: 6,
        }),
    };

    assert_eq!(
        base.projection_summary(),
        with_prompt_and_messages.projection_summary(),
        "projection summary は構造診断用なので message/prompt/rollback lifecycle を判断材料にしない"
    );
}
