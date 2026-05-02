use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct MarkdownPosition {
    pub line: usize,
    pub column: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MarkdownTextRange {
    pub start: MarkdownPosition,
    pub end: MarkdownPosition,
}

impl MarkdownTextRange {
    fn single_line(line: usize, start_column: usize, end_column: usize) -> Self {
        Self {
            start: MarkdownPosition {
                line,
                column: start_column,
            },
            end: MarkdownPosition {
                line,
                column: end_column,
            },
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MarkdownBlock {
    pub kind: MarkdownBlockKind,
    pub range: MarkdownTextRange,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MarkdownBlockKind {
    Heading {
        level: u8,
    },
    ListItem {
        ordered: bool,
        checkbox: Option<MarkdownCheckboxState>,
    },
    FencedCodeBlock {
        fence: String,
        info: Option<String>,
    },
    Table,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MarkdownCheckboxState {
    Checked,
    Unchecked,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MarkdownInline {
    pub kind: MarkdownInlineKind,
    pub range: MarkdownTextRange,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MarkdownInlineKind {
    EmphasisMarker {
        marker: String,
    },
    InlineCode,
    Link {
        text: MarkdownTextRange,
        destination: MarkdownTextRange,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MarkdownInlineKindName {
    EmphasisMarker,
    InlineCode,
    Link,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MarkdownDocumentMap {
    pub blocks: Vec<MarkdownBlock>,
    pub inlines: Vec<MarkdownInline>,
    pub line_count: usize,
    pub byte_len: usize,
}

impl MarkdownDocumentMap {
    pub fn parse(source: &str) -> Self {
        let started_at = Instant::now();
        let lines = source.lines().collect::<Vec<_>>();
        let mut blocks = Vec::new();
        let mut inlines = Vec::new();
        let mut line_index = 0;

        while line_index < lines.len() {
            let line = lines[line_index];

            if let Some(opening_fence) = parse_fence_opening(line) {
                let start_line = line_index;
                let mut end_line = line_index;
                while end_line + 1 < lines.len() {
                    end_line += 1;
                    if is_fence_closing(lines[end_line], &opening_fence.marker) {
                        break;
                    }
                }
                blocks.push(MarkdownBlock {
                    kind: MarkdownBlockKind::FencedCodeBlock {
                        fence: opening_fence.marker,
                        info: opening_fence.info,
                    },
                    range: MarkdownTextRange {
                        start: MarkdownPosition {
                            line: start_line,
                            column: 0,
                        },
                        end: MarkdownPosition {
                            line: end_line,
                            column: lines[end_line].len(),
                        },
                    },
                });
                line_index = end_line + 1;
                continue;
            }

            if let Some(table_end) = parse_table_block(&lines, line_index) {
                blocks.push(MarkdownBlock {
                    kind: MarkdownBlockKind::Table,
                    range: MarkdownTextRange {
                        start: MarkdownPosition {
                            line: line_index,
                            column: 0,
                        },
                        end: MarkdownPosition {
                            line: table_end,
                            column: lines[table_end].len(),
                        },
                    },
                });
                for (inline_line, inline_source) in lines
                    .iter()
                    .enumerate()
                    .take(table_end + 1)
                    .skip(line_index)
                {
                    parse_inlines(inline_source, inline_line, &mut inlines);
                }
                line_index = table_end + 1;
                continue;
            }

            if let Some(level) = parse_heading_level(line) {
                blocks.push(MarkdownBlock {
                    kind: MarkdownBlockKind::Heading { level },
                    range: MarkdownTextRange::single_line(line_index, 0, line.len()),
                });
            } else if let Some(list_item) = parse_list_item(line) {
                blocks.push(MarkdownBlock {
                    kind: MarkdownBlockKind::ListItem {
                        ordered: list_item.ordered,
                        checkbox: list_item.checkbox,
                    },
                    range: MarkdownTextRange::single_line(line_index, 0, line.len()),
                });
            }

            parse_inlines(line, line_index, &mut inlines);
            line_index += 1;
        }

        let map = Self {
            blocks,
            inlines,
            line_count: lines.len(),
            byte_len: source.len(),
        };
        log_parse_summary(&map, started_at.elapsed());
        map
    }

    pub fn inline_ranges_by_kind(
        &self,
        kind_name: MarkdownInlineKindName,
    ) -> Vec<MarkdownTextRange> {
        self.inlines
            .iter()
            .filter(|inline| inline.kind.name() == kind_name)
            .map(|inline| inline.range)
            .collect()
    }
}

impl MarkdownInlineKind {
    fn name(&self) -> MarkdownInlineKindName {
        match self {
            MarkdownInlineKind::EmphasisMarker { .. } => MarkdownInlineKindName::EmphasisMarker,
            MarkdownInlineKind::InlineCode => MarkdownInlineKindName::InlineCode,
            MarkdownInlineKind::Link { .. } => MarkdownInlineKindName::Link,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MarkdownMetadataKey {
    pub buffer_id: i64,
    pub revision: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MarkdownCacheStatus {
    Hit,
    Miss,
    Invalidated,
}

#[derive(Debug, Clone)]
pub struct MarkdownCacheOutcome {
    pub key: MarkdownMetadataKey,
    pub status: MarkdownCacheStatus,
    pub document_map: Arc<MarkdownDocumentMap>,
}

#[derive(Debug, Default)]
pub struct MarkdownMetadataCache {
    entries: BTreeMap<i64, CachedMarkdownDocumentMap>,
}

impl MarkdownMetadataCache {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn document_map(&mut self, key: MarkdownMetadataKey, source: &str) -> MarkdownCacheOutcome {
        if let Some(entry) = self.entries.get(&key.buffer_id) {
            if entry.revision == key.revision {
                log::debug!(
                    "[markdown_structure] cache hit: buffer_id={}, revision={}, block_count={}, inline_count={}",
                    key.buffer_id,
                    key.revision,
                    entry.document_map.blocks.len(),
                    entry.document_map.inlines.len()
                );
                return MarkdownCacheOutcome {
                    key,
                    status: MarkdownCacheStatus::Hit,
                    document_map: Arc::clone(&entry.document_map),
                };
            }

            log::debug!(
                "[markdown_structure] cache invalidated by revision: buffer_id={}, cached_revision={}, requested_revision={}",
                key.buffer_id,
                entry.revision,
                key.revision
            );
            let document_map = Arc::new(MarkdownDocumentMap::parse(source));
            self.entries.insert(
                key.buffer_id,
                CachedMarkdownDocumentMap {
                    revision: key.revision,
                    document_map: Arc::clone(&document_map),
                },
            );
            return MarkdownCacheOutcome {
                key,
                status: MarkdownCacheStatus::Invalidated,
                document_map,
            };
        }

        log::debug!(
            "[markdown_structure] cache miss: buffer_id={}, revision={}",
            key.buffer_id,
            key.revision
        );
        let document_map = Arc::new(MarkdownDocumentMap::parse(source));
        self.entries.insert(
            key.buffer_id,
            CachedMarkdownDocumentMap {
                revision: key.revision,
                document_map: Arc::clone(&document_map),
            },
        );
        MarkdownCacheOutcome {
            key,
            status: MarkdownCacheStatus::Miss,
            document_map,
        }
    }

    pub fn invalidate_buffer(&mut self, buffer_id: i64) -> bool {
        let removed = self.entries.remove(&buffer_id).is_some();
        log::debug!(
            "[markdown_structure] explicit buffer invalidation: buffer_id={}, removed={}",
            buffer_id,
            removed
        );
        removed
    }
}

#[derive(Debug, Clone)]
struct CachedMarkdownDocumentMap {
    revision: u64,
    document_map: Arc<MarkdownDocumentMap>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct FenceOpening {
    marker: String,
    info: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ListItem {
    ordered: bool,
    checkbox: Option<MarkdownCheckboxState>,
}

fn log_parse_summary(map: &MarkdownDocumentMap, elapsed: Duration) {
    log::debug!(
        "[markdown_structure] parse completed: duration_us={}, line_count={}, byte_len={}, block_count={}, inline_count={}, visible_block_count={}",
        elapsed.as_micros(),
        map.line_count,
        map.byte_len,
        map.blocks.len(),
        map.inlines.len(),
        map.blocks.len()
    );
}

fn parse_heading_level(line: &str) -> Option<u8> {
    let trimmed = line.trim_start();
    let level = trimmed.bytes().take_while(|byte| *byte == b'#').count();
    if (1..=6).contains(&level) && trimmed.as_bytes().get(level) == Some(&b' ') {
        Some(level as u8)
    } else {
        None
    }
}

fn parse_list_item(line: &str) -> Option<ListItem> {
    let trimmed = line.trim_start();
    let mut marker_len = 0;
    let mut ordered = false;

    if matches!(trimmed.as_bytes().first(), Some(b'-' | b'+' | b'*'))
        && trimmed.as_bytes().get(1) == Some(&b' ')
    {
        marker_len = 2;
    } else {
        let digit_count = trimmed
            .bytes()
            .take_while(|byte| byte.is_ascii_digit())
            .count();
        if digit_count > 0
            && trimmed.as_bytes().get(digit_count) == Some(&b'.')
            && trimmed.as_bytes().get(digit_count + 1) == Some(&b' ')
        {
            marker_len = digit_count + 2;
            ordered = true;
        }
    }

    if marker_len == 0 {
        return None;
    }

    let checkbox = parse_checkbox(trimmed.get(marker_len..).unwrap_or_default());
    Some(ListItem { ordered, checkbox })
}

fn parse_checkbox(text_after_marker: &str) -> Option<MarkdownCheckboxState> {
    match text_after_marker.as_bytes().get(0..3) {
        Some(b"[ ]") => Some(MarkdownCheckboxState::Unchecked),
        Some(b"[x]") | Some(b"[X]") => Some(MarkdownCheckboxState::Checked),
        _ => None,
    }
}

fn parse_fence_opening(line: &str) -> Option<FenceOpening> {
    let trimmed = line.trim_start();
    let marker_char = match trimmed.as_bytes().first() {
        Some(b'`') => b'`',
        Some(b'~') => b'~',
        _ => return None,
    };
    let marker_len = trimmed
        .bytes()
        .take_while(|byte| *byte == marker_char)
        .count();
    if marker_len < 3 {
        return None;
    }

    let info = trimmed
        .get(marker_len..)
        .map(str::trim)
        .filter(|info| !info.is_empty())
        .map(ToOwned::to_owned);
    Some(FenceOpening {
        marker: String::from_utf8(vec![marker_char; marker_len]).expect("ASCII fence marker"),
        info,
    })
}

fn is_fence_closing(line: &str, marker: &str) -> bool {
    let trimmed = line.trim_start();
    trimmed.starts_with(marker)
        && trimmed
            .get(marker.len()..)
            .map(str::trim)
            .unwrap_or_default()
            .is_empty()
}

fn parse_table_block(lines: &[&str], start: usize) -> Option<usize> {
    if start + 1 >= lines.len() || !looks_like_table_row(lines[start]) {
        return None;
    }
    if !looks_like_table_delimiter(lines[start + 1]) {
        return None;
    }

    let mut end = start + 1;
    while end + 1 < lines.len() && looks_like_table_row(lines[end + 1]) {
        end += 1;
    }
    Some(end)
}

fn looks_like_table_row(line: &str) -> bool {
    let trimmed = line.trim();
    trimmed.contains('|') && !trimmed.is_empty()
}

fn looks_like_table_delimiter(line: &str) -> bool {
    let trimmed = line.trim().trim_matches('|');
    let cells = trimmed.split('|').map(str::trim).collect::<Vec<_>>();
    cells.len() >= 2
        && cells.iter().all(|cell| {
            let core = cell.trim_matches(':');
            !core.is_empty() && core.bytes().all(|byte| byte == b'-')
        })
}

fn parse_inlines(line: &str, line_index: usize, inlines: &mut Vec<MarkdownInline>) {
    let mut covered_ranges = Vec::new();
    parse_inline_code(line, line_index, inlines, &mut covered_ranges);
    parse_links(line, line_index, inlines, &mut covered_ranges);
    parse_emphasis_markers(line, line_index, inlines, &covered_ranges);
}

fn parse_inline_code(
    line: &str,
    line_index: usize,
    inlines: &mut Vec<MarkdownInline>,
    covered_ranges: &mut Vec<(usize, usize)>,
) {
    let bytes = line.as_bytes();
    let mut cursor = 0;
    while cursor < bytes.len() {
        if bytes[cursor] != b'`' {
            cursor += 1;
            continue;
        }
        if let Some(relative_end) = line[cursor + 1..].find('`') {
            let end = cursor + 1 + relative_end + 1;
            let range = MarkdownTextRange::single_line(line_index, cursor, end);
            inlines.push(MarkdownInline {
                kind: MarkdownInlineKind::InlineCode,
                range,
            });
            covered_ranges.push((cursor, end));
            cursor = end;
        } else {
            cursor += 1;
        }
    }
}

fn parse_links(
    line: &str,
    line_index: usize,
    inlines: &mut Vec<MarkdownInline>,
    covered_ranges: &mut Vec<(usize, usize)>,
) {
    let bytes = line.as_bytes();
    let mut cursor = 0;
    while cursor < bytes.len() {
        if bytes[cursor] != b'[' || is_byte_covered(cursor, covered_ranges) {
            cursor += 1;
            continue;
        }

        let Some(text_end_relative) = line[cursor + 1..].find(']') else {
            cursor += 1;
            continue;
        };
        let text_end = cursor + 1 + text_end_relative;
        if bytes.get(text_end + 1) != Some(&b'(') {
            cursor += 1;
            continue;
        }
        let Some(destination_end_relative) = line[text_end + 2..].find(')') else {
            cursor += 1;
            continue;
        };
        let destination_end = text_end + 2 + destination_end_relative;
        let full_end = destination_end + 1;
        let range = MarkdownTextRange::single_line(line_index, cursor, full_end);
        let text = MarkdownTextRange::single_line(line_index, cursor + 1, text_end);
        let destination = MarkdownTextRange::single_line(line_index, text_end + 2, destination_end);
        inlines.push(MarkdownInline {
            kind: MarkdownInlineKind::Link { text, destination },
            range,
        });
        covered_ranges.push((cursor, full_end));
        cursor = full_end;
    }
}

fn parse_emphasis_markers(
    line: &str,
    line_index: usize,
    inlines: &mut Vec<MarkdownInline>,
    covered_ranges: &[(usize, usize)],
) {
    for (column, byte) in line.bytes().enumerate() {
        if !matches!(byte, b'*' | b'_') || is_byte_covered(column, covered_ranges) {
            continue;
        }
        inlines.push(MarkdownInline {
            kind: MarkdownInlineKind::EmphasisMarker {
                marker: (byte as char).to_string(),
            },
            range: MarkdownTextRange::single_line(line_index, column, column + 1),
        });
    }
}

fn is_byte_covered(column: usize, covered_ranges: &[(usize, usize)]) -> bool {
    covered_ranges
        .iter()
        .any(|(start, end)| (*start..*end).contains(&column))
}

#[cfg(test)]
mod tests {
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
}
