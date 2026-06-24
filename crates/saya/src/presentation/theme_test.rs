use super::*;

#[test]
fn resolves_palette_tokens_and_direct_hex_colors_without_exposing_token_names() {
    let registry = StartupRegistry::from_entries(vec![
        StartupRegistryEntry::ThemePalette {
            name: "accent".to_string(),
            value: "#7aa2f7".to_string(),
        },
        StartupRegistryEntry::ThemeMarkdownStyle {
            key: MarkdownSemanticStyleKey::Heading,
            style: ThemeTextStyleDeclaration {
                fg: Some("accent".to_string()),
                bg: Some("#111827".to_string()),
                bold: Some(true),
                ..ThemeTextStyleDeclaration::default()
            },
        },
    ]);

    let resolved = ThemeRegistry::from_startup_registry(&registry).resolve();
    let style = resolved
        .markdown_style(MarkdownSemanticStyleKey::Heading)
        .expect("heading style should resolve");

    assert_eq!(style.fg, Some(ResolvedThemeColor("#7aa2f7".to_string())));
    assert_eq!(style.bg, Some(ResolvedThemeColor("#111827".to_string())));
    assert!(style.bold);
}

#[test]
fn unknown_palette_tokens_fall_back_deterministically_without_panic() {
    let registry = StartupRegistry::from_entries(vec![StartupRegistryEntry::ThemeMarkdownStyle {
        key: MarkdownSemanticStyleKey::Link,
        style: ThemeTextStyleDeclaration {
            fg: Some("missingToken".to_string()),
            underline: Some(true),
            ..ThemeTextStyleDeclaration::default()
        },
    }]);

    let resolved = ThemeRegistry::from_startup_registry(&registry).resolve();
    let style = resolved
        .markdown_style(MarkdownSemanticStyleKey::Link)
        .expect("link style should still resolve");

    assert_eq!(style.fg, None);
    assert!(style.underline);
}

#[test]
fn heading_level_style_overrides_general_heading_style() {
    let registry = StartupRegistry::from_entries(vec![
        StartupRegistryEntry::ThemePalette {
            name: "accent".to_string(),
            value: "#7aa2f7".to_string(),
        },
        StartupRegistryEntry::ThemePalette {
            name: "heading2".to_string(),
            value: "#9ece6a".to_string(),
        },
        StartupRegistryEntry::ThemeMarkdownStyle {
            key: MarkdownSemanticStyleKey::Heading,
            style: ThemeTextStyleDeclaration {
                fg: Some("accent".to_string()),
                bold: Some(true),
                ..ThemeTextStyleDeclaration::default()
            },
        },
        StartupRegistryEntry::ThemeMarkdownStyle {
            key: MarkdownSemanticStyleKey::Heading2,
            style: ThemeTextStyleDeclaration {
                fg: Some("heading2".to_string()),
                underline: Some(true),
                ..ThemeTextStyleDeclaration::default()
            },
        },
    ]);

    let resolved = ThemeRegistry::from_startup_registry(&registry).resolve();
    let heading2 = resolved
        .heading_style(2)
        .expect("heading2 style should inherit heading");

    assert_eq!(heading2.fg, Some(ResolvedThemeColor("#9ece6a".to_string())));
    assert!(heading2.bold);
    assert!(heading2.underline);
}

#[test]
fn heading_level_style_can_disable_inherited_bold() {
    let registry = StartupRegistry::from_entries(vec![
        StartupRegistryEntry::ThemePalette {
            name: "accent".to_string(),
            value: "#7aa2f7".to_string(),
        },
        StartupRegistryEntry::ThemePalette {
            name: "heading2".to_string(),
            value: "#9ece6a".to_string(),
        },
        StartupRegistryEntry::ThemeMarkdownStyle {
            key: MarkdownSemanticStyleKey::Heading,
            style: ThemeTextStyleDeclaration {
                fg: Some("accent".to_string()),
                bold: Some(true),
                ..ThemeTextStyleDeclaration::default()
            },
        },
        StartupRegistryEntry::ThemeMarkdownStyle {
            key: MarkdownSemanticStyleKey::Heading2,
            style: ThemeTextStyleDeclaration {
                fg: Some("heading2".to_string()),
                bold: Some(false),
                underline: Some(true),
                ..ThemeTextStyleDeclaration::default()
            },
        },
    ]);

    let resolved = ThemeRegistry::from_startup_registry(&registry).resolve();
    let heading2 = resolved
        .heading_style(2)
        .expect("heading2 style should resolve");

    assert_eq!(heading2.fg, Some(ResolvedThemeColor("#9ece6a".to_string())));
    assert!(
        !heading2.bold,
        "heading2 bold=false should override heading bold=true"
    );
    assert!(heading2.underline);
}

#[test]
fn language_specific_syntax_style_overrides_global_syntax_fallback() {
    let registry = StartupRegistry::from_entries(vec![
        StartupRegistryEntry::ThemePalette {
            name: "purple".to_string(),
            value: "#bb9af7".to_string(),
        },
        StartupRegistryEntry::ThemeSyntaxStyle {
            key: SyntaxSemanticStyleKey::Function,
            style: ThemeTextStyleDeclaration {
                fg: Some("purple".to_string()),
                ..ThemeTextStyleDeclaration::default()
            },
        },
        StartupRegistryEntry::ThemeLanguageSyntaxStyle {
            language: "Go".to_string(),
            key: SyntaxSemanticStyleKey::Function,
            style: ThemeTextStyleDeclaration {
                fg: Some("#7aa2f7".to_string()),
                bold: Some(true),
                ..ThemeTextStyleDeclaration::default()
            },
        },
    ]);

    let resolved = ThemeRegistry::from_startup_registry(&registry).resolve();
    let go_function = resolved
        .syntax_style_for_language(Some("golang"), SyntaxSemanticStyleKey::Function)
        .expect("go function style should resolve");
    let rust_function = resolved
        .syntax_style_for_language(Some("rust"), SyntaxSemanticStyleKey::Function)
        .expect("rust function style should fall back to global syntax");

    assert_eq!(
        go_function.fg,
        Some(ResolvedThemeColor("#7aa2f7".to_string()))
    );
    assert!(go_function.bold);
    assert_eq!(
        rust_function.fg,
        Some(ResolvedThemeColor("#bb9af7".to_string()))
    );
    assert!(!rust_function.bold);
}

#[test]
fn filer_styles_resolve_separately_from_syntax_styles() {
    let registry = StartupRegistry::from_entries(vec![
        StartupRegistryEntry::ThemePalette {
            name: "accent".to_string(),
            value: "#7aa2f7".to_string(),
        },
        StartupRegistryEntry::ThemeFilerStyle {
            key: FilerSemanticStyleKey::Directory,
            style: ThemeTextStyleDeclaration {
                fg: Some("accent".to_string()),
                bold: Some(true),
                ..ThemeTextStyleDeclaration::default()
            },
        },
    ]);

    let resolved = ThemeRegistry::from_startup_registry(&registry).resolve();
    let directory = resolved
        .filer_style(FilerSemanticStyleKey::Directory)
        .expect("directory style should resolve");

    assert_eq!(
        directory.fg,
        Some(ResolvedThemeColor("#7aa2f7".to_string()))
    );
    assert!(directory.bold);
    assert!(
        resolved
            .syntax_style(SyntaxSemanticStyleKey::Function)
            .is_none()
    );
}
