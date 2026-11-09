// ─────────────────────────────────────────────────────────────────────────────
// plugins/host/map.rs - WIT item to SourceItem
//
// The WIT `item` record maps 1:1 onto `SourceItem::social`. The Fluvio message
// key is `"{platform}:{id}"` (assumes a single-partition topic). Item
// validation is a later plan; here nothing is dropped.
// ─────────────────────────────────────────────────────────────────────────────

use super::engine::WitItem;
use super::sink::SinkRecord;
use crate::models::SourceItem;

/// The mapped records of one poll.
#[derive(Debug)]
pub struct Mapped {
    pub records: Vec<SinkRecord>,
    /// Items rejected during mapping.
    pub dropped: usize,
}

/// Maps one poll's items to sink records, keeping the plugin's list order.
#[must_use]
pub fn map_items(_plugin: &str, items: Vec<WitItem>) -> Mapped {
    let records = items
        .into_iter()
        .map(|item| {
            let key = format!("{}:{}", item.platform, item.id);
            SinkRecord {
                key,
                item: SourceItem::social(
                    item.id,
                    item.url,
                    item.title,
                    item.text,
                    item.author,
                    item.published,
                    item.platform,
                    item.raw,
                ),
            }
        })
        .collect();
    Mapped {
        records,
        dropped: 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::SourceType;

    #[test]
    fn maps_item_fields_and_key() {
        let item = WitItem {
            id: "stub-0".to_string(),
            url: "https://example.invalid/stub/0".to_string(),
            title: Some("stub item".to_string()),
            text: "hello".to_string(),
            author: Some("stub".to_string()),
            published: Some(1_767_225_600_000),
            platform: "stub".to_string(),
            raw: Some("{\"n\":0}".to_string()),
        };

        let mapped = map_items("stub", vec![item]);
        assert_eq!(mapped.dropped, 0);
        assert_eq!(mapped.records.len(), 1);

        let record = &mapped.records[0];
        assert_eq!(record.key, "stub:stub-0");
        let item = &record.item;
        assert_eq!(item.source_type, SourceType::Social);
        assert_eq!(item.platform, "stub");
        assert_eq!(item.id, "stub-0");
        assert_eq!(item.url, "https://example.invalid/stub/0");
        assert_eq!(item.title.as_deref(), Some("stub item"));
        assert_eq!(item.text, "hello");
        assert_eq!(item.author.as_deref(), Some("stub"));
        assert_eq!(item.published.as_deref(), Some("2026-01-01T00:00:00.000Z"));
        assert_eq!(item.raw, Some(serde_json::json!({"n": 0})));
        assert!(item.html.is_none());
    }
}
