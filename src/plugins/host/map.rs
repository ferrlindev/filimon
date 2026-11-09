// ─────────────────────────────────────────────────────────────────────────────
// plugins/host/map.rs - WIT item to SourceItem
//
// The WIT `item` record maps 1:1 onto `SourceItem::social`. The Fluvio message
// key is `"{platform}:{id}"` (assumes a single-partition topic). Plugin output
// is untrusted and the consumer does no id or url validation, so the host is
// the only gate: a bad item is dropped with a content-free warning and never
// fails the poll.
// ─────────────────────────────────────────────────────────────────────────────

use tracing::warn;

use super::engine::WitItem;
use super::sink::SinkRecord;
use crate::models::SourceItem;

/// Largest record the host sends, in bytes: the serialized value plus the
/// message key. Fluvio sizes the whole record, key included, so both count.
/// This stays under the Fluvio default `max_request_size` (about 1 MiB) to leave
/// headroom for framing.
pub const MAX_RECORD_BYTES: usize = 1_000_000;

/// Most items the host takes from one poll. Items past the cap are dropped (and
/// counted) so one poll cannot make the host serialize and send an unbounded
/// list. The cursor still advances, so dropped items are not retried.
pub const MAX_ITEMS_PER_POLL: usize = 1000;

/// Longest message key (`"{platform}:{id}"`) the host accepts, in bytes. Both
/// parts are guest-controlled, so the key is bounded on its own as well as
/// counted in `MAX_RECORD_BYTES`.
pub const MAX_KEY_BYTES: usize = 512;

/// Why an item was dropped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DropReason {
    EmptyId,
    EmptyUrl,
    InvalidUrl,
    KeyTooLong,
    RecordTooLarge,
    Unserializable,
    TooManyItems,
}

impl DropReason {
    /// A fixed label, safe to log: it never carries item content.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::EmptyId => "empty-id",
            Self::EmptyUrl => "empty-url",
            Self::InvalidUrl => "invalid-url",
            Self::KeyTooLong => "key-too-long",
            Self::RecordTooLarge => "record-too-large",
            Self::Unserializable => "unserializable",
            Self::TooManyItems => "too-many-items",
        }
    }
}

/// The mapped records of one poll.
#[derive(Debug)]
pub struct Mapped {
    pub records: Vec<SinkRecord>,
    /// Items rejected during mapping.
    pub dropped: usize,
}

/// Validates and maps one item, or says why it must be dropped.
fn map_item(plugin: &str, item: WitItem) -> Result<SinkRecord, DropReason> {
    if item.id.trim().is_empty() {
        return Err(DropReason::EmptyId);
    }
    if item.url.trim().is_empty() {
        return Err(DropReason::EmptyUrl);
    }
    if url::Url::parse(&item.url).is_err() {
        return Err(DropReason::InvalidUrl);
    }

    let platform = if item.platform.trim().is_empty() {
        plugin.to_string()
    } else {
        item.platform
    };
    // Bound the key before building it: a guest can make `platform` and `id`
    // nearly as large as the whole record.
    if platform.len() + 1 + item.id.len() > MAX_KEY_BYTES {
        return Err(DropReason::KeyTooLong);
    }
    let key = format!("{platform}:{}", item.id);
    let source_item = SourceItem::social(
        item.id,
        item.url,
        item.title,
        item.text,
        item.author,
        item.published,
        platform,
        item.raw,
    );

    match serde_json::to_vec(&source_item) {
        Err(_) => Err(DropReason::Unserializable),
        Ok(bytes) if bytes.len() + key.len() > MAX_RECORD_BYTES => {
            Err(DropReason::RecordTooLarge)
        }
        Ok(_) => Ok(SinkRecord {
            key,
            item: source_item,
        }),
    }
}

/// Maps one poll's items to sink records, keeping the plugin's list order.
///
/// Invalid items are dropped and counted; equal keys are all kept (no dedupe
/// in this phase). At most `MAX_ITEMS_PER_POLL` items are considered.
#[must_use]
pub fn map_items(plugin: &str, mut items: Vec<WitItem>) -> Mapped {
    // Over the per-poll cap: keep the first `MAX_ITEMS_PER_POLL` in list order
    // and drop the rest with one warning (a count, never content).
    let excess = items.len().saturating_sub(MAX_ITEMS_PER_POLL);
    if excess > 0 {
        items.truncate(MAX_ITEMS_PER_POLL);
        warn!(
            plugin = %plugin,
            dropped = excess,
            reason = DropReason::TooManyItems.label(),
            "dropped plugin items over the per-poll cap"
        );
    }
    let mut records = Vec::with_capacity(items.len());
    let mut dropped = excess;
    for (index, item) in items.into_iter().enumerate() {
        match map_item(plugin, item) {
            Ok(record) => records.push(record),
            Err(reason) => {
                dropped += 1;
                warn!(
                    plugin = %plugin,
                    index = index,
                    reason = reason.label(),
                    "dropped plugin item"
                );
            }
        }
    }
    Mapped { records, dropped }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::SourceType;
    use crate::plugins::host::testing::LogCapture;

    fn wit(id: &str, url: &str) -> WitItem {
        WitItem {
            id: id.to_string(),
            url: url.to_string(),
            title: None,
            text: String::new(),
            author: None,
            published: None,
            platform: "p".to_string(),
            raw: None,
        }
    }

    fn good(id: &str) -> WitItem {
        wit(id, "https://example.invalid/x")
    }

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

    #[test]
    fn epoch_ms_becomes_rfc3339_and_out_of_range_becomes_none() {
        let mut a = good("a");
        a.published = Some(1_730_000_000_123);
        let mut b = good("b");
        b.published = Some(u64::MAX);

        let mapped = map_items("p", vec![a, b]);
        assert_eq!(mapped.dropped, 0);
        assert_eq!(mapped.records.len(), 2);
        assert_eq!(
            mapped.records[0].item.published.as_deref(),
            Some("2024-10-27T03:33:20.123Z")
        );
        assert_eq!(mapped.records[1].item.published, None);
    }

    #[test]
    fn raw_json_is_parsed_and_non_json_raw_becomes_a_string() {
        let mut a = good("a");
        a.raw = Some("{\"a\":1}".to_string());
        let mut b = good("b");
        b.raw = Some("not json".to_string());

        let mapped = map_items("p", vec![a, b]);
        assert_eq!(
            mapped.records[0].item.raw,
            Some(serde_json::json!({"a": 1}))
        );
        assert_eq!(
            mapped.records[1].item.raw,
            Some(serde_json::Value::String("not json".to_string()))
        );
    }

    #[test]
    fn key_is_platform_colon_id_after_fallback() {
        let mut item = good("t3_x");
        item.platform = "reddit".to_string();
        let mapped = map_items("acme", vec![item]);
        assert_eq!(mapped.records[0].key, "reddit:t3_x");
        assert_eq!(mapped.records[0].item.platform, "reddit");
    }

    #[test]
    fn empty_platform_falls_back_to_plugin_name() {
        let mut empty = good("1");
        empty.platform = String::new();
        let mut blank = good("2");
        blank.platform = "   ".to_string();

        let mapped = map_items("acme", vec![empty, blank]);
        assert_eq!(mapped.dropped, 0);
        assert_eq!(mapped.records[0].key, "acme:1");
        assert_eq!(mapped.records[0].item.platform, "acme");
        assert_eq!(mapped.records[1].key, "acme:2");
        assert_eq!(mapped.records[1].item.platform, "acme");
    }

    #[test]
    fn empty_or_whitespace_id_is_dropped() {
        let mapped = map_items(
            "p",
            vec![
                good("a"),
                wit("", "https://e.invalid/"),
                wit("  \t", "https://e.invalid/"),
                good("b"),
            ],
        );
        assert_eq!(mapped.dropped, 2);
        let ids: Vec<_> = mapped.records.iter().map(|r| r.item.id.as_str()).collect();
        assert_eq!(ids, ["a", "b"]);
    }

    #[test]
    fn empty_or_whitespace_url_is_dropped() {
        let mapped = map_items(
            "p",
            vec![good("a"), wit("x", ""), wit("y", "  "), good("b")],
        );
        assert_eq!(mapped.dropped, 2);
        let ids: Vec<_> = mapped.records.iter().map(|r| r.item.id.as_str()).collect();
        assert_eq!(ids, ["a", "b"]);
    }

    #[test]
    fn unparsable_url_is_dropped() {
        let mapped = map_items(
            "p",
            vec![
                good("a"),
                wit("x", "not a url"),
                wit("y", "/relative/path"),
                good("b"),
            ],
        );
        assert_eq!(mapped.dropped, 2);
        let ids: Vec<_> = mapped.records.iter().map(|r| r.item.id.as_str()).collect();
        assert_eq!(ids, ["a", "b"]);
    }

    /// Pads `text` so the record (value plus key) is exactly `total` bytes.
    fn padded(id: &str, total: usize) -> WitItem {
        let mut item = good(id);
        let baseline = record_len(&item);
        item.text = "a".repeat(total - baseline);
        item
    }

    fn serialized_len(item: &WitItem) -> usize {
        let source = SourceItem::social(
            item.id.clone(),
            item.url.clone(),
            item.title.clone(),
            item.text.clone(),
            item.author.clone(),
            item.published,
            item.platform.clone(),
            item.raw.clone(),
        );
        serde_json::to_vec(&source).expect("serializes").len()
    }

    /// The size the host compares to the cap: value plus `"{platform}:{id}"`.
    fn record_len(item: &WitItem) -> usize {
        serialized_len(item) + item.platform.len() + 1 + item.id.len()
    }

    #[test]
    fn record_exactly_at_the_cap_is_kept() {
        let at_cap = padded("a", MAX_RECORD_BYTES);
        assert_eq!(record_len(&at_cap), MAX_RECORD_BYTES);
        let over = padded("b", MAX_RECORD_BYTES + 1);
        assert_eq!(record_len(&over), MAX_RECORD_BYTES + 1);

        let mapped = map_items("p", vec![at_cap, over]);
        assert_eq!(mapped.dropped, 1);
        assert_eq!(mapped.records.len(), 1);
        assert_eq!(mapped.records[0].item.id, "a");
    }

    #[test]
    fn the_key_counts_toward_the_record_cap() {
        // A value that alone is at the cap is over it once the key is added.
        let mut value_at_cap = good("a");
        let baseline = serialized_len(&value_at_cap);
        value_at_cap.text = "a".repeat(MAX_RECORD_BYTES - baseline);
        assert_eq!(serialized_len(&value_at_cap), MAX_RECORD_BYTES);

        // One byte less than the value cap minus the key (3 bytes: "p:a") fits.
        let mut fits = good("a");
        fits.text = "a".repeat(MAX_RECORD_BYTES - baseline - 3);
        assert_eq!(record_len(&fits), MAX_RECORD_BYTES);

        let mapped = map_items("p", vec![value_at_cap, fits]);
        assert_eq!(mapped.dropped, 1);
        assert_eq!(mapped.records.len(), 1);
    }

    #[test]
    fn a_huge_id_is_dropped_by_the_key_bound_and_others_kept() {
        let big_id = "i".repeat(900_000);
        let mapped = map_items("p", vec![good("a"), good(&big_id), good("c")]);
        assert_eq!(mapped.dropped, 1);
        let ids: Vec<_> = mapped.records.iter().map(|r| r.item.id.as_str()).collect();
        assert_eq!(ids, ["a", "c"]);
    }

    #[test]
    fn a_huge_platform_is_dropped_by_the_key_bound() {
        let mut item = good("a");
        item.platform = "x".repeat(MAX_KEY_BYTES);
        let mapped = map_items("p", vec![item]);
        assert_eq!(mapped.dropped, 1);
        assert!(mapped.records.is_empty());
    }

    #[test]
    fn key_exactly_at_the_bound_is_kept_and_one_over_is_dropped() {
        // platform "p" + ":" + id = MAX_KEY_BYTES.
        let at_bound = good(&"i".repeat(MAX_KEY_BYTES - 2));
        let over = good(&"j".repeat(MAX_KEY_BYTES - 1));

        let mapped = map_items("p", vec![at_bound, over]);
        assert_eq!(mapped.dropped, 1);
        assert_eq!(mapped.records.len(), 1);
        assert_eq!(mapped.records[0].key.len(), MAX_KEY_BYTES);
    }

    #[test]
    fn the_key_bound_counts_bytes_not_chars() {
        // Two-byte chars: 255 of them (510 bytes) + "p:" = 512 fits; 256 does not.
        let fits = good(&"é".repeat(255));
        let over = good(&"é".repeat(256));
        let mapped = map_items("p", vec![fits, over]);
        assert_eq!(mapped.dropped, 1);
        assert_eq!(mapped.records.len(), 1);
    }

    #[test]
    fn key_too_long_warning_has_a_fixed_label_and_no_content() {
        let (capture, _guard) = LogCapture::start();
        let secret_id = format!("SECRET-ID-789-{}", "z".repeat(MAX_KEY_BYTES));
        let mapped = map_items("acme", vec![good(&secret_id)]);
        assert_eq!(mapped.dropped, 1);

        let log = capture.text();
        assert!(
            log.contains("reason=\"key-too-long\"") || log.contains("reason=key-too-long"),
            "{log}"
        );
        assert!(!log.contains("SECRET-ID-789"), "{log}");
    }

    #[test]
    fn oversized_record_is_dropped_and_others_kept() {
        let mut huge = good("big");
        huge.text = "a".repeat(MAX_RECORD_BYTES + 1);

        let mapped = map_items("p", vec![good("a"), huge, good("c")]);
        assert_eq!(mapped.dropped, 1);
        let ids: Vec<_> = mapped.records.iter().map(|r| r.item.id.as_str()).collect();
        assert_eq!(ids, ["a", "c"]);
    }

    #[test]
    fn order_is_preserved_and_equal_keys_are_both_kept() {
        let mut again = good("a");
        again.text = "second".to_string();

        let mapped = map_items("p", vec![good("a"), good("b"), again]);
        assert_eq!(mapped.dropped, 0);
        let keys: Vec<_> = mapped.records.iter().map(|r| r.key.as_str()).collect();
        assert_eq!(keys, ["p:a", "p:b", "p:a"]);
        assert_eq!(mapped.records[2].item.text, "second");
    }

    #[test]
    fn exactly_the_per_poll_cap_is_kept() {
        let items: Vec<_> = (0..MAX_ITEMS_PER_POLL).map(|n| good(&n.to_string())).collect();
        let mapped = map_items("p", items);
        assert_eq!(mapped.dropped, 0);
        assert_eq!(mapped.records.len(), MAX_ITEMS_PER_POLL);
    }

    #[test]
    fn items_over_the_per_poll_cap_are_dropped_in_order_with_one_content_free_warning() {
        let (capture, _guard) = LogCapture::start();
        let items: Vec<_> = (0..MAX_ITEMS_PER_POLL + 5)
            .map(|n| good(&format!("SECRET-{n}")))
            .collect();
        let mapped = map_items("acme", items);

        assert_eq!(mapped.dropped, 5);
        assert_eq!(mapped.records.len(), MAX_ITEMS_PER_POLL);
        assert_eq!(mapped.records[0].item.id, "SECRET-0");
        assert_eq!(
            mapped.records[MAX_ITEMS_PER_POLL - 1].item.id,
            format!("SECRET-{}", MAX_ITEMS_PER_POLL - 1)
        );

        let log = capture.text();
        assert!(log.contains("plugin=acme"), "{log}");
        assert!(log.contains("dropped=5"), "{log}");
        assert!(
            log.contains("reason=\"too-many-items\"") || log.contains("reason=too-many-items"),
            "{log}"
        );
        assert_eq!(log.matches("dropped plugin items").count(), 1, "{log}");
        assert!(!log.contains("SECRET-"), "{log}");
    }

    #[test]
    fn empty_list_maps_to_nothing() {
        let mapped = map_items("p", Vec::new());
        assert!(mapped.records.is_empty());
        assert_eq!(mapped.dropped, 0);
    }

    #[test]
    fn optional_fields_none_are_accepted() {
        let mapped = map_items("p", vec![good("a")]);
        assert_eq!(mapped.dropped, 0);
        let item = &mapped.records[0].item;
        assert!(item.title.is_none());
        assert!(item.author.is_none());
        assert!(item.raw.is_none());
        assert!(item.published.is_none());
    }

    #[test]
    fn valid_ids_are_not_trimmed_or_altered() {
        // A url with surrounding spaces still parses (the url crate strips them
        // when parsing), and the stored value stays byte-exact.
        let spaced = map_items("p", vec![wit(" a b ", " https://e.invalid/x ")]);
        assert_eq!(spaced.dropped, 0);
        assert_eq!(spaced.records[0].item.id, " a b ");
        assert_eq!(spaced.records[0].item.url, " https://e.invalid/x ");
        assert_eq!(spaced.records[0].key, "p: a b ");

        let exact = map_items("p", vec![wit(" a b ", "https://e.invalid/x")]);
        assert_eq!(exact.records[0].item.id, " a b ");
        assert_eq!(exact.records[0].key, "p: a b ");
    }

    #[test]
    fn drop_warning_names_plugin_index_and_reason_but_not_content() {
        let (capture, _guard) = LogCapture::start();
        let mut bad = wit("   ", "https://secret.invalid/SECRET-URL-456");
        bad.text = "SECRET-TEXT-123".to_string();
        bad.title = Some("SECRET-TITLE".to_string());
        bad.author = Some("SECRET-AUTHOR".to_string());
        bad.raw = Some("SECRET-RAW".to_string());

        let mapped = map_items("acme", vec![good("a"), bad]);
        assert_eq!(mapped.dropped, 1);

        let log = capture.text();
        assert!(log.contains("plugin=acme"), "{log}");
        assert!(log.contains("index=1"), "{log}");
        assert!(
            log.contains("reason=\"empty-id\"") || log.contains("reason=empty-id"),
            "{log}"
        );
        assert!(log.contains("fili::plugins::host::map"), "{log}");
        for secret in [
            "SECRET-TEXT-123",
            "SECRET-URL-456",
            "SECRET-TITLE",
            "SECRET-AUTHOR",
            "SECRET-RAW",
        ] {
            assert!(!log.contains(secret), "{log}");
        }
    }

    #[test]
    fn drop_reason_labels_are_fixed() {
        assert_eq!(DropReason::EmptyId.label(), "empty-id");
        assert_eq!(DropReason::EmptyUrl.label(), "empty-url");
        assert_eq!(DropReason::InvalidUrl.label(), "invalid-url");
        assert_eq!(DropReason::KeyTooLong.label(), "key-too-long");
        assert_eq!(DropReason::RecordTooLarge.label(), "record-too-large");
        assert_eq!(DropReason::Unserializable.label(), "unserializable");
        assert_eq!(DropReason::TooManyItems.label(), "too-many-items");
    }
}
