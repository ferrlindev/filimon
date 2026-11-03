// ─────────────────────────────────────────────────────────────────────────────
// commands/consume.rs - Consume mode
//
// Reads news `SourceItem`s from `filimon-raw-pages` with the three-SmartModule
// chain attached (map-preprocess → map-extract → filter-relevance), maps each
// extracted article to a unified output record, applies optional NER and
// writes the records as one JSON array.
// ─────────────────────────────────────────────────────────────────────────────

use std::path::Path;
use std::time::Duration;

use fluvio::Offset;
use fluvio::consumer::Record;
use fluvio::dataplane::link::ErrorCode;
use futures_util::{Stream, StreamExt, future};

use crate::models::{ExtractedArticle, OutputRecord};
use crate::output;
use crate::scorer::NerEngine;
use crate::streaming::{FluvioStream, NEWS_CHAIN, TOPIC_RAW_PAGES, news_chain};

/// Where batch mode writes its JSON output.
pub const DEFAULT_OUTPUT: &str = "output/wise_output.json";

/// Batch mode stops when no record arrives for this long.
const IDLE_TIMEOUT: Duration = Duration::from_secs(5);

/// Longest error detail a diagnostic carries, in characters.
const MAX_DETAIL: usize = 200;

/// The topics `fili consume` reads.
pub struct ConsumeTopics<'a> {
    pub raw_pages: &'a str,
}

impl Default for ConsumeTopics<'_> {
    fn default() -> Self {
        Self {
            raw_pages: TOPIC_RAW_PAGES,
        }
    }
}

/// One item from a consumed stream: a record, or a diagnostic for stderr.
pub enum Incoming {
    Record(OutputRecord),
    Diagnostic(String),
}

/// What a batch run collected.
#[derive(Default)]
pub struct Collected {
    pub records: Vec<OutputRecord>,
    pub diagnostics: Vec<String>,
}

/// Keep at most `MAX_DETAIL` characters of an error message.
fn truncate_detail(s: &str) -> String {
    s.chars().take(MAX_DETAIL).collect()
}

/// Replace every double-quoted span of a serde error with `"…"`.
///
/// `serde_json` quotes the offending string value in `invalid type` errors, and
/// that value can be page content. Field names use backticks and stay.
fn redact_quoted(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut in_quotes = false;
    for c in s.chars() {
        if c == '"' {
            if in_quotes {
                out.push_str("…\"");
            } else {
                out.push('"');
            }
            in_quotes = !in_quotes;
        } else if !in_quotes {
            out.push(c);
        }
    }
    out
}

/// Decode one value from the news chain.
///
/// A value that is not an `ExtractedArticle` becomes a diagnostic naming the
/// topic and offset. The diagnostic never includes the value itself.
pub fn decode_news(topic: &str, offset: i64, value: &[u8]) -> Incoming {
    match serde_json::from_slice::<ExtractedArticle>(value) {
        Ok(article) => Incoming::Record(OutputRecord::from_article(article)),
        Err(e) => Incoming::Diagnostic(format!(
            "{topic} offset {offset}: not an extracted article: {}",
            truncate_detail(&redact_quoted(&e.to_string()))
        )),
    }
}

/// Describe a stream error without the record value.
///
/// A chain runtime error carries the whole failing record (page HTML),
/// so only its offset and the start of its hint are reported.
pub fn describe_stream_error(topic: &str, code: &ErrorCode) -> String {
    match code {
        ErrorCode::SmartModuleRuntimeError(e) => format!(
            "{topic} offset {}: SmartModule error: {}",
            e.offset,
            truncate_detail(&e.hint)
        ),
        other => format!("{topic}: stream error: {}", truncate_detail(&other.to_string())),
    }
}

/// Turn a raw news stream into `Incoming` items.
///
/// The first stream error becomes one diagnostic and then the stream ends,
/// because a retry-disabled consumer cannot move past it.
fn news_incoming<S>(stream: S, topic: &str) -> impl Stream<Item = Incoming> + Unpin + use<S>
where
    S: Stream<Item = Result<Record, ErrorCode>> + Unpin,
{
    let topic = topic.to_string();
    stream.scan(false, move |failed, item| {
        if *failed {
            return future::ready(None);
        }
        let incoming = match item {
            Ok(record) => decode_news(&topic, record.offset(), record.value()),
            Err(code) => {
                *failed = true;
                Incoming::Diagnostic(describe_stream_error(&topic, &code))
            }
        };
        future::ready(Some(incoming))
    })
}

/// Collect up to `limit` records, in arrival order.
///
/// Stops when the stream ends or nothing arrives within `idle`. Diagnostics
/// are kept separately and do not count toward `limit`.
pub async fn collect_records<S: Stream<Item = Incoming> + Unpin>(
    mut stream: S,
    limit: usize,
    idle: Duration,
) -> Collected {
    let mut collected = Collected::default();
    while collected.records.len() < limit {
        match tokio::time::timeout(idle, stream.next()).await {
            Ok(Some(Incoming::Record(record))) => collected.records.push(record),
            Ok(Some(Incoming::Diagnostic(message))) => collected.diagnostics.push(message),
            Ok(None) | Err(_) => break,
        }
    }
    collected
}

/// Check the topics and chain, then collect up to `limit` news records.
pub async fn collect_from_topics(
    stream: &FluvioStream,
    topics: &ConsumeTopics<'_>,
    offset: Offset,
    limit: usize,
) -> miette::Result<Collected> {
    stream.preflight(&[topics.raw_pages], &NEWS_CHAIN).await?;
    let news = stream
        .topic_stream(topics.raw_pages, offset, news_chain())
        .await?;
    Ok(collect_records(news_incoming(news, topics.raw_pages), limit, IDLE_TIMEOUT).await)
}

/// Consume news records from Fluvio.
///
/// # Arguments
/// * `from_beginning` - Start consuming from the beginning of the topic
/// * `limit` - Maximum records to consume (0 = unlimited streaming)
/// * `ner` - Optional NER engine for entity enrichment
pub async fn run(
    from_beginning: bool,
    limit: usize,
    ner: Option<&NerEngine>,
) -> miette::Result<()> {
    println!("[Consume] Connecting to Fluvio cluster...");
    let stream = FluvioStream::connect().await?;

    let offset = if from_beginning {
        Offset::beginning()
    } else {
        Offset::end()
    };

    let topics = ConsumeTopics::default();
    println!(
        "[Consume] Reading {} through {}",
        topics.raw_pages,
        NEWS_CHAIN.join(" → ")
    );

    if limit > 0 {
        consume_batch(&stream, &topics, offset, limit, ner).await
    } else {
        consume_stream(&stream, &topics, offset).await
    }
}

/// Consume a batch of records, enrich them, print them and write the JSON file.
async fn consume_batch(
    stream: &FluvioStream,
    topics: &ConsumeTopics<'_>,
    offset: Offset,
    limit: usize,
    ner: Option<&NerEngine>,
) -> miette::Result<()> {
    println!("[Consume] Will consume up to {limit} records.\n");
    let Collected {
        mut records,
        diagnostics,
    } = collect_from_topics(stream, topics, offset, limit).await?;

    for message in &diagnostics {
        eprintln!("  ✗ {message}");
    }

    for record in &mut records {
        if let Some(engine) = ner {
            if let Ok(entities) = engine.detect(&record.text) {
                record.entities = entities;
            }
        }
        output::print_summary(record);
    }

    output::save_json(&records, Path::new(DEFAULT_OUTPUT))
        .map_err(|e| miette::miette!("Failed to write {DEFAULT_OUTPUT}: {e}"))?;
    output::print_run_summary(records.len(), DEFAULT_OUTPUT);
    Ok(())
}

/// Consume records in streaming mode (unlimited). Print only: no NER, no file.
async fn consume_stream(
    stream: &FluvioStream,
    topics: &ConsumeTopics<'_>,
    offset: Offset,
) -> miette::Result<()> {
    stream.preflight(&[topics.raw_pages], &NEWS_CHAIN).await?;
    let news = stream
        .topic_stream(topics.raw_pages, offset, news_chain())
        .await?;
    println!("[Consume] Press Ctrl+C to stop.\n");

    let mut incoming = news_incoming(news, topics.raw_pages);
    while let Some(item) = incoming.next().await {
        match item {
            Incoming::Record(record) => output::print_summary(&record),
            Incoming::Diagnostic(message) => eprintln!("  ✗ {message}"),
        }
    }
    Ok(())
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::SourceType;
    use fluvio::dataplane::link::smartmodule::SmartModuleTransformRuntimeError;

    fn article(url: &str) -> ExtractedArticle {
        ExtractedArticle {
            url: url.to_string(),
            title: "Title".to_string(),
            author: None,
            published_date: None,
            body_preview: "body".to_string(),
            word_count: 1,
            relevance_score: 0.5,
            top_keywords: vec![],
            named_entities: vec![],
            inferred_category: "General".to_string(),
        }
    }

    fn record(url: &str) -> Incoming {
        Incoming::Record(OutputRecord::from_article(article(url)))
    }

    #[test]
    fn decode_news_reports_bad_value_without_content() {
        let value = br#"{"secret":"<html>PAGE-BODY</html>"}"#;
        let Incoming::Diagnostic(message) = decode_news("filimon-raw-pages", 42, value) else {
            panic!("expected a diagnostic");
        };
        assert!(message.contains("filimon-raw-pages"), "{message}");
        assert!(message.contains("42"), "{message}");
        assert!(!message.contains("PAGE-BODY"), "{message}");

        // An invalid-type error quotes the value; it must still not leak.
        let Incoming::Diagnostic(message) = decode_news("t", 1, br#""<html>PAGE-BODY</html>""#)
        else {
            panic!("expected a diagnostic");
        };
        assert!(!message.contains("PAGE-BODY"), "{message}");
    }

    #[test]
    fn stream_error_omits_record_value() {
        let code = ErrorCode::SmartModuleRuntimeError(Box::new(SmartModuleTransformRuntimeError {
            hint: "bad".into(),
            offset: 7,
            record_value: "PAGE-BODY".into(),
            ..Default::default()
        }));
        let message = describe_stream_error("filimon-raw-pages", &code);
        assert!(message.contains('7'), "{message}");
        assert!(message.contains("bad"), "{message}");
        assert!(!message.contains("PAGE-BODY"), "{message}");
    }

    #[tokio::test]
    async fn collect_stops_at_limit() {
        let stream = futures_util::stream::iter(vec![record("a"), record("b"), record("c")]);
        let collected = collect_records(stream, 2, Duration::from_secs(1)).await;
        assert_eq!(collected.records.len(), 2);
    }

    #[tokio::test]
    async fn collect_keeps_arrival_order_and_duplicates() {
        let stream = futures_util::stream::iter(vec![record("a"), record("a"), record("b")]);
        let collected = collect_records(stream, 10, Duration::from_secs(1)).await;
        let ids: Vec<&str> = collected.records.iter().map(|r| r.id.as_str()).collect();
        assert_eq!(ids, ["a", "a", "b"]);
    }

    #[tokio::test]
    async fn collect_empty_stream_returns_no_records() {
        let stream = futures_util::stream::iter(Vec::<Incoming>::new());
        let collected = collect_records(stream, 10, Duration::from_secs(1)).await;
        assert!(collected.records.is_empty());
        assert!(collected.diagnostics.is_empty());
    }

    /// The README `Tech Market Surges` sample, which passes the full chain.
    const NEWS_HTML: &str = "<html><head><title>Breaking News</title></head><body><h1>Tech Market Surges</h1><p>The technology market experienced significant growth today as investors rallied around artificial intelligence stocks.</p></body></html>";

    #[tokio::test]
    #[ignore = "needs a local Fluvio cluster with the filimon SmartModules loaded"]
    async fn news_through_chain() -> miette::Result<()> {
        use crate::models::SourceItem;
        use fluvio::metadata::topic::TopicSpec;

        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock after epoch")
            .as_nanos();
        let topic = format!("filimon-test-raw-{nanos}");

        let admin = fluvio::Fluvio::connect()
            .await
            .map_err(|e| miette::miette!("connect: {e}"))?
            .admin()
            .await;
        admin
            .create(topic.clone(), false, TopicSpec::new_computed(1, 1, None))
            .await
            .map_err(|e| miette::miette!("create {topic}: {e}"))?;

        let result = async {
            let stream = FluvioStream::connect().await?;
            let url = "https://example.com/news";
            let item = SourceItem::news(url.to_string(), NEWS_HTML.to_string());

            // A new topic can take a moment to provision; retry the producer.
            let mut producer = None;
            for _ in 0..20 {
                if let Ok(p) = stream.producer(&topic).await {
                    producer = Some(p);
                    break;
                }
                tokio::time::sleep(Duration::from_millis(250)).await;
            }
            let producer = producer.ok_or_else(|| miette::miette!("no producer for {topic}"))?;
            producer.send(url, &item).await?;
            producer.flush().await?;

            let topics = ConsumeTopics { raw_pages: &topic };
            let collected = collect_from_topics(&stream, &topics, Offset::beginning(), 1).await?;
            assert!(collected.diagnostics.is_empty(), "{:?}", collected.diagnostics);
            assert_eq!(collected.records.len(), 1);

            let record = &collected.records[0];
            assert_eq!(record.source_type, SourceType::News);
            assert_eq!(record.platform, "example.com");
            assert_eq!(record.id, record.url);
            assert_eq!(record.url, url);
            let score = record.article.as_ref().expect("article block").relevance_score;
            assert!(score >= 0.05, "relevance {score}");

            let value = serde_json::to_value(record).map_err(|e| miette::miette!("{e}"))?;
            assert!(value.as_object().expect("object").get("html").is_none());
            Ok::<(), miette::Report>(())
        }
        .await;

        let deleted = admin.delete::<TopicSpec>(topic.clone()).await;
        result?;
        deleted.map_err(|e| miette::miette!("delete {topic}: {e}"))
    }
}
