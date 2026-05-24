use std::collections::BTreeMap;

use crate::runtime::config::{StartupRegistry, StartupRegistryEntry};

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ThemeTextStyleDeclaration {
    pub fg: Option<String>,
    pub bg: Option<String>,
    pub bold: Option<bool>,
    pub italic: Option<bool>,
    pub underline: Option<bool>,
    pub strikethrough: Option<bool>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum MarkdownSemanticStyleKey {
    Heading,
    Heading1,
    Heading2,
    Heading3,
    Heading4,
    Heading5,
    Heading6,
    InlineCode,
    Link,
    ListMarker,
    CheckboxChecked,
    CheckboxUnchecked,
    Table,
    FencedCodeBlock,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum UiStyleKey {
    Text,
    Gutter,
    StatusActive,
    StatusInactive,
    Message,
    WarningMsg,
    Prompt,
}

impl UiStyleKey {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "text" => Some(Self::Text),
            "gutter" => Some(Self::Gutter),
            "statusActive" => Some(Self::StatusActive),
            "statusInactive" => Some(Self::StatusInactive),
            "message" => Some(Self::Message),
            "warningMsg" => Some(Self::WarningMsg),
            "prompt" => Some(Self::Prompt),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum SyntaxSemanticStyleKey {
    Comment,
    String,
    Constant,
    Statement,
    Identifier,
    Type,
    Function,
    Punctuation,
    Markup,
    Default,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum FilerSemanticStyleKey {
    Directory,
    File,
    Symlink,
    Other,
    Marked,
}

impl FilerSemanticStyleKey {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "directory" => Some(Self::Directory),
            "file" => Some(Self::File),
            "symlink" => Some(Self::Symlink),
            "other" => Some(Self::Other),
            "marked" => Some(Self::Marked),
            _ => None,
        }
    }
}

impl SyntaxSemanticStyleKey {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "comment" => Some(Self::Comment),
            "string" => Some(Self::String),
            "constant" => Some(Self::Constant),
            "statement" => Some(Self::Statement),
            "identifier" => Some(Self::Identifier),
            "type" => Some(Self::Type),
            "function" => Some(Self::Function),
            "punctuation" => Some(Self::Punctuation),
            "markup" => Some(Self::Markup),
            "default" => Some(Self::Default),
            _ => None,
        }
    }
}

impl MarkdownSemanticStyleKey {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "heading" => Some(Self::Heading),
            "heading1" => Some(Self::Heading1),
            "heading2" => Some(Self::Heading2),
            "heading3" => Some(Self::Heading3),
            "heading4" => Some(Self::Heading4),
            "heading5" => Some(Self::Heading5),
            "heading6" => Some(Self::Heading6),
            "inlineCode" => Some(Self::InlineCode),
            "link" => Some(Self::Link),
            "listMarker" => Some(Self::ListMarker),
            "checkboxChecked" => Some(Self::CheckboxChecked),
            "checkboxUnchecked" => Some(Self::CheckboxUnchecked),
            "table" => Some(Self::Table),
            "fencedCodeBlock" => Some(Self::FencedCodeBlock),
            _ => None,
        }
    }

    pub fn from_heading_level(level: u8) -> Option<Self> {
        match level {
            1 => Some(Self::Heading1),
            2 => Some(Self::Heading2),
            3 => Some(Self::Heading3),
            4 => Some(Self::Heading4),
            5 => Some(Self::Heading5),
            6 => Some(Self::Heading6),
            _ => None,
        }
    }

    fn heading_level_keys() -> impl Iterator<Item = Self> {
        [
            Self::Heading1,
            Self::Heading2,
            Self::Heading3,
            Self::Heading4,
            Self::Heading5,
            Self::Heading6,
        ]
        .into_iter()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedThemeColor(pub String);

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ResolvedTextStyle {
    pub fg: Option<ResolvedThemeColor>,
    pub bg: Option<ResolvedThemeColor>,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub strikethrough: bool,
}

impl ResolvedTextStyle {
    pub fn is_empty(&self) -> bool {
        self.fg.is_none()
            && self.bg.is_none()
            && !self.bold
            && !self.italic
            && !self.underline
            && !self.strikethrough
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ResolvedTheme {
    ui: BTreeMap<UiStyleKey, ResolvedTextStyle>,
    syntax: BTreeMap<SyntaxSemanticStyleKey, ResolvedTextStyle>,
    language_syntax: BTreeMap<String, BTreeMap<SyntaxSemanticStyleKey, ResolvedTextStyle>>,
    filer: BTreeMap<FilerSemanticStyleKey, ResolvedTextStyle>,
    markdown: BTreeMap<MarkdownSemanticStyleKey, ResolvedTextStyle>,
}

impl ResolvedTheme {
    pub fn ui_style(&self, key: UiStyleKey) -> Option<&ResolvedTextStyle> {
        self.ui.get(&key)
    }

    pub fn syntax_style(&self, key: SyntaxSemanticStyleKey) -> Option<&ResolvedTextStyle> {
        self.syntax.get(&key)
    }

    pub fn syntax_style_for_language(
        &self,
        language: Option<&str>,
        key: SyntaxSemanticStyleKey,
    ) -> Option<ResolvedTextStyle> {
        language
            .and_then(normalize_language_id)
            .and_then(|language| self.language_syntax.get(&language))
            .and_then(|syntax| syntax.get(&key))
            .cloned()
            .or_else(|| self.syntax.get(&key).cloned())
            .filter(|style| !style.is_empty())
    }

    pub fn filer_style(&self, key: FilerSemanticStyleKey) -> Option<&ResolvedTextStyle> {
        self.filer.get(&key)
    }

    pub fn markdown_style(&self, key: MarkdownSemanticStyleKey) -> Option<&ResolvedTextStyle> {
        self.markdown.get(&key)
    }

    pub fn heading_style(&self, level: u8) -> Option<ResolvedTextStyle> {
        MarkdownSemanticStyleKey::from_heading_level(level)
            .and_then(|level_key| self.markdown.get(&level_key))
            .or_else(|| self.markdown.get(&MarkdownSemanticStyleKey::Heading))
            .cloned()
            .filter(|style| !style.is_empty())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ThemeRegistry {
    palette: BTreeMap<String, String>,
    ui: BTreeMap<UiStyleKey, ThemeTextStyleDeclaration>,
    syntax: BTreeMap<SyntaxSemanticStyleKey, ThemeTextStyleDeclaration>,
    language_syntax: BTreeMap<String, BTreeMap<SyntaxSemanticStyleKey, ThemeTextStyleDeclaration>>,
    filer: BTreeMap<FilerSemanticStyleKey, ThemeTextStyleDeclaration>,
    markdown: BTreeMap<MarkdownSemanticStyleKey, ThemeTextStyleDeclaration>,
}

impl ThemeRegistry {
    pub fn from_startup_registry(registry: &StartupRegistry) -> Self {
        let mut theme = Self::default();
        for entry in registry.entries() {
            match entry {
                StartupRegistryEntry::ThemePalette { name, value } => {
                    log::debug!(
                        "[theme] collect palette token from startup registry: name={}, value={}",
                        name,
                        value
                    );
                    theme.palette.insert(name.clone(), value.clone());
                }
                StartupRegistryEntry::ThemeMarkdownStyle { key, style } => {
                    log::debug!(
                        "[theme] collect markdown style from startup registry: key={key:?}, style={style:?}"
                    );
                    theme.markdown.insert(*key, style.clone());
                }
                StartupRegistryEntry::ThemeUiStyle { key, style } => {
                    log::debug!(
                        "[theme] collect ui style from startup registry: key={key:?}, style={style:?}"
                    );
                    theme.ui.insert(*key, style.clone());
                }
                StartupRegistryEntry::ThemeSyntaxStyle { key, style } => {
                    log::debug!(
                        "[theme] collect syntax style from startup registry: key={key:?}, style={style:?}"
                    );
                    theme.syntax.insert(*key, style.clone());
                }
                StartupRegistryEntry::ThemeLanguageSyntaxStyle {
                    language,
                    key,
                    style,
                } => {
                    let Some(language) = normalize_language_id(language) else {
                        log::debug!(
                            "[theme] ignored empty language syntax style from startup registry: key={key:?}"
                        );
                        continue;
                    };
                    log::debug!(
                        "[theme] collect language syntax style from startup registry: language={}, key={key:?}, style={style:?}",
                        language
                    );
                    theme
                        .language_syntax
                        .entry(language)
                        .or_default()
                        .insert(*key, style.clone());
                }
                StartupRegistryEntry::ThemeFilerStyle { key, style } => {
                    log::debug!(
                        "[theme] collect filer style from startup registry: key={key:?}, style={style:?}"
                    );
                    theme.filer.insert(*key, style.clone());
                }
                _ => {}
            }
        }
        theme
    }

    pub fn resolve(&self) -> ResolvedTheme {
        let ui = self
            .ui
            .iter()
            .map(|(key, style)| (*key, self.resolve_text_style(style)))
            .collect::<BTreeMap<_, _>>();
        let syntax = self
            .syntax
            .iter()
            .map(|(key, style)| (*key, self.resolve_text_style(style)))
            .collect::<BTreeMap<_, _>>();
        let language_syntax = self
            .language_syntax
            .iter()
            .map(|(language, syntax_styles)| {
                let resolved_styles = syntax_styles
                    .iter()
                    .map(|(key, style)| {
                        let mut resolved = self
                            .syntax
                            .get(key)
                            .map(|base| self.resolve_text_style(base))
                            .unwrap_or_default();
                        self.apply_text_style_override(&mut resolved, style);
                        (*key, resolved)
                    })
                    .collect::<BTreeMap<_, _>>();
                (language.clone(), resolved_styles)
            })
            .collect::<BTreeMap<_, _>>();
        let filer = self
            .filer
            .iter()
            .map(|(key, style)| (*key, self.resolve_text_style(style)))
            .collect::<BTreeMap<_, _>>();
        let mut markdown = self
            .markdown
            .iter()
            .map(|(key, style)| (*key, self.resolve_text_style(style)))
            .collect::<BTreeMap<_, _>>();
        for key in MarkdownSemanticStyleKey::heading_level_keys() {
            if let Some(style) = self.resolve_heading_level_style(key) {
                markdown.insert(key, style);
            }
        }
        log::debug!(
            "[theme] resolved theme: palette_tokens={}, ui_styles={}, syntax_styles={}, markdown_styles={}",
            self.palette.len(),
            ui.len(),
            syntax.len(),
            markdown.len()
        );
        ResolvedTheme {
            ui,
            syntax,
            language_syntax,
            filer,
            markdown,
        }
    }

    fn resolve_heading_level_style(
        &self,
        key: MarkdownSemanticStyleKey,
    ) -> Option<ResolvedTextStyle> {
        let heading = self.markdown.get(&MarkdownSemanticStyleKey::Heading);
        let level = self.markdown.get(&key);
        match (heading, level) {
            (None, None) => None,
            (Some(heading), None) => Some(self.resolve_text_style(heading)),
            (None, Some(level)) => Some(self.resolve_text_style(level)),
            (Some(heading), Some(level)) => {
                let mut inherited = self.resolve_text_style(heading);
                self.apply_text_style_override_with_log(&mut inherited, key, level);
                Some(inherited)
            }
        }
    }

    fn resolve_text_style(&self, style: &ThemeTextStyleDeclaration) -> ResolvedTextStyle {
        ResolvedTextStyle {
            fg: style
                .fg
                .as_deref()
                .and_then(|value| self.resolve_color(value)),
            bg: style
                .bg
                .as_deref()
                .and_then(|value| self.resolve_color(value)),
            bold: style.bold.unwrap_or(false),
            italic: style.italic.unwrap_or(false),
            underline: style.underline.unwrap_or(false),
            strikethrough: style.strikethrough.unwrap_or(false),
        }
    }

    fn apply_text_style_override_with_log(
        &self,
        inherited: &mut ResolvedTextStyle,
        key: MarkdownSemanticStyleKey,
        override_style: &ThemeTextStyleDeclaration,
    ) {
        if let Some(fg) = override_style.fg.as_deref() {
            if let Some(resolved) = self.resolve_color(fg) {
                inherited.fg = Some(resolved);
            } else {
                log::debug!(
                    "[theme] heading style override fg fell back to inherited color: key={key:?}, value={fg}"
                );
            }
        }
        if let Some(bg) = override_style.bg.as_deref() {
            if let Some(resolved) = self.resolve_color(bg) {
                inherited.bg = Some(resolved);
            } else {
                log::debug!(
                    "[theme] heading style override bg fell back to inherited color: key={key:?}, value={bg}"
                );
            }
        }
        if let Some(bold) = override_style.bold {
            inherited.bold = bold;
        }
        if let Some(italic) = override_style.italic {
            inherited.italic = italic;
        }
        if let Some(underline) = override_style.underline {
            inherited.underline = underline;
        }
        if let Some(strikethrough) = override_style.strikethrough {
            inherited.strikethrough = strikethrough;
        }
    }

    fn apply_text_style_override(
        &self,
        inherited: &mut ResolvedTextStyle,
        override_style: &ThemeTextStyleDeclaration,
    ) {
        if let Some(fg) = override_style
            .fg
            .as_deref()
            .and_then(|value| self.resolve_color(value))
        {
            inherited.fg = Some(fg);
        }
        if let Some(bg) = override_style
            .bg
            .as_deref()
            .and_then(|value| self.resolve_color(value))
        {
            inherited.bg = Some(bg);
        }
        if let Some(bold) = override_style.bold {
            inherited.bold = bold;
        }
        if let Some(italic) = override_style.italic {
            inherited.italic = italic;
        }
        if let Some(underline) = override_style.underline {
            inherited.underline = underline;
        }
        if let Some(strikethrough) = override_style.strikethrough {
            inherited.strikethrough = strikethrough;
        }
    }

    fn resolve_color(&self, value: &str) -> Option<ResolvedThemeColor> {
        if is_direct_hex_color(value) {
            return Some(ResolvedThemeColor(value.to_string()));
        }
        if let Some(resolved) = self.palette.get(value) {
            if is_direct_hex_color(resolved) {
                return Some(ResolvedThemeColor(resolved.clone()));
            }
            log::debug!(
                "[theme] palette token resolved to unsupported color value: token={}, value={}",
                value,
                resolved
            );
            return None;
        }
        log::debug!("[theme] unknown palette token ignored during theme resolution: {value}");
        None
    }
}

pub fn normalize_language_id(value: &str) -> Option<String> {
    match value.trim().to_ascii_lowercase().as_str() {
        "" => None,
        "go" | "golang" => Some("go".to_string()),
        "rs" | "rust" => Some("rust".to_string()),
        "ts" | "typescript" => Some("typescript".to_string()),
        "tsx" => Some("tsx".to_string()),
        "md" | "markdown" => Some("markdown".to_string()),
        "js" | "javascript" => Some("javascript".to_string()),
        "jsx" => Some("jsx".to_string()),
        other => Some(other.to_string()),
    }
}

fn is_direct_hex_color(value: &str) -> bool {
    let Some(hex) = value.strip_prefix('#') else {
        return false;
    };
    hex.len() == 6 && hex.bytes().all(|byte| byte.is_ascii_hexdigit())
}

#[cfg(test)]
mod tests {
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
        let registry =
            StartupRegistry::from_entries(vec![StartupRegistryEntry::ThemeMarkdownStyle {
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
}
