// Stage 4: Output Sructuring & Repository Management
//
// Once content is extracted and scored, it is standardised into a machine-readable
// format (ie JSON) and written to a centralized store.
//
// In a production WISE deployment, this module would push to a MongoDB / Elasticsearch. For portability
// we write to a local JSON file and print a human-readable summary to stdout.
//
use crate::models::ExtractedArticles;
use std::fs::File;
use std::io::Write;

/// Serialize a slice of articles to a pretty-printed JSON file.
pub fn save_json(articles: &[ExtractedArticle], path: &str) -> std::io::Result<()> {
    let json = serde_json::to_string_pretty(articles)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
    let mut f = File::create(path)?;
    f.write_all(json.as_bytes())?;
    Ok(())
}

/// Print a single-article summary to stdout in a WISE pipeline log style.
pub fn print_summary(article: &ExtractedArticles) {
    let bar = "--".repeat(60);
    println!("\n{bar}");
    println!(" ■  {}", article.url);
    println!("{bar}");
    println!(" Title  : {}", article.title);
    println!(" Author : {}", article.author.as_deref().unwrap_or("--"));
    println!(
        " Date   : {}",
        article.published_date.as_deref().unwrap_or("--")
    );
    println!(" Category : {}", article.inferred_category);
    println!(
        " Relevance : {:.3} | Words: {}",
        article.relevance_score, article.word_count
    );

    if !article.top_keywords.is_empty() {
        println!(" Keywords : {}", article.top_keywords.join(", "));
    }

    if !article.named_entities.is_empty() {
        let entity_str: Vec<String> = article
            .named_entities
            .iter()
            .take(5)
            .map(|e| format!("{} [{}]", e.text, e.kind))
            .collect();
        println!(" Entities : {}", entity_str.join("  •  "));
    }

    // Body preview - word-wrap at 76 chars.
    let preview = &article.body_preview;
    let preview_short: String = preview.chars().take(200).collect();
    println!(" Preview : {}", preview_short.replace('\n', " "));
    println!("{bar}");
}

pub fn print_run_summary(total_crawled: usize, accepted: usize, output_path: &str) {
    println!("\n╔══════════════════════════════════════════╗");
    println!("║           WISE Pipeline Summary           ║");
    println!("╠══════════════════════════════════════════╣");
    println!("║  Pages crawled : {total_crawled:<25}║");
    println!("║  Articles kept : {accepted:<25}║");
    println!("║  Output file   : {output_path:<25}║");
    println!("╚══════════════════════════════════════════╝");
}
