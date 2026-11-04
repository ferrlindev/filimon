// ─────────────────────────────────────────────────────────────────────────────
// commands/consume.rs - Consume mode
//
// Reads two streams and merges them into one:
//   - news `SourceItem`s from `filimon-raw-pages`, with the three-SmartModule
//     chain attached (map-preprocess → map-extract → filter-relevance)
//   - social `SourceItem`s from `filimon-social-items`, with no SmartModules
// Each item becomes a unified output record, labelled by the topic it came
// from. Batch mode applies optional NER and writes one JSON array.
// ─────────────────────────────────────────────────────────────────────────────

use std::path::Path;
use std::time::Duration;

use fluvio::Offset;
use fluvio::consumer::Record;
use fluvio::dataplane::link::ErrorCode;
use futures_util::{Stream, StreamExt, future};

use crate::models::{
    ExtractedArticle, NamedEntity, OutputRecord, SourceItem, SourceType, rfc3339_to_utc,
};
use crate::cli::SourceFilter;
use crate::output;
use crate::scorer::NerEngine;
use crate::streaming::{
    FluvioStream, NEWS_CHAIN, TOPIC_RAW_PAGES, TOPIC_SOCIAL_ITEMS, news_chain,
};

/// Batch mode stops when no record arrives for this long.
const IDLE_TIMEOUT: Duration = Duration::from_secs(5);

/// Longest error detail a diagnostic carries, in characters.
const MAX_DETAIL: usize = 200;

/// The topics `fili consume` reads.
pub struct ConsumeTopics<'a> {
    pub raw_pages: &'a str,
    pub social: &'a str,
}

impl Default for ConsumeTopics<'_> {
    fn default() -> Self {
        Self {
            raw_pages: TOPIC_RAW_PAGES,
            social: TOPIC_SOCIAL_ITEMS,
        }
    }
}

/// One item from a consumed stream: a record, a diagnostic for stderr, or
/// the diagnostic of a stream error that ended its branch.
pub enum Incoming {
    Record(OutputRecord),
    Diagnostic(String),
    Fatal(String),
}

/// What a batch run collected.
#[derive(Default)]
pub struct Collected {
    pub records: Vec<OutputRecord>,
    /// Every diagnostic, fatal ones included, in arrival order.
    pub diagnostics: Vec<String>,
    /// Whether a branch ended on a stream error.
    pub failed: bool,
}

/// Keep at most `MAX_DETAIL` characters of an error message.
fn truncate_detail(s: &str) -> String {
    s.chars().take(MAX_DETAIL).collect()
}

/// Describe a JSON decode error by its class and position only.
///
/// The result holds a fixed phrase and two numbers. serde's Display text
/// quotes string values (escaped, inside double quotes) and prints variant
/// names, unknown field names, numbers and characters inside backticks
/// without escaping, so no redaction of that text can find where record
/// content ends. This function never reads it.
fn describe_json_error(e: &serde_json::Error) -> String {
    use serde_json::error::Category;
    let class = match e.classify() {
        Category::Syntax => "invalid JSON",
        Category::Eof => "truncated JSON",
        Category::Data => "wrong JSON shape",
        Category::Io => "read error",
    };
    format!("{class} at line {} column {}", e.line(), e.column())
}

/// What a stream diagnostic says when no module-authored text is left.
const NO_DETAIL: &str = "no detail";

/// Keep only the module-authored start of a `SmartModule` hint.
///
/// fluvio builds a `SmartModule` hint from the module's eyre report. The
/// filimon modules write a fixed message prefix, such as
/// `Failed to parse RawPage: `, followed by `serde_json`'s error text, and
/// eyre adds a location block after a newline. serde puts every
/// record-controlled piece of its text after a double quote (string values)
/// or a backtick (variant names, field names, numbers), and a `look_back`
/// error's record value follows a newline. So the
/// text before the first double quote, backtick or control character holds
/// only module-authored text. It is cut to `MAX_DETAIL` characters, and
/// `NO_DETAIL` stands in when nothing is left.
fn hint_summary(text: &str) -> String {
    let end = text
        .find(|c: char| c == '"' || c == '`' || c.is_control())
        .unwrap_or(text.len());
    let kept = truncate_detail(text[..end].trim_end());
    if kept.is_empty() {
        NO_DETAIL.to_string()
    } else {
        kept
    }
}

/// Decode one value from the news chain.
///
/// A value that is not an `ExtractedArticle` becomes a diagnostic naming the
/// topic and offset. The diagnostic carries only the JSON error class and
/// position, never the value itself.
pub fn decode_news(topic: &str, offset: i64, value: &[u8]) -> Incoming {
    match serde_json::from_slice::<ExtractedArticle>(value) {
        Ok(article) => Incoming::Record(OutputRecord::from_article(article)),
        Err(e) => Incoming::Diagnostic(format!(
            "{topic} offset {offset}: not an extracted article: {}",
            describe_json_error(&e)
        )),
    }
}

/// Decode one value from the social topic.
///
/// Every record read here is labelled social (topic of origin), whatever
/// `source_type` it claims; a claimed `news` adds a diagnostic. An RFC 3339
/// `published` is converted to UTC; any other value becomes null with a
/// diagnostic. A value that is not a
/// `SourceItem` becomes one diagnostic, which carries only the JSON error
/// class and position. No diagnostic includes the value.
pub fn decode_social(topic: &str, offset: i64, value: &[u8]) -> Vec<Incoming> {
    let mut item = match serde_json::from_slice::<SourceItem>(value) {
        Ok(item) => item,
        Err(e) => {
            return vec![Incoming::Diagnostic(format!(
                "{topic} offset {offset}: not a social item: {}",
                describe_json_error(&e)
            ))];
        }
    };

    let mut out = Vec::new();
    if item.source_type == SourceType::News {
        out.push(Incoming::Diagnostic(format!(
            "{topic} offset {offset}: record claims source_type news; labelled social by topic of origin"
        )));
    }
    if let Some(published) = item.published.take() {
        item.published = rfc3339_to_utc(&published);
        if item.published.is_none() {
            out.push(Incoming::Diagnostic(format!(
                "{topic} offset {offset}: published is not RFC 3339; set to null"
            )));
        }
    }
    out.push(Incoming::Record(OutputRecord::from_social(item)));
    out
}

/// Describe a stream error without the record value.
///
/// A chain runtime error carries the whole failing record (page HTML), and
/// its hint quotes parts of it. The diagnostic keeps the offset and only the
/// module-authored start of the hint (see `hint_summary`); other errors keep
/// only the same kind of start of their text. Neither reads the stored record
/// key or value.
pub fn describe_stream_error(topic: &str, code: &ErrorCode) -> String {
    match code {
        ErrorCode::SmartModuleRuntimeError(e) => format!(
            "{topic} offset {}: SmartModule error: {}",
            e.offset,
            hint_summary(&e.hint)
        ),
        other => format!("{topic}: stream error: {}", hint_summary(&other.to_string())),
    }
}

/// Most times one news branch reopens its stream past a record the chain
/// rejected. After that the branch ends.
const MAX_REOPENS: usize = 100;

/// One decoded item of the news stream, or the stream error.
type NewsItem = Result<Incoming, ErrorCode>;

/// Decode each record of a raw news stream; stream errors pass through.
fn decoded_news<S>(stream: S, topic: String) -> impl Stream<Item = NewsItem> + Unpin + use<S>
where
    S: Stream<Item = Result<Record, ErrorCode>> + Unpin,
{
    stream.map(move |item| item.map(|record| decode_news(&topic, record.offset(), record.value())))
}

/// State of the news branch between items.
struct NewsBranch<S, F> {
    topic: String,
    stream: Option<S>,
    /// The offset the current stream was reopened at, if it was.
    start: Option<i64>,
    /// A last diagnostic to give before the branch ends.
    closing: Option<String>,
    reopen: F,
    reopens: usize,
}

/// Turn a decoded news stream into `Incoming` items.
///
/// A retry-disabled consumer cannot move past a record the chain rejects, so
/// a `SmartModule` runtime error becomes one diagnostic and the branch calls
/// `reopen` with the next offset, at most `MAX_REOPENS` times.
///
/// The cluster runs the chain over whole stored batches, whatever the start
/// offset. A reopened stream that fails again on a record before its start
/// offset therefore started inside the rejected record's batch: the record at
/// the start offset cannot pass the chain either, so it is reported as
/// skipped and the next reopen starts one further on.
///
/// Any other stream error, a failed reopen or the reopen limit gives a
/// fatal diagnostic (`Incoming::Fatal`) and ends the branch.
fn news_incoming<S, F, Fut>(
    first: S,
    topic: &str,
    reopen: F,
) -> impl Stream<Item = Incoming> + Unpin + use<S, F, Fut>
where
    S: Stream<Item = NewsItem> + Unpin,
    F: FnMut(i64) -> Fut,
    Fut: Future<Output = miette::Result<S>>,
{
    let branch = NewsBranch {
        topic: topic.to_string(),
        stream: Some(first),
        start: None,
        closing: None,
        reopen,
        reopens: 0,
    };
    Box::pin(futures_util::stream::unfold(branch, |mut b| async move {
        if let Some(message) = b.closing.take() {
            b.stream = None;
            return Some((Incoming::Fatal(message), b));
        }
        let code = match b.stream.as_mut()?.next().await? {
            Ok(incoming) => return Some((incoming, b)),
            Err(code) => code,
        };
        b.stream = None;
        let ErrorCode::SmartModuleRuntimeError(e) = &code else {
            return Some((Incoming::Fatal(describe_stream_error(&b.topic, &code)), b));
        };
        let (message, next) = match b.start {
            Some(start) if e.offset < start => (
                format!(
                    "{} offset {start}: stored in one batch with rejected offset {}, \
                     so it cannot pass the SmartModule chain",
                    b.topic, e.offset
                ),
                start.saturating_add(1),
            ),
            _ => (describe_stream_error(&b.topic, &code), e.offset.saturating_add(1)),
        };
        if b.reopens >= MAX_REOPENS {
            b.closing = Some(format!(
                "{}: stopped after skipping {MAX_REOPENS} records the SmartModule chain rejected",
                b.topic
            ));
            return Some((Incoming::Diagnostic(message), b));
        }
        b.reopens += 1;
        match (b.reopen)(next).await {
            Ok(stream) => {
                b.stream = Some(stream);
                b.start = Some(next);
            }
            Err(err) => {
                b.closing = Some(format!(
                    "{}: could not reopen at offset {next}: {}",
                    b.topic,
                    hint_summary(&err.to_string())
                ));
            }
        }
        Some((Incoming::Diagnostic(format!("{message}; skipped")), b))
    }))
}

/// Turn a raw social stream into `Incoming` items.
///
/// Each record gives its `decode_social` items, and the first stream error
/// gives one diagnostic and ends the branch. The social stream has no chain,
/// so no record can be rejected and there is nothing to reopen past.
fn social_incoming<S>(stream: S, topic: &str) -> impl Stream<Item = Incoming> + Unpin + use<S>
where
    S: Stream<Item = Result<Record, ErrorCode>> + Unpin,
{
    let topic = topic.to_string();
    stream
        .scan(false, move |failed, item| {
            if *failed {
                return future::ready(None);
            }
            let items = match item {
                Ok(record) => decode_social(&topic, record.offset(), record.value()),
                Err(code) => {
                    *failed = true;
                    vec![Incoming::Fatal(describe_stream_error(&topic, &code))]
                }
            };
            future::ready(Some(futures_util::stream::iter(items)))
        })
        .flatten()
}

/// Merge the news and social branches into one stream.
///
/// `stream::select` polls both fairly and ends only when both end. Each
/// branch keeps its own order.
pub fn merge_incoming<N, S>(news: N, social: S) -> impl Stream<Item = Incoming> + Unpin
where
    N: Stream<Item = Incoming> + Unpin,
    S: Stream<Item = Incoming> + Unpin,
{
    futures_util::stream::select(news, social)
}

/// Collect up to `limit` records, in arrival order.
///
/// Stops when the stream ends or nothing arrives within `idle`. Diagnostics
/// are kept separately and do not count toward `limit`; a fatal one also
/// marks the run failed.
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
            Ok(Some(Incoming::Fatal(message))) => {
                collected.failed = true;
                collected.diagnostics.push(message);
            }
            Ok(None) | Err(_) => break,
        }
    }
    collected
}

// ── NER enrichment ───────────────────────────────────────────────────────────

/// Something that finds named entities in text. `NerEngine` in production,
/// a fake in tests.
pub trait EntityDetector {
    /// Find the named entities in `text`.
    fn detect(&self, text: &str) -> miette::Result<Vec<NamedEntity>>;
}

impl EntityDetector for NerEngine {
    fn detect(&self, text: &str) -> miette::Result<Vec<NamedEntity>> {
        NerEngine::detect(self, text)
    }
}

/// Longest social NER input, in characters.
pub const NER_INPUT_CHARS: usize = 500;

/// Lower-case label of a source type, as it appears in the JSON output.
fn kind_label(kind: SourceType) -> &'static str {
    match kind {
        SourceType::News => "news",
        SourceType::Social => "social",
    }
}

/// The text handed to the NER detector for one record.
///
/// News uses its `text` (the body preview) unchanged. Social uses the title,
/// a newline and the text, or only the text when there is no title, cut to
/// the first `NER_INPUT_CHARS` characters (Unicode scalar values, never
/// bytes). The record itself is never changed.
pub fn ner_input(record: &OutputRecord) -> String {
    match record.source_type {
        SourceType::News => record.text.clone(),
        SourceType::Social => {
            let full = match record.title.as_deref() {
                Some(title) if !title.is_empty() => format!("{title}\n{}", record.text),
                _ => record.text.clone(),
            };
            full.chars().take(NER_INPUT_CHARS).collect()
        }
    }
}

/// Run the detector over every record and store the entities it returns.
///
/// With no detector nothing changes. A record whose NER input is blank is
/// skipped. A detector error leaves that record unchanged and returns one
/// diagnostic naming the record. Records are never dropped.
pub fn enrich(records: &mut [OutputRecord], detector: Option<&dyn EntityDetector>) -> Vec<String> {
    let Some(detector) = detector else {
        return Vec::new();
    };
    let mut diagnostics = Vec::new();
    for record in records.iter_mut() {
        let input = ner_input(record);
        if input.trim().is_empty() {
            continue;
        }
        match detector.detect(&input) {
            Ok(entities) => record.entities = entities,
            Err(e) => diagnostics.push(format!(
                "{} {}: NER failed ({})",
                kind_label(record.source_type),
                output::sanitize_for_terminal(&record.id),
                truncate_detail(&e.to_string())
            )),
        }
    }
    diagnostics
}

/// Whether a `--source` selection reads news.
fn reads_news(source: SourceFilter) -> bool {
    matches!(source, SourceFilter::Both | SourceFilter::News)
}

/// Whether a `--source` selection reads social items.
fn reads_social(source: SourceFilter) -> bool {
    matches!(source, SourceFilter::Both | SourceFilter::Social)
}

/// The topics and chain modules a `--source` selection needs.
///
/// The news chain modules are required only when news is selected.
pub fn required_inputs<'a>(
    source: SourceFilter,
    topics: &ConsumeTopics<'a>,
) -> (Vec<&'a str>, &'static [&'static str]) {
    let mut needed = Vec::new();
    if reads_news(source) {
        needed.push(topics.raw_pages);
    }
    if reads_social(source) {
        needed.push(topics.social);
    }
    let chain: &'static [&'static str] = if reads_news(source) { &NEWS_CHAIN } else { &[] };
    (needed, chain)
}

/// Check the selected topics and chain, then open the merged stream.
///
/// The social stream is opened with no chain modules, so the chain never sees
/// social records. Preflight runs before any stream opens, and an unselected
/// source is an empty branch. The news branch reopens its stream past a
/// record the chain rejects (see `news_incoming`).
async fn open_incoming<'s>(
    stream: &'s FluvioStream,
    topics: &ConsumeTopics<'_>,
    offset: Offset,
    source: SourceFilter,
) -> miette::Result<impl Stream<Item = Incoming> + Unpin + use<'s>> {
    let (needed, chain) = required_inputs(source, topics);
    stream.preflight(&needed, chain).await?;

    let news = if reads_news(source) {
        let raw_pages = topics.raw_pages.to_string();
        let raw = stream
            .topic_stream(&raw_pages, offset.clone(), news_chain())
            .await?;
        let first = decoded_news(raw, raw_pages.clone());
        let reopen = move |next: i64| {
            let raw_pages = raw_pages.clone();
            async move {
                let offset = Offset::absolute(next)
                    .map_err(|e| miette::miette!("bad offset {next}: {e}"))?;
                let raw = stream.topic_stream(&raw_pages, offset, news_chain()).await?;
                Ok(decoded_news(raw, raw_pages))
            }
        };
        news_incoming(first, topics.raw_pages, reopen).boxed()
    } else {
        futures_util::stream::empty().boxed()
    };
    let social = if reads_social(source) {
        let raw = stream
            .topic_stream(topics.social, offset, Vec::new())
            .await?;
        social_incoming(raw, topics.social).boxed()
    } else {
        futures_util::stream::empty().boxed()
    };
    Ok(merge_incoming(news, social))
}

/// Check the selected topics and chain, then collect up to `limit` records of
/// both kinds together.
pub async fn collect_from_topics(
    stream: &FluvioStream,
    topics: &ConsumeTopics<'_>,
    offset: Offset,
    limit: usize,
    source: SourceFilter,
) -> miette::Result<Collected> {
    let merged = open_incoming(stream, topics, offset, source).await?;
    Ok(collect_records(merged, limit, IDLE_TIMEOUT).await)
}

/// Reject `--enrich-ner` in streaming mode (`--limit 0`), which never runs NER.
pub fn check_ner_mode(enrich_ner: bool, limit: usize) -> miette::Result<()> {
    if enrich_ner && limit == 0 {
        return Err(miette::miette!(
            help = "add --limit N to enrich a batch of N records",
            "--enrich-ner works only in batch mode (--limit N); streaming mode (--limit 0) prints records without NER"
        ));
    }
    Ok(())
}

/// Consume news and social records from Fluvio.
///
/// # Arguments
/// * `from_beginning` - Start consuming from the beginning of the topics
/// * `limit` - Maximum records across news and social (0 = unlimited streaming)
/// * `source` - Which sources to read and preflight
/// * `output` - JSON output file (batch mode)
/// * `ner` - Optional NER engine for entity enrichment
pub async fn run(
    from_beginning: bool,
    limit: usize,
    source: SourceFilter,
    output: &Path,
    ner: Option<&NerEngine>,
) -> miette::Result<()> {
    check_ner_mode(ner.is_some(), limit)?;
    println!("[Consume] Connecting to Fluvio cluster...");
    let stream = FluvioStream::connect().await?;

    let offset = if from_beginning {
        Offset::beginning()
    } else {
        Offset::end()
    };

    let topics = ConsumeTopics::default();
    if reads_news(source) {
        println!(
            "[Consume] Reading {} through {}",
            topics.raw_pages,
            NEWS_CHAIN.join(" → ")
        );
    }
    if reads_social(source) {
        println!("[Consume] Reading {} with no SmartModules", topics.social);
    }

    if limit > 0 {
        let batch = Batch {
            limit,
            source,
            output,
            ner,
        };
        consume_batch(&stream, &topics, offset, &batch).await
    } else {
        consume_stream(&stream, &topics, offset, source).await
    }
}

/// Settings for one batch run.
struct Batch<'a> {
    limit: usize,
    source: SourceFilter,
    output: &'a Path,
    ner: Option<&'a NerEngine>,
}

/// Consume a batch of records, enrich them, print them and write the JSON file.
async fn consume_batch(
    stream: &FluvioStream,
    topics: &ConsumeTopics<'_>,
    offset: Offset,
    batch: &Batch<'_>,
) -> miette::Result<()> {
    let Batch {
        limit,
        source,
        output: path,
        ner,
    } = *batch;
    println!("[Consume] Will consume up to {limit} records.\n");
    let Collected {
        mut records,
        diagnostics,
        failed,
    } = collect_from_topics(stream, topics, offset, limit, source).await?;

    for message in &diagnostics {
        print_diagnostic(message);
    }

    for message in enrich(&mut records, ner.map(|engine| engine as &dyn EntityDetector)) {
        print_diagnostic(&message);
    }

    for record in &records {
        output::print_summary(record);
    }

    finish_batch(&records, failed, path)
}

/// Write the batch file, then report a failed stream as an error.
///
/// When a stream failed before any record arrived the file is not written,
/// so the last good output survives. Records read before a failure are
/// written, and the run still fails.
fn finish_batch(records: &[OutputRecord], failed: bool, path: &Path) -> miette::Result<()> {
    if failed && records.is_empty() {
        return Err(miette::miette!(
            "a stream failed before any record arrived; {} was not written",
            path.display()
        ));
    }
    output::save_json(records, path)
        .map_err(|e| miette::miette!("Failed to write {}: {e}", path.display()))?;
    output::print_run_summary(records.len(), &path.display().to_string());
    if failed {
        return Err(miette::miette!(
            "a stream failed; {} holds the {} records read before it",
            path.display(),
            records.len()
        ));
    }
    Ok(())
}

/// One stderr diagnostic line, with control characters removed.
fn diagnostic_line(message: &str) -> String {
    format!("  ✗ {}", output::sanitize_for_terminal(message))
}

/// The single place consume writes a diagnostic to stderr.
fn print_diagnostic(message: &str) {
    eprintln!("{}", diagnostic_line(message));
}

/// Consume records in streaming mode (unlimited). Print only: no NER, no file.
async fn consume_stream(
    stream: &FluvioStream,
    topics: &ConsumeTopics<'_>,
    offset: Offset,
    source: SourceFilter,
) -> miette::Result<()> {
    let mut incoming = open_incoming(stream, topics, offset, source).await?;
    println!("[Consume] Press Ctrl+C to stop.\n");

    let mut failed = false;
    while let Some(item) = incoming.next().await {
        match item {
            Incoming::Record(record) => output::print_summary(&record),
            Incoming::Diagnostic(message) => print_diagnostic(&message),
            Incoming::Fatal(message) => {
                failed = true;
                print_diagnostic(&message);
            }
        }
    }
    if failed {
        return Err(miette::miette!("consume stopped: a stream ended on an error"));
    }
    Ok(())
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
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

    fn social_item(id: &str, url: &str, text: &str) -> SourceItem {
        SourceItem::social(
            id.to_string(),
            url.to_string(),
            None,
            text.to_string(),
            None,
            None,
            "reddit".to_string(),
            None,
        )
    }

    fn social(id: &str) -> Incoming {
        Incoming::Record(OutputRecord::from_social(social_item(
            id,
            &format!("https://www.reddit.com/r/test/comments/{id}"),
            "lol cat",
        )))
    }

    fn iter(items: Vec<Incoming>) -> futures_util::stream::Iter<std::vec::IntoIter<Incoming>> {
        futures_util::stream::iter(items)
    }

    fn json(value: &serde_json::Value) -> Vec<u8> {
        serde_json::to_vec(value).expect("serializable")
    }

    fn split(items: Vec<Incoming>) -> (Vec<OutputRecord>, Vec<String>) {
        let mut records = Vec::new();
        let mut diagnostics = Vec::new();
        for item in items {
            match item {
                Incoming::Record(r) => records.push(r),
                Incoming::Diagnostic(d) => diagnostics.push(d),
                Incoming::Fatal(d) => panic!("a decode gave a fatal item: {d:?}"),
            }
        }
        (records, diagnostics)
    }

    fn ids_of(records: &[OutputRecord], kind: SourceType) -> Vec<&str> {
        records
            .iter()
            .filter(|r| r.source_type == kind)
            .map(|r| r.id.as_str())
            .collect()
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

    /// Reddit-shaped social value with the given `source_type`.
    fn poison_social(source_type: &str) -> serde_json::Value {
        serde_json::json!({
            "source_type": source_type,
            "platform": "reddit",
            "id": "t3_poison",
            "url": "https://www.reddit.com/r/test/comments/poison",
            "text": "PAGE-BODY text"
        })
    }

    #[test]
    fn decode_social_withholds_variant_value() {
        let esc = poison_social("\u{1b}[2JPAGE-BODY");
        let ticks = poison_social(r"a`b`PAGE-BODY");
        for value in [esc, ticks] {
            let (records, diagnostics) =
                split(decode_social("filimon-social-items", 9, &json(&value)));
            assert!(records.is_empty(), "{value}");
            assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
            let message = &diagnostics[0];
            assert!(message.contains("filimon-social-items"), "{message:?}");
            assert!(message.contains('9'), "{message:?}");
            assert!(message.contains("line 1 column"), "{message:?}");
            assert!(!message.contains("PAGE-BODY"), "{message:?}");
            assert!(!message.contains('\u{1b}'), "{message:?}");
            assert!(!message.contains('`'), "{message:?}");
        }
    }

    /// The 02-VERIFICATION.md reproduction: a JSON string with one escaped
    /// inner quote before the secret.
    const ESCAPED_QUOTE_VALUE: &[u8] = br#""a\"PAGE-BODY-SECRET""#;

    fn assert_content_free(message: &str) {
        assert!(!message.contains("PAGE-BODY"), "{message:?}");
        assert!(!message.contains('"'), "{message:?}");
        assert!(!message.contains('`'), "{message:?}");
    }

    #[test]
    fn decode_news_withholds_escaped_quote_content() {
        let three_quotes: &[u8] = br#""a\"b\"c\"PAGE-BODY-SECRET""#;
        let decoded: String = serde_json::from_slice(ESCAPED_QUOTE_VALUE).expect("JSON string");
        let article_shaped = json(&serde_json::json!({
            "url": "https://example.com/poison",
            "title": "Title",
            "body_preview": "body",
            "word_count": 1,
            "relevance_score": decoded,
            "top_keywords": [],
            "named_entities": [],
            "inferred_category": "General"
        }));
        for value in [ESCAPED_QUOTE_VALUE, three_quotes, article_shaped.as_slice()] {
            let Incoming::Diagnostic(message) = decode_news("filimon-raw-pages", 42, value) else {
                panic!("expected a diagnostic");
            };
            assert!(message.contains("filimon-raw-pages"), "{message:?}");
            assert!(message.contains("42"), "{message:?}");
            assert!(message.contains("line 1 column"), "{message:?}");
            assert_content_free(&message);
        }
    }

    #[test]
    fn empty_and_malformed_values_are_reported() {
        let Incoming::Diagnostic(message) = decode_news("filimon-raw-pages", 3, b"") else {
            panic!("expected a diagnostic");
        };
        assert!(message.contains("filimon-raw-pages offset 3"), "{message:?}");
        assert!(message.contains("truncated JSON at line 1 column 0"), "{message:?}");

        let (records, diagnostics) = split(decode_social("filimon-social-items", 4, b""));
        assert!(records.is_empty());
        assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
        assert!(
            diagnostics[0].contains("filimon-social-items offset 4"),
            "{:?}",
            diagnostics[0]
        );
        assert!(
            diagnostics[0].contains("truncated JSON at line 1 column 0"),
            "{:?}",
            diagnostics[0]
        );

        // Bytes that are not valid UTF-8 inside a JSON string.
        let Incoming::Diagnostic(message) =
            decode_news("filimon-raw-pages", 5, b"\"\xFFPAGE-BODY\"")
        else {
            panic!("expected a diagnostic");
        };
        assert!(message.contains("filimon-raw-pages offset 5"), "{message:?}");
        assert!(!message.contains("PAGE-BODY"), "{message:?}");

        let (records, diagnostics) = split(decode_social(
            "filimon-social-items",
            6,
            &json(&poison_social("")),
        ));
        assert!(records.is_empty());
        assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
        assert!(
            diagnostics[0].contains("filimon-social-items offset 6"),
            "{:?}",
            diagnostics[0]
        );
        assert!(!diagnostics[0].contains('`'), "{:?}", diagnostics[0]);
    }

    fn runtime_error(hint: String) -> ErrorCode {
        ErrorCode::SmartModuleRuntimeError(Box::new(SmartModuleTransformRuntimeError {
            hint,
            offset: 11,
            record_value: "PAGE-BODY-SECRET".into(),
            ..Default::default()
        }))
    }

    #[test]
    fn stream_error_hint_withholds_quoted_value() {
        /// Mirror of map-preprocess's `RawPage`.
        #[derive(serde::Deserialize)]
        #[allow(dead_code)]
        struct SmRawPage {
            url: String,
            html: String,
        }

        let number_url = json(&serde_json::json!({ "url": 424242, "html": "x" }));
        for payload in [ESCAPED_QUOTE_VALUE, number_url.as_slice()] {
            let Err(e) = serde_json::from_slice::<SmRawPage>(payload) else {
                panic!("payload must not parse");
            };
            // map-preprocess: eyre!("Failed to parse RawPage: {}", e); fluvio
            // then formats the eyre report with Debug, which adds Location.
            let hint = format!("Failed to parse RawPage: {e}\n\nLocation:\n    src/lib.rs:172:10");
            let message = describe_stream_error("filimon-raw-pages", &runtime_error(hint));
            assert!(message.contains("offset 11"), "{message:?}");
            assert!(message.contains("SmartModule error"), "{message:?}");
            assert!(message.contains("Failed to parse RawPage"), "{message:?}");
            assert!(!message.contains("424242"), "{message:?}");
            assert!(!message.contains("Location"), "{message:?}");
            assert_content_free(&message);
        }

        let message =
            describe_stream_error("filimon-raw-pages", &runtime_error("bad\u{1b}[2J tail".into()));
        assert!(message.contains("bad"), "{message:?}");
        assert!(!message.contains('\u{1b}'), "{message:?}");
        assert!(!message.contains("tail"), "{message:?}");

        for hint in [String::new(), "\"PAGE-BODY\" first".to_string()] {
            let message = describe_stream_error("filimon-raw-pages", &runtime_error(hint));
            assert!(message.contains("no detail"), "{message:?}");
            assert!(message.contains("offset 11"), "{message:?}");
            assert!(!message.contains("PAGE-BODY"), "{message:?}");
        }
    }

    #[test]
    fn hint_detail_truncates_by_chars() {
        for c in ['東', '🦀'] {
            let hint: String = std::iter::repeat_n(c, 300).collect();
            let message = describe_stream_error("filimon-raw-pages", &runtime_error(hint));
            assert_eq!(message.chars().filter(|&m| m == c).count(), 200, "{c}");
        }
    }

    #[test]
    fn other_stream_errors_withhold_record_value() {
        let look_back = ErrorCode::SmartModuleLookBackError(
            "Failed to parse RawPage: invalid type: string \"PAGE-BODY\"\n\nSmartModule Lookback Error:\nOffset: 3\nKey: NULL\nValue: PAGE-BODY".into(),
        );
        let other = ErrorCode::Other("\u{1b}[2J\"PAGE-BODY\"".into());
        for code in [look_back, other] {
            let message = describe_stream_error("filimon-raw-pages", &code);
            assert!(message.contains("filimon-raw-pages"), "{message:?}");
            assert!(!message.contains("PAGE-BODY"), "{message:?}");
            assert!(!message.contains('\u{1b}'), "{message:?}");
        }

        let not_found = ErrorCode::SmartModuleNotFound {
            name: "filimon/missing@0.1.0".into(),
        };
        let message = describe_stream_error("filimon-raw-pages", &not_found);
        assert!(message.contains("filimon/missing@0.1.0"), "{message:?}");
    }

    fn runtime_error_at(offset: i64) -> ErrorCode {
        ErrorCode::SmartModuleRuntimeError(Box::new(SmartModuleTransformRuntimeError {
            hint: "Failed to parse RawPage: invalid type: string \"PAGE-BODY\"".into(),
            offset,
            record_value: "PAGE-BODY".into(),
            ..Default::default()
        }))
    }

    type NewsIter = futures_util::stream::Iter<std::vec::IntoIter<NewsItem>>;

    fn news_items(items: Vec<NewsItem>) -> NewsIter {
        futures_util::stream::iter(items)
    }

    #[tokio::test]
    async fn news_branch_reopens_past_rejected_record() {
        let first = news_items(vec![Ok(record("n0")), Err(runtime_error_at(1))]);
        let calls = std::cell::RefCell::new(Vec::new());
        let reopen = |next: i64| {
            calls.borrow_mut().push(next);
            future::ready(Ok(news_items(vec![Ok(record("n2"))])))
        };
        let collected =
            collect_records(news_incoming(first, "raw", reopen), 10, Duration::from_secs(1)).await;
        assert_eq!(*calls.borrow(), [2]);
        let ids: Vec<&str> = collected.records.iter().map(|r| r.id.as_str()).collect();
        assert_eq!(ids, ["n0", "n2"]);
        assert!(!collected.failed, "a skipped record is not a failed stream");
        assert_eq!(collected.diagnostics.len(), 1, "{:?}", collected.diagnostics);
        let message = &collected.diagnostics[0];
        assert!(message.contains("raw offset 1"), "{message:?}");
        assert!(message.contains("skipped"), "{message:?}");
        assert!(!message.contains("PAGE-BODY"), "{message:?}");
    }

    #[tokio::test]
    async fn news_branch_steps_through_rejected_batch() {
        // The cluster feeds the chain whole stored batches, so a stream
        // reopened inside the rejected record's batch fails on it again.
        let first = news_items(vec![Err(runtime_error_at(0))]);
        let calls = std::cell::RefCell::new(Vec::new());
        let reopen = |next: i64| {
            calls.borrow_mut().push(next);
            let items = if next < 3 {
                vec![Err(runtime_error_at(0))]
            } else {
                vec![Ok(record("n3"))]
            };
            future::ready(Ok(news_items(items)))
        };
        let collected =
            collect_records(news_incoming(first, "raw", reopen), 10, Duration::from_secs(1)).await;
        assert_eq!(*calls.borrow(), [1, 2, 3]);
        let ids: Vec<&str> = collected.records.iter().map(|r| r.id.as_str()).collect();
        assert_eq!(ids, ["n3"]);
        assert!(!collected.failed);
        let d = &collected.diagnostics;
        assert_eq!(d.len(), 3, "{d:?}");
        assert!(d[0].contains("raw offset 0: SmartModule error"), "{d:?}");
        for (message, offset) in d[1..].iter().zip(1..) {
            assert!(message.contains(&format!("raw offset {offset}: stored in one batch")), "{d:?}");
            assert!(message.contains("rejected offset 0"), "{d:?}");
        }
        assert!(d.iter().all(|m| m.ends_with("; skipped")), "{d:?}");
        assert!(d.iter().all(|m| !m.contains("PAGE-BODY")), "{d:?}");
    }

    #[tokio::test]
    async fn news_branch_ends_on_other_stream_error() {
        let first = news_items(vec![
            Err(ErrorCode::Other("broken".into())),
            Ok(record("never")),
        ]);
        let reopen = |_next: i64| -> future::Ready<miette::Result<NewsIter>> {
            panic!("must not reopen on a non-SmartModule error")
        };
        let collected =
            collect_records(news_incoming(first, "raw", reopen), 10, Duration::from_secs(1)).await;
        assert!(collected.records.is_empty(), "{:?}", collected.records);
        assert!(collected.failed);
        assert_eq!(collected.diagnostics.len(), 1, "{:?}", collected.diagnostics);
    }

    #[tokio::test]
    async fn news_branch_reopens_at_most_max_times() {
        let first = news_items(vec![Err(runtime_error_at(0))]);
        let calls = std::cell::Cell::new(0_usize);
        let reopen = |next: i64| {
            calls.set(calls.get() + 1);
            future::ready(Ok(news_items(vec![Err(runtime_error_at(next))])))
        };
        let collected =
            collect_records(news_incoming(first, "raw", reopen), 10, Duration::from_secs(1)).await;
        assert_eq!(calls.get(), MAX_REOPENS);
        assert!(collected.records.is_empty());
        assert!(collected.failed);
        // One per rejected record, then the closing line.
        assert_eq!(collected.diagnostics.len(), MAX_REOPENS + 2);
        let last = collected.diagnostics.last().expect("closing line");
        assert!(last.contains("stopped after skipping"), "{last:?}");
    }

    #[tokio::test]
    async fn news_branch_reports_failed_reopen() {
        let first = news_items(vec![Err(runtime_error_at(4))]);
        let reopen = |_next: i64| future::ready(Err::<NewsIter, _>(miette::miette!("no stream")));
        let collected =
            collect_records(news_incoming(first, "raw", reopen), 10, Duration::from_secs(1)).await;
        assert!(collected.records.is_empty());
        assert!(collected.failed);
        let d = &collected.diagnostics;
        assert_eq!(d.len(), 2, "{d:?}");
        assert!(d[1].contains("could not reopen at offset 5"), "{d:?}");
    }

    #[tokio::test]
    async fn fatal_item_marks_collection_failed() {
        let items = vec![
            record("a"),
            Incoming::Diagnostic("skipped".into()),
            Incoming::Fatal("ended".into()),
        ];
        let collected = collect_records(iter(items), 10, Duration::from_secs(1)).await;
        assert!(collected.failed);
        assert_eq!(collected.records.len(), 1);
        assert_eq!(collected.diagnostics, ["skipped", "ended"]);

        let clean = vec![record("a"), Incoming::Diagnostic("skipped".into())];
        let collected = collect_records(iter(clean), 10, Duration::from_secs(1)).await;
        assert!(!collected.failed);
    }

    #[test]
    fn failed_batch_with_no_records_keeps_previous_output() {
        let dir = tempfile::TempDir::new().expect("temp dir");
        let path = dir.path().join("out.json");
        std::fs::write(&path, "[\"last good\"]").expect("seed file");
        let err = finish_batch(&[], true, &path).expect_err("failed run");
        assert!(err.to_string().contains("not written"), "{err}");
        assert_eq!(std::fs::read_to_string(&path).expect("read"), "[\"last good\"]");
    }

    #[test]
    fn failed_batch_with_records_writes_them_and_fails() {
        let dir = tempfile::TempDir::new().expect("temp dir");
        let path = dir.path().join("out.json");
        let records = vec![OutputRecord::from_article(article("https://example.com/a"))];
        assert!(finish_batch(&records, true, &path).is_err());
        let text = std::fs::read_to_string(&path).expect("read");
        let value: serde_json::Value = serde_json::from_str(&text).expect("json");
        assert_eq!(value.as_array().map(Vec::len), Some(1));
    }

    #[test]
    fn clean_empty_batch_writes_empty_array() {
        let dir = tempfile::TempDir::new().expect("temp dir");
        let path = dir.path().join("out.json");
        finish_batch(&[], false, &path).expect("clean run");
        assert_eq!(std::fs::read_to_string(&path).expect("read"), "[]");
    }

    #[test]
    fn enrich_ner_is_rejected_in_streaming_mode() {
        let err = check_ner_mode(true, 0).expect_err("streaming with NER");
        assert!(err.to_string().contains("--enrich-ner"), "{err}");
        assert!(check_ner_mode(true, 5).is_ok());
        assert!(check_ner_mode(false, 0).is_ok());
    }

    #[test]
    fn diagnostic_line_strips_control_chars() {
        let line = diagnostic_line("t offset 1: \u{1b}[2J bad\rworse\nworst");
        assert!(line.starts_with("  ✗"), "{line:?}");
        assert!(!line.chars().any(char::is_control), "{line:?}");
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

    // ── Social decode and the merged stream ──────────────────────────────────

    #[tokio::test]
    async fn merged_limit_counts_both_kinds() {
        let news = iter(vec![record("n1"), record("n2")]);
        let social = iter(vec![social("s1"), social("s2")]);
        let collected =
            collect_records(merge_incoming(news, social), 3, Duration::from_secs(1)).await;
        assert_eq!(collected.records.len(), 3);
        assert!(!ids_of(&collected.records, SourceType::News).is_empty());
        assert!(!ids_of(&collected.records, SourceType::Social).is_empty());
    }

    #[tokio::test]
    async fn low_relevance_social_is_kept() {
        let item = social_item("t3_lol", "https://www.reddit.com/r/test/comments/lol", "lol cat");
        let value = serde_json::to_vec(&item).expect("serializable");
        let social = iter(decode_social("filimon-social-items", 0, &value));
        let collected =
            collect_records(merge_incoming(iter(vec![]), social), 10, Duration::from_secs(1))
                .await;
        assert!(collected.diagnostics.is_empty(), "{:?}", collected.diagnostics);
        assert_eq!(collected.records.len(), 1);
        assert_eq!(collected.records[0].text, "lol cat");
        assert_eq!(collected.records[0].source_type, SourceType::Social);
    }

    #[test]
    fn social_label_comes_from_topic() {
        let value = json(&serde_json::json!({
            "source_type": "news",
            "platform": "reddit",
            "id": "t3_spoof",
            "url": "https://www.reddit.com/r/test/comments/spoof",
            "text": "claims to be news"
        }));
        let (records, diagnostics) = split(decode_social("filimon-social-items", 5, &value));
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].source_type, SourceType::Social);
        assert!(records[0].article.is_none());
        assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
        assert!(diagnostics[0].contains("filimon-social-items"), "{}", diagnostics[0]);
        assert!(diagnostics[0].contains('5'), "{}", diagnostics[0]);
        assert!(diagnostics[0].contains("social"), "{}", diagnostics[0]);
    }

    #[test]
    fn undeserializable_social_record_is_reported() {
        let (records, diagnostics) = split(decode_social(
            "filimon-social-items",
            9,
            br#"{"text":"SECRET-BODY"}"#,
        ));
        assert!(records.is_empty());
        assert_eq!(diagnostics.len(), 1);
        let message = &diagnostics[0];
        assert!(message.contains("filimon-social-items"), "{message}");
        assert!(message.contains('9'), "{message}");
        assert!(!message.contains("SECRET-BODY"), "{message}");

        // An invalid-type error quotes the value; it must still not leak.
        let (records, diagnostics) = split(decode_social("t", 1, br#""SECRET-BODY""#));
        assert!(records.is_empty());
        assert_eq!(diagnostics.len(), 1);
        assert!(!diagnostics[0].contains("SECRET-BODY"), "{}", diagnostics[0]);
    }

    #[tokio::test]
    async fn social_with_empty_text_is_delivered() {
        let value = json(&serde_json::json!({
            "source_type": "social",
            "platform": "reddit",
            "id": "t3_empty",
            "url": "https://www.reddit.com/r/test/comments/empty",
            "text": ""
        }));
        let social = iter(decode_social("filimon-social-items", 0, &value));
        let collected =
            collect_records(merge_incoming(iter(vec![]), social), 10, Duration::from_secs(1))
                .await;
        assert!(collected.diagnostics.is_empty(), "{:?}", collected.diagnostics);
        assert_eq!(collected.records.len(), 1);
        let record = &collected.records[0];
        assert_eq!(record.id, "t3_empty");
        assert_eq!(record.text, "");
        assert!(record.title.is_none());
    }

    #[tokio::test]
    async fn per_source_order_preserved() {
        let news = iter(vec![record("n1"), record("n2"), record("n3")]);
        let social = iter(vec![social("s1"), social("s2"), social("s3")]);
        let collected =
            collect_records(merge_incoming(news, social), 10, Duration::from_secs(1)).await;
        assert_eq!(collected.records.len(), 6);
        assert_eq!(ids_of(&collected.records, SourceType::News), ["n1", "n2", "n3"]);
        assert_eq!(ids_of(&collected.records, SourceType::Social), ["s1", "s2", "s3"]);
    }

    #[tokio::test]
    async fn news_and_social_with_same_id_stay_separate() {
        let url = "https://example.com/same";
        let news = iter(vec![record(url)]);
        let social = iter(vec![
            Incoming::Record(OutputRecord::from_social(social_item(url, url, "a"))),
            Incoming::Record(OutputRecord::from_social(social_item(url, url, "b"))),
        ]);
        let collected =
            collect_records(merge_incoming(news, social), 10, Duration::from_secs(1)).await;
        assert_eq!(collected.records.len(), 3);
        assert!(collected.records.iter().all(|r| r.id == url && r.url == url));
        assert_eq!(ids_of(&collected.records, SourceType::News).len(), 1);
        assert_eq!(ids_of(&collected.records, SourceType::Social).len(), 2);
    }

    #[test]
    fn invalid_social_published_becomes_null() {
        let item = |published: &str| {
            json(&serde_json::json!({
                "source_type": "social",
                "platform": "reddit",
                "id": "t3_date",
                "url": "https://www.reddit.com/r/test/comments/date",
                "text": "hi",
                "published": published
            }))
        };

        let (records, diagnostics) =
            split(decode_social("filimon-social-items", 3, &item("yesterday")));
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].published, None);
        assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
        assert!(diagnostics[0].contains("filimon-social-items"), "{}", diagnostics[0]);
        assert!(diagnostics[0].contains('3'), "{}", diagnostics[0]);
        assert!(!diagnostics[0].contains("yesterday"), "{}", diagnostics[0]);

        let (records, diagnostics) = split(decode_social(
            "filimon-social-items",
            4,
            &item("2024-10-27T03:33:20.123Z"),
        ));
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].published.as_deref(), Some("2024-10-27T03:33:20.123Z"));
        assert!(diagnostics.is_empty(), "{diagnostics:?}");

        // An offset is converted to UTC, as the output contract states.
        let (records, diagnostics) = split(decode_social(
            "filimon-social-items",
            5,
            &item("2024-05-01T10:00:00+01:00"),
        ));
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].published.as_deref(), Some("2024-05-01T09:00:00.000Z"));
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
    }

    #[tokio::test(start_paused = true)]
    async fn idle_timeout_stops_collection() {
        let merged = merge_incoming(
            futures_util::stream::pending::<Incoming>(),
            futures_util::stream::pending::<Incoming>(),
        );
        let collected = collect_records(merged, 5, IDLE_TIMEOUT).await;
        assert!(collected.records.is_empty());
        assert!(collected.diagnostics.is_empty());
    }

    #[test]
    fn required_inputs_follow_source_filter() {
        let topics = ConsumeTopics::default();

        let (needed, chain) = required_inputs(SourceFilter::Both, &topics);
        assert_eq!(needed, ["filimon-raw-pages", "filimon-social-items"]);
        assert_eq!(chain, NEWS_CHAIN);

        let (needed, chain) = required_inputs(SourceFilter::News, &topics);
        assert_eq!(needed, ["filimon-raw-pages"]);
        assert_eq!(chain, NEWS_CHAIN);

        let (needed, chain) = required_inputs(SourceFilter::Social, &topics);
        assert_eq!(needed, ["filimon-social-items"]);
        assert!(chain.is_empty(), "{chain:?}");
    }

    // ── NER enrichment ───────────────────────────────────────────────────────

    /// Records every input and returns one entity per call.
    #[derive(Default)]
    struct RecordingDetector {
        calls: std::cell::RefCell<Vec<String>>,
    }

    impl EntityDetector for RecordingDetector {
        fn detect(&self, text: &str) -> miette::Result<Vec<NamedEntity>> {
            self.calls.borrow_mut().push(text.to_string());
            Ok(vec![NamedEntity {
                text: "Ferris".to_string(),
                kind: "PERSON".to_string(),
            }])
        }
    }

    /// Always fails.
    struct FailingDetector;

    impl EntityDetector for FailingDetector {
        fn detect(&self, _text: &str) -> miette::Result<Vec<NamedEntity>> {
            Err(miette::miette!("model exploded"))
        }
    }

    fn news_record(text: &str) -> OutputRecord {
        let mut a = article("https://example.com/news");
        a.body_preview = text.to_string();
        OutputRecord::from_article(a)
    }

    fn social_record(id: &str, title: Option<&str>, text: &str) -> OutputRecord {
        let mut item = social_item(id, &format!("https://www.reddit.com/r/test/comments/{id}"), text);
        item.title = title.map(str::to_string);
        OutputRecord::from_social(item)
    }

    #[test]
    fn ner_runs_on_social_text() {
        let mut records = vec![
            news_record("Markets rallied"),
            social_record("t3_f", Some("Ferris"), "lol cat"),
        ];
        let detector = RecordingDetector::default();
        let diagnostics = enrich(&mut records, Some(&detector));
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
        assert_eq!(*detector.calls.borrow(), ["Markets rallied", "Ferris\nlol cat"]);
        for record in &records {
            let texts: Vec<&str> = record.entities.iter().map(|e| e.text.as_str()).collect();
            assert_eq!(texts, ["Ferris"], "{}", record.id);
            assert_eq!(record.entities[0].kind, "PERSON");
        }
    }

    #[test]
    fn ner_input_keeps_news_text() {
        let text = "x".repeat(600) + " Zürich 🦀";
        let record = news_record(&text);
        assert!(record.title.is_some());
        assert_eq!(ner_input(&record).as_bytes(), text.as_bytes());
    }

    #[test]
    fn ner_input_truncates_by_chars() {
        let input = ner_input(&social_record("a", None, &"é".repeat(600)));
        assert_eq!(input.chars().count(), 500);
        assert_eq!(input.len(), 1000);
        assert_eq!(input, "é".repeat(500));

        let input = ner_input(&social_record("b", None, &"🦀".repeat(501)));
        assert_eq!(input, "🦀".repeat(500));

        // The cap applies to title + newline + text together.
        let title = "t".repeat(499);
        let input = ner_input(&social_record("c", Some(&title), "body"));
        assert_eq!(input.chars().count(), 500);
        assert_eq!(input, format!("{title}\n"));

        let title = "t".repeat(498);
        let input = ner_input(&social_record("d", Some(&title), "body"));
        assert_eq!(input.chars().count(), 500);
        assert_eq!(input, format!("{title}\nb"));
    }

    #[test]
    fn empty_ner_input_skips_detector() {
        let mut records = vec![
            social_record("e1", None, ""),
            social_record("e2", None, "  \n\t "),
            social_record("e3", Some(""), ""),
        ];
        let detector = RecordingDetector::default();
        let diagnostics = enrich(&mut records, Some(&detector));
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
        assert!(detector.calls.borrow().is_empty(), "{:?}", detector.calls.borrow());
        assert!(records.iter().all(|r| r.entities.is_empty()));
    }

    #[test]
    fn detector_error_keeps_record() {
        let mut records = vec![social_record("t3_fail", None, "hello")];
        let diagnostics = enrich(&mut records, Some(&FailingDetector));
        assert_eq!(records.len(), 1);
        assert!(records[0].entities.is_empty());
        assert_eq!(records[0].text, "hello");
        assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
        assert!(diagnostics[0].contains("t3_fail"), "{}", diagnostics[0]);
        assert!(diagnostics[0].contains("NER failed"), "{}", diagnostics[0]);
    }

    #[test]
    fn no_detector_leaves_entities_empty() {
        let mut records = vec![
            news_record("Markets rallied"),
            social_record("t3_n", Some("Ferris"), "lol cat"),
        ];
        let diagnostics = enrich(&mut records, None);
        assert!(diagnostics.is_empty());
        assert!(records.iter().all(|r| r.entities.is_empty()));

        let mut none: Vec<OutputRecord> = Vec::new();
        assert!(enrich(&mut none, Some(&RecordingDetector::default())).is_empty());
        assert_eq!(serde_json::to_string(&none).unwrap(), "[]");
    }

    #[test]
    fn enriched_output_labels_and_omits_html() {
        let mut records = vec![
            news_record("Markets rallied"),
            social_record("t3_o", Some("Ferris"), "lol cat"),
        ];
        enrich(&mut records, Some(&RecordingDetector::default()));
        let value = serde_json::to_value(&records).unwrap();
        let array = value.as_array().expect("array");
        assert_eq!(array.len(), 2);
        let kinds: Vec<&str> = array
            .iter()
            .map(|o| {
                assert!(o["platform"].is_string(), "{o}");
                o["source_type"].as_str().expect("source_type string")
            })
            .collect();
        assert_eq!(kinds, ["news", "social"]);
        let text = serde_json::to_string(&records).unwrap();
        assert!(!text.contains("\"html\""), "{text}");
    }

    #[test]
    fn social_text_is_not_truncated_in_output() {
        let text = "é".repeat(600);
        let title = "T".repeat(600);
        let mut records = vec![social_record("t3_long", Some(&title), &text)];
        let detector = RecordingDetector::default();
        enrich(&mut records, Some(&detector));
        assert_eq!(records[0].text, text);
        assert_eq!(records[0].text.chars().count(), 600);
        assert_eq!(records[0].title.as_deref(), Some(title.as_str()));
        assert_eq!(detector.calls.borrow()[0].chars().count(), 500);
    }

    // ── Cluster tests (local Fluvio, scratch topics) ─────────────────────────

    /// The README `Tech Market Surges` sample, which passes the full chain.
    const NEWS_HTML: &str = "<html><head><title>Breaking News</title></head><body><h1>Tech Market Surges</h1><p>The technology market experienced significant growth today as investors rallied around artificial intelligence stocks.</p></body></html>";

    fn nanos() -> u128 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock after epoch")
            .as_nanos()
    }

    async fn admin() -> miette::Result<fluvio::FluvioAdmin> {
        Ok(fluvio::Fluvio::connect()
            .await
            .map_err(|e| miette::miette!("connect: {e}"))?
            .admin()
            .await)
    }

    /// Create a scratch topic `filimon-test-<kind>-<nanos>-<pid>-<n>`.
    ///
    /// The cluster tests run in parallel and the clock can be coarser than a
    /// nanosecond, so the process id and a counter keep names unique.
    async fn create_scratch(admin: &fluvio::FluvioAdmin, kind: &str) -> miette::Result<String> {
        use fluvio::metadata::topic::TopicSpec;
        use std::sync::atomic::{AtomicU32, Ordering};
        static NEXT: AtomicU32 = AtomicU32::new(0);
        let topic = format!(
            "filimon-test-{kind}-{}-{}-{}",
            nanos(),
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        );
        admin
            .create(topic.clone(), false, TopicSpec::new_computed(1, 1, None))
            .await
            .map_err(|e| miette::miette!("create {topic}: {e}"))?;
        Ok(topic)
    }

    /// Delete every topic, reporting the first failure.
    async fn delete_topics(admin: &fluvio::FluvioAdmin, topics: &[String]) -> miette::Result<()> {
        use fluvio::metadata::topic::TopicSpec;
        let mut first_error = None;
        for topic in topics {
            if let Err(e) = admin.delete::<TopicSpec>(topic.clone()).await {
                first_error.get_or_insert(miette::miette!("delete {topic}: {e}"));
            }
        }
        first_error.map_or(Ok(()), Err)
    }

    /// A producer for a new topic; it can take a moment to provision.
    async fn ready_producer(
        stream: &FluvioStream,
        topic: &str,
    ) -> miette::Result<crate::streaming::TypedProducer> {
        for _ in 0..20 {
            if let Ok(p) = stream.producer(topic).await {
                return Ok(p);
            }
            tokio::time::sleep(Duration::from_millis(250)).await;
        }
        Err(miette::miette!("no producer for {topic}"))
    }

    /// Create a raw-pages and a social scratch topic, run `body`, then delete
    /// both, also when `body` fails an assertion (panics).
    async fn with_scratch_topics<F, Fut>(body: F) -> miette::Result<()>
    where
        F: FnOnce(String, String) -> Fut,
        Fut: std::future::Future<Output = miette::Result<()>>,
    {
        use futures_util::FutureExt;

        let admin = admin().await?;
        let raw = create_scratch(&admin, "raw").await?;
        let social = match create_scratch(&admin, "social").await {
            Ok(topic) => topic,
            Err(e) => {
                delete_topics(&admin, std::slice::from_ref(&raw)).await?;
                return Err(e);
            }
        };
        let result = std::panic::AssertUnwindSafe(body(raw.clone(), social.clone()))
            .catch_unwind()
            .await;
        let deleted = delete_topics(&admin, &[raw, social]).await;
        match result {
            Ok(outcome) => {
                outcome?;
                deleted
            }
            Err(panic) => std::panic::resume_unwind(panic),
        }
    }

    #[tokio::test]
    #[ignore = "needs a local Fluvio cluster with the filimon SmartModules loaded"]
    async fn news_through_chain() -> miette::Result<()> {
        with_scratch_topics(|raw, social| async move {
            let stream = FluvioStream::connect().await?;
            let url = "https://example.com/news";
            let item = SourceItem::news(url.to_string(), NEWS_HTML.to_string());

            let producer = ready_producer(&stream, &raw).await?;
            producer.send(url, &item).await?;
            producer.flush().await?;
            // The social topic stays empty, but must be provisioned to open.
            ready_producer(&stream, &social).await?;

            let topics = ConsumeTopics {
                raw_pages: &raw,
                social: &social,
            };
            let collected =
                collect_from_topics(&stream, &topics, Offset::beginning(), 1, SourceFilter::Both)
                    .await?;
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
            Ok(())
        })
        .await
    }

    #[tokio::test]
    #[ignore = "needs a local Fluvio cluster with the filimon SmartModules loaded"]
    async fn social_passthrough() -> miette::Result<()> {
        with_scratch_topics(|raw, social| async move {
            let stream = FluvioStream::connect().await?;

            let news_url = "https://example.com/news";
            let lol_url = "https://example.com/lol";
            let raw_producer = ready_producer(&stream, &raw).await?;
            raw_producer
                .send(news_url, &SourceItem::news(news_url.to_string(), NEWS_HTML.to_string()))
                .await?;
            raw_producer
                .send(
                    lol_url,
                    &SourceItem::news(
                        lol_url.to_string(),
                        "<html><body><p>lol cat</p></body></html>".to_string(),
                    ),
                )
                .await?;
            raw_producer.flush().await?;

            let item = SourceItem::social(
                "t3_lol".to_string(),
                "https://www.reddit.com/r/test/comments/lol".to_string(),
                None,
                "lol cat".to_string(),
                Some("u1".to_string()),
                Some(1_730_000_000_123),
                "reddit".to_string(),
                Some("{\"score\":3}".to_string()),
            );
            let social_producer = ready_producer(&stream, &social).await?;
            social_producer.send("t3_lol", &item).await?;
            social_producer.flush().await?;

            let topics = ConsumeTopics {
                raw_pages: &raw,
                social: &social,
            };
            let collected =
                collect_from_topics(&stream, &topics, Offset::beginning(), 3, SourceFilter::Both)
                    .await?;
            assert!(collected.diagnostics.is_empty(), "{:?}", collected.diagnostics);
            assert_eq!(collected.records.len(), 2, "{:?}", collected.records);

            // The same text produced as news is dropped by the chain (relevance 0.0).
            assert!(collected.records.iter().all(|r| r.url != lol_url));

            let news = collected
                .records
                .iter()
                .find(|r| r.source_type == SourceType::News)
                .expect("news record");
            assert_eq!(news.url, news_url);

            let social_record = collected
                .records
                .iter()
                .find(|r| r.source_type == SourceType::Social)
                .expect("social record");
            assert_eq!(social_record.id, "t3_lol");
            assert_eq!(social_record.text, "lol cat");
            assert_eq!(
                social_record.published.as_deref(),
                Some("2024-10-27T03:33:20.123Z")
            );
            assert_eq!(social_record.raw, Some(serde_json::json!({"score": 3})));
            assert!(social_record.article.is_none());
            Ok(())
        })
        .await
    }

    #[tokio::test]
    #[ignore = "needs a local Fluvio cluster with the filimon SmartModules loaded"]
    async fn missing_topic_is_named() -> miette::Result<()> {
        let stream = FluvioStream::connect().await?;
        let topic = format!("filimon-test-missing-{}", nanos());
        let Err(err) = stream.preflight(&[topic.as_str()], &[]).await else {
            panic!("preflight passed for a missing topic");
        };
        let message = err.to_string();
        assert!(message.contains(&topic), "{message}");
        assert!(message.contains("fluvio topic create"), "{message}");
        Ok(())
    }

    #[tokio::test]
    #[ignore = "needs a local Fluvio cluster with the filimon SmartModules loaded"]
    async fn poison_social_record_is_reported_without_value() -> miette::Result<()> {
        with_scratch_topics(|raw, social| async move {
            let stream = FluvioStream::connect().await?;

            let producer = ready_producer(&stream, &social).await?;
            producer
                .send("t3_poison", &poison_social("\u{1b}[2JPAGE-BODY"))
                .await?;
            producer.flush().await?;

            let topics = ConsumeTopics {
                raw_pages: &raw,
                social: &social,
            };
            // Diagnostics do not count toward the limit, so this run ends on
            // the idle timeout.
            let collected =
                collect_from_topics(&stream, &topics, Offset::beginning(), 1, SourceFilter::Social)
                    .await?;
            assert!(collected.records.is_empty(), "{:?}", collected.records);
            assert_eq!(collected.diagnostics.len(), 1, "{:?}", collected.diagnostics);
            let message = &collected.diagnostics[0];
            assert!(message.contains(social.as_str()), "{message:?}");
            assert!(message.contains("offset 0"), "{message:?}");
            assert!(!message.contains("PAGE-BODY"), "{message:?}");
            let line = diagnostic_line(message);
            assert!(!line.chars().any(char::is_control), "{line:?}");
            Ok(())
        })
        .await
    }

    #[tokio::test]
    #[ignore = "needs a local Fluvio cluster with the filimon SmartModules loaded"]
    async fn poison_news_record_is_reported_without_value() -> miette::Result<()> {
        with_scratch_topics(|raw, social| async move {
            let stream = FluvioStream::connect().await?;

            // map-preprocess rejects a JSON string with `invalid type: string`,
            // so the real hint quotes this value.
            let poison = serde_json::Value::String("a\"PAGE-BODY-SECRET".to_string());
            let producer = ready_producer(&stream, &raw).await?;
            producer.send("poison", &poison).await?;
            producer.flush().await?;
            // The social topic stays empty, but must be provisioned to open.
            ready_producer(&stream, &social).await?;

            let topics = ConsumeTopics {
                raw_pages: &raw,
                social: &social,
            };
            let collected =
                collect_from_topics(&stream, &topics, Offset::beginning(), 1, SourceFilter::News)
                    .await?;
            assert!(collected.records.is_empty(), "{:?}", collected.records);
            assert_eq!(collected.diagnostics.len(), 1, "{:?}", collected.diagnostics);
            let message = &collected.diagnostics[0];
            assert!(message.contains(raw.as_str()), "{message:?}");
            assert!(message.contains("offset 0"), "{message:?}");
            assert!(message.contains("SmartModule error"), "{message:?}");
            assert!(!message.contains("PAGE-BODY-SECRET"), "{message:?}");
            let line = diagnostic_line(message);
            assert!(!line.chars().any(char::is_control), "{line:?}");
            Ok(())
        })
        .await
    }

    #[tokio::test]
    #[ignore = "needs a local Fluvio cluster with the filimon SmartModules loaded"]
    async fn good_news_after_poison_record_is_delivered() -> miette::Result<()> {
        with_scratch_topics(|raw, social| async move {
            let stream = FluvioStream::connect().await?;

            let same_batch = "https://example.com/same-batch";
            let next_batch = "https://example.com/next-batch";
            let page = |url: &str| SourceItem::news(url.to_string(), NEWS_HTML.to_string());
            let poison = serde_json::Value::String("a\"PAGE-BODY-SECRET".to_string());
            let producer = ready_producer(&stream, &raw).await?;
            producer.send("poison", &poison).await?;
            // Usually stored in the poison record's batch (producer linger).
            producer.send(same_batch, &page(same_batch)).await?;
            producer.flush().await?;
            // Always in a later batch.
            producer.send(next_batch, &page(next_batch)).await?;
            producer.flush().await?;
            // The social topic stays empty, but must be provisioned to open.
            ready_producer(&stream, &social).await?;

            let topics = ConsumeTopics {
                raw_pages: &raw,
                social: &social,
            };
            let collected =
                collect_from_topics(&stream, &topics, Offset::beginning(), 2, SourceFilter::News)
                    .await?;
            let urls: Vec<&str> = collected.records.iter().map(|r| r.url.as_str()).collect();
            let d = &collected.diagnostics;
            assert!(urls.ends_with(&[next_batch]), "{urls:?} {d:?}");
            assert!(d[0].contains("offset 0: SmartModule error"), "{d:?}");
            if urls.len() == 2 {
                // The second record landed in its own batch.
                assert_eq!(d.len(), 1, "{d:?}");
            } else {
                assert_eq!(urls.len(), 1, "{urls:?}");
                assert_eq!(d.len(), 2, "{d:?}");
                assert!(d[1].contains("offset 1: stored in one batch"), "{d:?}");
            }
            assert!(d.iter().all(|m| m.ends_with("; skipped")), "{d:?}");
            assert!(d.iter().all(|m| !m.contains("PAGE-BODY")), "{d:?}");
            Ok(())
        })
        .await
    }
}
