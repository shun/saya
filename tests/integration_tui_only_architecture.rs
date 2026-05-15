//! 統合テスト: TUI-only architecture boundary の検証
//!
//! このファイルは `saya` の host/application TUI-only policy, dependency
//! compliance, terminal broker, and capability portability suite です。
//!
//! 責務は TUI-only 起動境界、GUI/GPU drift の品質ゲート、terminal session の
//! phase ownership、capability degrade、cross-environment portability に限定する。
//! 詳細な editing semantics は ADR 0001 に従って `vim-core-rs` に委ねる。

use std::io;

use saya::app::cli::LaunchRequest;
use saya::app::event_loop::EventLoopCoordinator;
use saya::app::startup::{prepare_launch_and_start_terminal, prepare_tui_startup_context};
use saya::core::notification_prompt::{
    MessageLineCandidate, MessageLineSource, resolve_workspace_message_line,
};
use saya::presentation::overlay::asset_store::{
    OverlayAssetMedia, OverlayAssetSource, OverlayAssetStore, OverlayAssetStoreService,
};
use saya::presentation::overlay::effect::{
    OverlayContentKey, OverlayTarget, PresentationEffectProjector,
    PresentationEffectProjectorService, RuntimePresentationIntent,
};
use saya::presentation::overlay::optional_graphics::{
    OptionalGraphicsAdapter, OverlayRenderResult, RecordingOverlayWriter,
};
use saya::presentation::render::coordinator::{
    RenderFrameRequest, RenderTextMode, TuiRenderCoordinator, TuiRenderCoordinatorService,
};
use saya::presentation::screen_model::{
    CommandLineModel, PaneRect, ScreenCursorStyle, ScreenModel, WorkspaceScreenModel,
};
use saya::presentation::ui_surface::{
    UiFeatureRequest, UiSurfaceMode, UiSurfacePolicy, UiSurfacePolicyService,
};
use saya::support::architecture_compliance::ArchitectureComplianceGuard;
use saya::terminal::capability::{
    CapabilityDegradationReason, InlineGraphicsProbeResult, TerminalCapabilityObservation,
    TerminalCapabilityProbe, TerminalCapabilityProbeService, TerminalSessionKind,
    TextStyleCapability,
};
use saya::terminal::input_loop::TerminalEventSource;
use saya::terminal::io_broker::TerminalIoPhase;
use saya::terminal::lifecycle::TerminalBackend;

fn tui_only_architecture_suite_scope_statement() -> &'static str {
    "host/application TUI-only architecture suite for startup policy, dependency drift guard, terminal phase ownership, capability degradation, and portability"
}

#[test]
fn tui_only_architecture_suite_scope_statement_stays_pinned_to_host_layer_boundaries() {
    let statement = tui_only_architecture_suite_scope_statement();

    assert!(statement.contains("TUI-only architecture suite"));
    assert!(statement.contains("startup policy"));
    assert!(statement.contains("dependency drift guard"));
    assert!(statement.contains("terminal phase ownership"));
    assert!(statement.contains("portability"));
    assert!(!statement.contains("editing semantics"));
}

#[test]
fn tui_surface_policy_pins_tui_only_mode_and_rejects_out_of_scope_surfaces() {
    let policy = UiSurfacePolicy::default();

    assert_eq!(policy.resolve_mode(), UiSurfaceMode::TuiOnly);
    assert_eq!(
        policy.validate_feature_request(UiFeatureRequest::CoreEditing),
        Ok(())
    );
    assert_eq!(
        policy.validate_feature_request(UiFeatureRequest::StyledText),
        Ok(())
    );
    assert_eq!(
        policy.validate_feature_request(UiFeatureRequest::InlineGraphics),
        Ok(())
    );
    assert!(
        policy
            .validate_feature_request(UiFeatureRequest::DedicatedGuiWindow)
            .is_err()
    );
    assert!(
        policy
            .validate_feature_request(UiFeatureRequest::DirectGpuRendering)
            .is_err()
    );
    assert!(
        policy
            .validate_feature_request(UiFeatureRequest::GuiCompatibility)
            .is_err()
    );
    assert!(
        policy
            .validate_feature_request(UiFeatureRequest::NeovimCompatibility)
            .is_err()
    );
}

#[test]
fn architecture_compliance_guard_rejects_gui_gpu_dependencies_from_manifest_and_lockfile() {
    let guard = ArchitectureComplianceGuard::default();
    let manifest = r#"
        [dependencies]
        ratatui = "0.29.0"
        fancy-ui = { package = "wgpu", version = "0.20" }
    "#;
    let lockfile = r#"
        [[package]]
        name = "egui"
        version = "0.31.0"
    "#;

    let error = guard
        .verify_dependencies(manifest, lockfile)
        .expect_err("banned GUI/GPU dependencies should be rejected");

    let rendered = error.to_string();
    assert!(rendered.contains("wgpu"));
    assert!(rendered.contains("egui"));
}

#[test]
fn architecture_compliance_guard_accepts_the_repository_dependency_set() {
    let guard = ArchitectureComplianceGuard::default();
    let manifest = std::fs::read_to_string("Cargo.toml").expect("Cargo.toml should be readable");
    let lockfile = std::fs::read_to_string("Cargo.lock").expect("Cargo.lock should be readable");

    guard
        .verify_dependencies(&manifest, &lockfile)
        .expect("current repository should satisfy the TUI-only dependency gate");
}

#[tokio::test(flavor = "current_thread")]
async fn prepare_launch_starts_a_tui_only_broker_and_requires_probe_before_interactive_input() {
    let _lock = saya::app::bootstrap::launch_test_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());

    #[derive(Default)]
    struct RecordingBackend {
        calls: Vec<&'static str>,
    }

    impl TerminalBackend for RecordingBackend {
        fn enable_raw_mode(&mut self) -> io::Result<()> {
            self.calls.push("enable_raw_mode");
            Ok(())
        }

        fn enter_alternate_screen(&mut self) -> io::Result<()> {
            self.calls.push("enter_alternate_screen");
            Ok(())
        }

        fn enable_mouse_capture(&mut self) -> io::Result<()> {
            self.calls.push("enable_mouse_capture");
            Ok(())
        }

        fn enable_bracketed_paste(&mut self) -> io::Result<()> {
            self.calls.push("enable_bracketed_paste");
            Ok(())
        }

        fn disable_bracketed_paste(&mut self) -> io::Result<()> {
            self.calls.push("disable_bracketed_paste");
            Ok(())
        }

        fn disable_mouse_capture(&mut self) -> io::Result<()> {
            self.calls.push("disable_mouse_capture");
            Ok(())
        }

        fn leave_alternate_screen(&mut self) -> io::Result<()> {
            self.calls.push("leave_alternate_screen");
            Ok(())
        }

        fn disable_raw_mode(&mut self) -> io::Result<()> {
            self.calls.push("disable_raw_mode");
            Ok(())
        }
    }

    struct IdleEventSource;

    impl TerminalEventSource for IdleEventSource {
        fn poll(&mut self, _timeout: std::time::Duration) -> io::Result<bool> {
            Ok(false)
        }

        fn read(&mut self) -> io::Result<crossterm::event::Event> {
            unreachable!("idle source should never be read")
        }
    }

    let mut backend = RecordingBackend::default();
    let (_outcome, mut broker) =
        prepare_launch_and_start_terminal(LaunchRequest::default(), &mut backend)
            .expect("startup should produce a TUI-only broker");

    assert_eq!(broker.surface_mode(), UiSurfaceMode::TuiOnly);
    assert_eq!(broker.phase(), TerminalIoPhase::Probe);

    let (coordinator, sender) = EventLoopCoordinator::new();
    drop(coordinator);
    let error = broker
        .start_interactive_input(sender.clone(), IdleEventSource)
        .expect_err("interactive input must not start before capability probe");
    let rendered = error.to_string();
    assert!(rendered.contains("probe"));

    let mut probe = TerminalCapabilityProbe::new(
        TerminalCapabilityObservation {
            session_kind: TerminalSessionKind::Local,
            basic_terminal_control: true,
            styled_text: true,
            color_text: true,
            truecolor: true,
        },
        InlineGraphicsProbeResult::Unsupported,
    );
    let profile = broker
        .run_probe(&mut probe)
        .expect("probe should succeed before interactive phase");
    assert_eq!(profile.text_style, TextStyleCapability::TrueColor);

    broker
        .start_interactive_input(sender, IdleEventSource)
        .expect("interactive input should start after probe");
    broker.request_shutdown();
    broker
        .shutdown()
        .await
        .expect("broker shutdown should restore terminal");

    assert_eq!(
        backend.calls,
        vec![
            "enable_raw_mode",
            "enter_alternate_screen",
            "enable_mouse_capture",
            "enable_bracketed_paste",
            "disable_bracketed_paste",
            "disable_mouse_capture",
            "leave_alternate_screen",
            "disable_raw_mode",
        ]
    );
}

#[test]
fn prepare_tui_startup_context_composes_policy_probe_and_runtime_owner_before_event_loop() {
    let _lock = saya::app::bootstrap::launch_test_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());

    #[derive(Default)]
    struct RecordingBackend {
        calls: Vec<&'static str>,
    }

    impl TerminalBackend for RecordingBackend {
        fn enable_raw_mode(&mut self) -> io::Result<()> {
            self.calls.push("enable_raw_mode");
            Ok(())
        }

        fn enter_alternate_screen(&mut self) -> io::Result<()> {
            self.calls.push("enter_alternate_screen");
            Ok(())
        }

        fn enable_mouse_capture(&mut self) -> io::Result<()> {
            self.calls.push("enable_mouse_capture");
            Ok(())
        }

        fn enable_bracketed_paste(&mut self) -> io::Result<()> {
            self.calls.push("enable_bracketed_paste");
            Ok(())
        }

        fn disable_bracketed_paste(&mut self) -> io::Result<()> {
            self.calls.push("disable_bracketed_paste");
            Ok(())
        }

        fn disable_mouse_capture(&mut self) -> io::Result<()> {
            self.calls.push("disable_mouse_capture");
            Ok(())
        }

        fn leave_alternate_screen(&mut self) -> io::Result<()> {
            self.calls.push("leave_alternate_screen");
            Ok(())
        }

        fn disable_raw_mode(&mut self) -> io::Result<()> {
            self.calls.push("disable_raw_mode");
            Ok(())
        }
    }

    let mut backend = RecordingBackend::default();
    let mut probe = TerminalCapabilityProbe::new(
        TerminalCapabilityObservation {
            session_kind: TerminalSessionKind::Local,
            basic_terminal_control: true,
            styled_text: true,
            color_text: true,
            truecolor: true,
        },
        InlineGraphicsProbeResult::Unsupported,
    );

    let startup = prepare_tui_startup_context(LaunchRequest::default(), &mut backend, &mut probe)
        .expect("startup context should compose launch, probe, and runtime wiring");

    assert_eq!(
        startup.terminal_broker.surface_mode(),
        UiSurfaceMode::TuiOnly
    );
    assert_eq!(startup.terminal_broker.phase(), TerminalIoPhase::Probe);
    assert_eq!(
        startup.capability_profile.session_kind,
        TerminalSessionKind::Local
    );
    assert_eq!(
        startup.capability_profile.text_style,
        TextStyleCapability::TrueColor
    );
    assert_eq!(
        startup.terminal_broker.capability_profile(),
        Some(&startup.capability_profile)
    );
    assert!(startup.runtime_init_message.is_none());
    assert!(startup.runtime_session.is_some());
}

#[test]
fn capability_probe_degrades_graphics_without_blocking_core_workflow_across_terminal_contexts() {
    let contexts = [
        TerminalSessionKind::Local,
        TerminalSessionKind::Ssh,
        TerminalSessionKind::Tmux,
        TerminalSessionKind::Container,
    ];

    for session_kind in contexts {
        let mut probe = TerminalCapabilityProbe::new(
            TerminalCapabilityObservation {
                session_kind,
                basic_terminal_control: true,
                styled_text: true,
                color_text: true,
                truecolor: false,
            },
            InlineGraphicsProbeResult::Timeout,
        );

        let profile = probe.detect();
        assert_eq!(profile.session_kind, session_kind);
        assert!(profile.maintains_core_workflow);
        assert!(!profile.requires_remote_gui_transport);
        assert_eq!(profile.text_style, TextStyleCapability::Ansi);
        assert_eq!(profile.inline_graphics, None);
        assert!(
            profile
                .degraded_reasons
                .contains(&CapabilityDegradationReason::GraphicsProbeTimedOut),
            "graphics timeout should degrade instead of blocking the editing path: {:?}",
            profile
        );
    }
}

#[test]
fn presentation_effect_projector_normalizes_runtime_overlay_requests_without_leaking_protocol_state()
 {
    let workspace = WorkspaceScreenModel {
        panes: vec![ScreenModel {
            window_id: 7,
            buffer_id: 9,
            rect: PaneRect {
                x: 0,
                y: 0,
                width: 40,
                height: 4,
            },
            file_name: "main.rs".to_string(),
            mode_label: "NORMAL".to_string(),
            cursor_style: ScreenCursorStyle::Block,
            dirty: false,
            lines: vec!["fn main() {}".to_string()],
            line_projections: vec![],
            cursor_row: 0,
            cursor_col: 0,
            visual_selection: None,
            search_overlays: vec![],
            syntax_chunks: vec![],
            markdown_style_ranges: vec![],
            resolved_theme: saya::presentation::theme::ResolvedTheme::default(),
            message_line: None,
            command_cursor_col: None,
            is_active: true,
        }],
        floats: vec![],
        active_window_id: 7,
        message_line: resolve_workspace_message_line(vec![MessageLineCandidate::legacy(
            MessageLineSource::SystemWarning,
            "existing warning",
        )]),
        message_area_height: 5,
        message_scroll_offset: 0,
        prompt_line: None,
        pager_prompt: None,
        suppressed_prompt_hints: vec![],
        bell: None,
        command_line: Some(CommandLineModel {
            text: ":write".to_string(),
            cursor_col: 2,
        }),
    };
    let runtime_intents = vec![RuntimePresentationIntent {
        content_key: OverlayContentKey::RuntimeRegistered {
            id: "runtime.preview".to_string(),
        },
        target: OverlayTarget::ActivePaneCorner,
        fallback_text: "preview unavailable".to_string(),
    }];
    let capabilities = TerminalCapabilityProbe::new(
        TerminalCapabilityObservation {
            session_kind: TerminalSessionKind::Local,
            basic_terminal_control: true,
            styled_text: true,
            color_text: true,
            truecolor: true,
        },
        InlineGraphicsProbeResult::Supported(
            saya::terminal::capability::InlineGraphicsProtocol::Kitty,
        ),
    )
    .detect();

    let projector = PresentationEffectProjector::default();
    let presentation = projector.project(&workspace, &runtime_intents, &capabilities);

    assert_eq!(
        presentation.visible_message_text(),
        Some("existing warning")
    );
    assert_eq!(
        presentation
            .command_line
            .as_ref()
            .map(|line| line.text.as_str()),
        Some(":write")
    );
    assert_eq!(presentation.overlays.len(), 1);
    assert_eq!(
        presentation.overlays[0].content_key,
        OverlayContentKey::RuntimeRegistered {
            id: "runtime.preview".to_string(),
        }
    );
    assert_eq!(
        presentation.overlays[0].target,
        OverlayTarget::ActivePaneCorner
    );
    assert_eq!(
        presentation.overlays[0].fallback_text,
        "preview unavailable"
    );
}

#[test]
fn overlay_asset_store_materializes_resolves_and_releases_session_scoped_assets() {
    let mut store = OverlayAssetStore::default();
    let key = OverlayContentKey::RuntimeRegistered {
        id: "runtime.preview".to_string(),
    };
    store.register_asset(
        key.clone(),
        OverlayAssetSource::Static(OverlayAssetMedia::png(
            "preview",
            16,
            8,
            b"png-binary".to_vec(),
        )),
    );

    let asset_ref = store
        .materialize(&key)
        .expect("registered asset should materialize");
    let snapshot = store
        .resolve(&asset_ref)
        .expect("materialized asset should resolve");

    assert_eq!(snapshot.metadata.alt_text, "preview");
    assert_eq!(snapshot.metadata.pixel_width, 16);
    assert_eq!(snapshot.bytes, b"png-binary");

    store.release_unused(&[]);

    let error = store
        .resolve(&asset_ref)
        .expect_err("released asset should no longer resolve");
    assert!(error.to_string().contains("runtime.preview"));
}

#[test]
fn tui_render_coordinator_keeps_text_grid_on_plain_styled_and_graphics_fallback_paths() {
    let workspace = WorkspaceScreenModel {
        panes: vec![ScreenModel {
            window_id: 1,
            buffer_id: 1,
            rect: PaneRect {
                x: 0,
                y: 0,
                width: 40,
                height: 4,
            },
            file_name: "sample.txt".to_string(),
            mode_label: "NORMAL".to_string(),
            cursor_style: ScreenCursorStyle::Block,
            dirty: false,
            lines: vec!["alpha".to_string(), "beta".to_string()],
            line_projections: vec![],
            cursor_row: 1,
            cursor_col: 2,
            visual_selection: None,
            search_overlays: vec![],
            syntax_chunks: vec![],
            markdown_style_ranges: vec![],
            resolved_theme: saya::presentation::theme::ResolvedTheme::default(),
            message_line: None,
            command_cursor_col: None,
            is_active: true,
        }],
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
    let projector = PresentationEffectProjector::default();
    let runtime_intents = vec![RuntimePresentationIntent {
        content_key: OverlayContentKey::RuntimeRegistered {
            id: "runtime.preview".to_string(),
        },
        target: OverlayTarget::StatusArea,
        fallback_text: "preview unavailable".to_string(),
    }];
    let mut store = OverlayAssetStore::default();
    store.register_asset(
        OverlayContentKey::RuntimeRegistered {
            id: "runtime.preview".to_string(),
        },
        OverlayAssetSource::Static(OverlayAssetMedia::png("preview", 8, 4, b"preview".to_vec())),
    );
    let plain_capabilities = TerminalCapabilityProbe::new(
        TerminalCapabilityObservation {
            session_kind: TerminalSessionKind::Local,
            basic_terminal_control: true,
            styled_text: false,
            color_text: false,
            truecolor: false,
        },
        InlineGraphicsProbeResult::Disabled,
    )
    .detect();
    let styled_capabilities = TerminalCapabilityProbe::new(
        TerminalCapabilityObservation {
            session_kind: TerminalSessionKind::Local,
            basic_terminal_control: true,
            styled_text: true,
            color_text: true,
            truecolor: true,
        },
        InlineGraphicsProbeResult::Unsupported,
    )
    .detect();
    let monochrome_capabilities = TerminalCapabilityProbe::new(
        TerminalCapabilityObservation {
            session_kind: TerminalSessionKind::Local,
            basic_terminal_control: true,
            styled_text: true,
            color_text: false,
            truecolor: false,
        },
        InlineGraphicsProbeResult::Unsupported,
    )
    .detect();
    let graphics_capabilities = TerminalCapabilityProbe::new(
        TerminalCapabilityObservation {
            session_kind: TerminalSessionKind::Local,
            basic_terminal_control: true,
            styled_text: true,
            color_text: true,
            truecolor: true,
        },
        InlineGraphicsProbeResult::Supported(
            saya::terminal::capability::InlineGraphicsProtocol::Kitty,
        ),
    )
    .detect();
    let adapter = OptionalGraphicsAdapter::new_failing_for_tests();

    let plain_presentation = projector.project(&workspace, &runtime_intents, &plain_capabilities);
    let styled_presentation = projector.project(&workspace, &runtime_intents, &styled_capabilities);
    let monochrome_presentation =
        projector.project(&workspace, &runtime_intents, &monochrome_capabilities);
    let graphics_presentation =
        projector.project(&workspace, &runtime_intents, &graphics_capabilities);

    let mut writer = RecordingOverlayWriter::default();
    let mut coordinator = TuiRenderCoordinator::new_for_tests(store, adapter);

    let plain = coordinator
        .render_workspace(RenderFrameRequest {
            workspace: &workspace,
            capabilities: &plain_capabilities,
            presentation: &plain_presentation,
            overlay_writer: Some(&mut writer),
            redraw_plan: None,
        })
        .expect("plain render should succeed");
    let styled = coordinator
        .render_workspace(RenderFrameRequest {
            workspace: &workspace,
            capabilities: &styled_capabilities,
            presentation: &styled_presentation,
            overlay_writer: Some(&mut writer),
            redraw_plan: None,
        })
        .expect("styled render should succeed");
    let monochrome = coordinator
        .render_workspace(RenderFrameRequest {
            workspace: &workspace,
            capabilities: &monochrome_capabilities,
            presentation: &monochrome_presentation,
            overlay_writer: Some(&mut writer),
            redraw_plan: None,
        })
        .expect("monochrome render should succeed");
    let graphics = coordinator
        .render_workspace(RenderFrameRequest {
            workspace: &workspace,
            capabilities: &graphics_capabilities,
            presentation: &graphics_presentation,
            overlay_writer: Some(&mut writer),
            redraw_plan: None,
        })
        .expect("graphics fallback render should succeed");

    assert_eq!(plain.text_mode, RenderTextMode::Plain);
    assert_eq!(monochrome.text_mode, RenderTextMode::StyledMonochrome);
    assert_eq!(styled.text_mode, RenderTextMode::StyledTrueColor);
    assert_eq!(graphics.text_mode, RenderTextMode::StyledTrueColor);
    assert_eq!(
        plain.rendered_workspace.panes[0].lines,
        workspace.panes[0].lines
    );
    assert_eq!(
        styled.rendered_workspace.panes[0].lines,
        workspace.panes[0].lines
    );
    assert_eq!(
        monochrome.rendered_workspace.panes[0].lines,
        workspace.panes[0].lines
    );
    assert_eq!(
        graphics.rendered_workspace.panes[0].lines,
        workspace.panes[0].lines
    );
    assert_eq!(
        graphics.overlay_results,
        vec![OverlayRenderResult::FallbackToText]
    );
    assert_eq!(
        graphics.rendered_workspace.visible_message_text(),
        Some("preview unavailable")
    );
    assert!(writer.writes.is_empty());
}

#[test]
fn layered_architecture_keeps_terminal_protocols_out_of_runtime_and_public_surface() {
    let runtime_source =
        std::fs::read_to_string("src/runtime/integration.rs").expect("runtime source should load");
    let public_declaration = saya::RUNTIME_SAYA_TYPE_DECLARATION
        .to_ascii_lowercase()
        .replace("estimatedbytes", "");
    let graphics_source = std::fs::read_to_string("src/presentation/overlay/optional_graphics.rs")
        .expect("graphics adapter source should load");

    for forbidden in [
        "InlineGraphicsProtocol",
        "GraphicsOverlayRequest",
        "OverlayAssetSnapshot",
        "TerminalIoBroker",
    ] {
        assert!(
            !runtime_source.contains(forbidden),
            "runtime integration should stay transport-agnostic: {forbidden}"
        );
    }
    for forbidden in ["overlay", "graphics", "protocol", "bytes", "kitty", "sixel"] {
        assert!(
            !public_declaration.contains(forbidden),
            "public runtime declaration should keep transport details private: {forbidden}"
        );
    }
    assert!(
        !graphics_source.contains("vim_core_rs"),
        "graphics adapter should not take ownership of editing semantics"
    );
}
