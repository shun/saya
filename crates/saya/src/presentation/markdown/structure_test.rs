use std::sync::Arc;

use super::*;

#[test]
fn parses_markdown_blocks_and_inline_ranges_without_changing_text() {
    let source = "\
# Title

- [x] done with *emphasis* and `code` plus [link](https://example.com)
- [ ] todo

```rust
*not emphasis*
```

| A | B |
|---|---|
| 1 | 2 |
";
    let original = source.to_string();

    let map = MarkdownDocumentMap::parse(source);

    assert_eq!(source, original);
    assert_eq!(
        map.blocks
            .iter()
            .map(|block| &block.kind)
            .collect::<Vec<_>>(),
        vec![
            &MarkdownBlockKind::Heading { level: 1 },
            &MarkdownBlockKind::ListItem {
                ordered: false,
                checkbox: Some(MarkdownCheckboxState::Checked),
            },
            &MarkdownBlockKind::ListItem {
                ordered: false,
                checkbox: Some(MarkdownCheckboxState::Unchecked),
            },
            &MarkdownBlockKind::FencedCodeBlock {
                fence: "```".to_string(),
                info: Some("rust".to_string()),
            },
            &MarkdownBlockKind::Table,
        ]
    );
    assert!(
        map.inline_ranges_by_kind(MarkdownInlineKindName::EmphasisMarker)
            .len()
            >= 2
    );
    assert_eq!(
        map.inline_ranges_by_kind(MarkdownInlineKindName::InlineCode)
            .len(),
        1
    );
    assert_eq!(
        map.inline_ranges_by_kind(MarkdownInlineKindName::Link)
            .len(),
        1
    );
    let link = map
        .inlines
        .iter()
        .find_map(|inline| match &inline.kind {
            MarkdownInlineKind::Link { text, destination } => {
                Some((inline.range, text, destination))
            }
            _ => None,
        })
        .expect("link metadata should include nested typed ranges");
    assert_eq!(link.0.start.line, 2);
    assert!(link.1.start.column < link.1.end.column);
    assert!(link.2.start.column < link.2.end.column);
    assert!(
        map.inlines
            .iter()
            .all(|inline| !matches!(inline.range.start.line, 6)),
        "inline metadata must not be extracted from fenced code contents"
    );
}

#[test]
fn cache_reuses_same_buffer_revision_and_invalidates_changed_revision() {
    let mut cache = MarkdownMetadataCache::new();
    let key = MarkdownMetadataKey {
        buffer_id: 7,
        revision: 10,
    };

    let first = cache.document_map(key, "# One\n");
    let second = cache.document_map(key, "# One\n");
    assert_eq!(first.status, MarkdownCacheStatus::Miss);
    assert_eq!(second.status, MarkdownCacheStatus::Hit);
    assert!(Arc::ptr_eq(&first.document_map, &second.document_map));

    let changed = cache.document_map(
        MarkdownMetadataKey {
            buffer_id: 7,
            revision: 11,
        },
        "# Two\n",
    );

    assert_eq!(changed.status, MarkdownCacheStatus::Invalidated);
    assert!(!Arc::ptr_eq(&first.document_map, &changed.document_map));
    assert_eq!(
        changed.document_map.blocks[0].kind,
        MarkdownBlockKind::Heading { level: 1 }
    );
}

#[test]
fn cache_hit_does_not_evaluate_deferred_source() {
    let mut cache = MarkdownMetadataCache::new();
    let key = MarkdownMetadataKey {
        buffer_id: 7,
        revision: 10,
    };

    let first = cache.document_map_with_source(key, || "# One\n".to_string());
    let second = cache.document_map_with_source(key, || {
        panic!("source text should not be fetched for a cache hit")
    });

    assert_eq!(first.status, MarkdownCacheStatus::Miss);
    assert_eq!(second.status, MarkdownCacheStatus::Hit);
    assert!(Arc::ptr_eq(&first.document_map, &second.document_map));
}

#[test]
fn cache_keeps_buffer_identities_separate_and_supports_explicit_invalidation() {
    let mut cache = MarkdownMetadataCache::new();
    let alpha = cache.document_map(
        MarkdownMetadataKey {
            buffer_id: 1,
            revision: 1,
        },
        "# Alpha\n",
    );
    let beta = cache.document_map(
        MarkdownMetadataKey {
            buffer_id: 2,
            revision: 1,
        },
        "- Beta\n",
    );

    assert!(!Arc::ptr_eq(&alpha.document_map, &beta.document_map));

    assert!(cache.invalidate_buffer(1));
    let refreshed = cache.document_map(
        MarkdownMetadataKey {
            buffer_id: 1,
            revision: 1,
        },
        "# Alpha\n",
    );
    assert_eq!(refreshed.status, MarkdownCacheStatus::Miss);
    assert!(!Arc::ptr_eq(&alpha.document_map, &refreshed.document_map));
}
