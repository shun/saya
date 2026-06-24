use super::*;

#[test]
fn registry_resolves_alias_owner_type_and_startup_public_contract() {
    let expandtab = SayaOptionRegistry::resolve("et").expect("expandtab alias");
    assert_eq!(expandtab.name, SayaOptionName::ExpandTab);
    assert_eq!(expandtab.value_type, SayaOptionType::Boolean);
    assert_eq!(expandtab.owner, SayaOptionOwner::CoreOwned);
    assert!(expandtab.startup_public);

    let cursorline = SayaOptionRegistry::resolve("cul").expect("cursorline alias");
    assert_eq!(cursorline.owner, SayaOptionOwner::PresentationOwned);

    let syntax = SayaOptionRegistry::resolve("syntax").expect("syntax option");
    assert_eq!(syntax.name, SayaOptionName::Syntax);
    assert_eq!(syntax.value_type, SayaOptionType::Boolean);
    assert_eq!(syntax.owner, SayaOptionOwner::CoreOwned);
    assert!(syntax.startup_public);

    let clipboard = SayaOptionRegistry::resolve("clipboard").expect("clipboard");
    assert_eq!(clipboard.owner, SayaOptionOwner::HostOwned);
    assert!(!clipboard.startup_public);

    let markdown_render =
        SayaOptionRegistry::resolve("markdownrender").expect("markdownrender option");
    assert_eq!(markdown_render.name, SayaOptionName::MarkdownRender);
    assert_eq!(markdown_render.value_type, SayaOptionType::Boolean);
    assert_eq!(markdown_render.owner, SayaOptionOwner::PresentationOwned);
    assert!(
        !markdown_render.startup_public,
        "Markdown render mode is command-controlled until the startup API is explicitly designed"
    );

    let mermaid_preview =
        SayaOptionRegistry::resolve("mermaidpreview").expect("mermaidpreview option");
    assert_eq!(mermaid_preview.name, SayaOptionName::MermaidPreview);
    assert_eq!(mermaid_preview.value_type, SayaOptionType::Boolean);
    assert_eq!(mermaid_preview.owner, SayaOptionOwner::PresentationOwned);
    assert!(
        mermaid_preview.startup_public,
        "Mermaid auto preview should be configurable from TypeScript startup"
    );

    let mermaid_preview_background = SayaOptionRegistry::resolve("mermaidpreviewbackground")
        .expect("mermaidpreviewbackground option");
    assert_eq!(
        mermaid_preview_background.name,
        SayaOptionName::MermaidPreviewBackground
    );
    assert_eq!(
        mermaid_preview_background.value_type,
        SayaOptionType::String
    );
    assert_eq!(
        mermaid_preview_background.owner,
        SayaOptionOwner::PresentationOwned
    );
    assert!(mermaid_preview_background.startup_public);

    let mermaid_preview_width =
        SayaOptionRegistry::resolve("mermaidpreviewwidth").expect("mermaidpreviewwidth option");
    assert_eq!(
        mermaid_preview_width.name,
        SayaOptionName::MermaidPreviewWidth
    );
    assert_eq!(mermaid_preview_width.value_type, SayaOptionType::Number);
    assert_eq!(
        mermaid_preview_width.owner,
        SayaOptionOwner::PresentationOwned
    );
    assert!(mermaid_preview_width.startup_public);

    let mermaid_preview_height =
        SayaOptionRegistry::resolve("mermaidpreviewheight").expect("mermaidpreviewheight option");
    assert_eq!(
        mermaid_preview_height.name,
        SayaOptionName::MermaidPreviewHeight
    );
    assert_eq!(mermaid_preview_height.value_type, SayaOptionType::Number);
    assert_eq!(
        mermaid_preview_height.owner,
        SayaOptionOwner::PresentationOwned
    );
    assert!(mermaid_preview_height.startup_public);
}

#[test]
fn parse_set_command_handles_boolean_forms_and_assignments() {
    assert_eq!(
        SayaOptionRegistry::parse_set_command(":set noet").expect("noet"),
        ParsedSayaSet {
            definition: SayaOptionRegistry::resolve("expandtab").unwrap(),
            operation: SayaSetOperation::Assign(SayaOptionValue::Boolean(false)),
        }
    );
    assert_eq!(
        SayaOptionRegistry::parse_set_command("set invwrap")
            .expect("invwrap")
            .operation,
        SayaSetOperation::Toggle
    );
    assert_eq!(
        SayaOptionRegistry::parse_set_command("set shiftwidth=4")
            .expect("shiftwidth")
            .operation,
        SayaSetOperation::Assign(SayaOptionValue::Number(4))
    );
    assert_eq!(
        SayaOptionRegistry::parse_set_command("set listchars=tab:>-,trail:-")
            .expect("listchars")
            .operation,
        SayaSetOperation::Assign(SayaOptionValue::String("tab:>-,trail:-".to_string()))
    );
    assert_eq!(
        SayaOptionRegistry::parse_set_command(":set nomarkdownrender")
            .expect("nomarkdownrender")
            .operation,
        SayaSetOperation::Assign(SayaOptionValue::Boolean(false))
    );
    assert_eq!(
        SayaOptionRegistry::parse_set_command(":set markdownrender!")
            .expect("markdownrender toggle")
            .operation,
        SayaSetOperation::Toggle
    );
    assert_eq!(
        SayaOptionRegistry::parse_set_command(":set nomermaidpreview")
            .expect("nomermaidpreview")
            .operation,
        SayaSetOperation::Assign(SayaOptionValue::Boolean(false))
    );
    assert_eq!(
        SayaOptionRegistry::parse_set_command(":set mermaidpreviewwidth=72")
            .expect("mermaidpreviewwidth")
            .operation,
        SayaSetOperation::Assign(SayaOptionValue::Number(72))
    );
    assert_eq!(
        SayaOptionRegistry::parse_set_command(":set mermaidpreviewheight=64")
            .expect("mermaidpreviewheight")
            .operation,
        SayaSetOperation::Assign(SayaOptionValue::Number(64))
    );
    assert_eq!(
        SayaOptionRegistry::parse_set_command(":set mermaidpreviewbackground=#ffffff")
            .expect("mermaidpreviewbackground")
            .operation,
        SayaSetOperation::Assign(SayaOptionValue::String("#ffffff".to_string()))
    );
}
