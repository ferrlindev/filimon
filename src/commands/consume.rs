// ─────────────────────────────────────────────────────────────────────────────
// commands/consume.rs - Consume mode
//
// Consumes processed articles from Fluvio with optional NER enrichment.
// ─────────────────────────────────────────────────────────────────────────────

use fluvio::Offset;

use crate::models;
use crate::output;
use crate::scorer::NerEngine;
use crate::streaming::{FluvioStream, TypedConsumer, TOPIC_ARTICLES};

/// Consume processed articles from Fluvio.
///
/// # Arguments
/// * `from_beginning` - Start consuming from the beginning of the topic
/// * `limit` - Maximum articles to consume (0 = unlimited streaming)
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

    let consumer: TypedConsumer<models::ExtractedArticle> =
        stream.consumer(TOPIC_ARTICLES, offset).await?;

    println!("[Consume] Listening on topic: {}", TOPIC_ARTICLES);

    if limit > 0 {
        consume_batch(&consumer, limit, ner).await?;
    } else {
        consume_stream(&consumer).await?;
    }

    Ok(())
}

/// Consume a batch of articles with optional NER enrichment.
async fn consume_batch(
    consumer: &TypedConsumer<models::ExtractedArticle>,
    limit: usize,
    ner: Option<&NerEngine>,
) -> miette::Result<()> {
    println!("[Consume] Will consume up to {} articles.\n", limit);
    let articles = consumer.consume_batch(limit).await?;

    for mut article in articles {
        // Optionally enrich with NER
        if let Some(engine) = ner {
            if let Ok(entities) = engine.detect(&article.body_preview) {
                article.named_entities = entities;
            }
        }
        output::print_summary(&article);
    }

    output::print_run_summary(limit, TOPIC_ARTICLES);
    Ok(())
}

/// Consume articles in streaming mode (unlimited).
async fn consume_stream(
    consumer: &TypedConsumer<models::ExtractedArticle>,
) -> miette::Result<()> {
    println!("[Consume] Press Ctrl+C to stop.\n");

    consumer
        .process(|article: models::ExtractedArticle| {
            // Note: NER enrichment not available in streaming mode due to async closure constraints
            // Use --limit mode for NER enrichment
            async move {
                output::print_summary(&article);
                Ok(())
            }
        })
        .await?;

    Ok(())
}
