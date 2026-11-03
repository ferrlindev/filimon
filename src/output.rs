// Stage 4: Output Sructuring & Repository Management
//
// Once content is extracted and scored, it is standardised into a machine-readable
// format (ie JSON) and written to a centralized store.
//
// In a production WISE deployment, this module would push to a MongoDB / Elasticsearch. For portability
// we write to a local JSON file and print a human-readable summary to stdout.
//
use crate::models::{OutputRecord, SourceType};
use std::fs::File;
use std::io::Write;
use std::path::Path;

/// Serialize records to a pretty-printed JSON array, creating parent directories.
pub fn save_json(records: &[OutputRecord], path: &Path) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)?;
        }
    }
    let json = serde_json::to_string_pretty(records).map_err(std::io::Error::other)?;
    let mut f = File::create(path)?;
    f.write_all(json.as_bytes())?;
    Ok(())
}

/// Print a single-record summary to stdout in a WISE pipeline log style.
pub fn print_summary(record: &OutputRecord) {
    let bar = "--".repeat(60);
    let kind = match record.source_type {
        SourceType::News => "news",
        SourceType::Social => "social",
    };
    println!("\n{bar}");
    println!(" ■  [{kind}] {}  {}", record.platform, record.url);
    println!("{bar}");
    println!(" Title  : {}", record.title.as_deref().unwrap_or("--"));
    println!(" Author : {}", record.author.as_deref().unwrap_or("--"));
    println!(" Published : {}", record.published.as_deref().unwrap_or("--"));

    if let Some(article) = &record.article {
        println!(" Category : {}", article.inferred_category);
        println!(
            " Relevance : {:.3} | Words: {}",
            article.relevance_score, article.word_count
        );
        if !article.top_keywords.is_empty() {
            println!(" Keywords : {}", article.top_keywords.join(", "));
        }
    }

    if !record.entities.is_empty() {
        let entity_str: Vec<String> = record
            .entities
            .iter()
            .take(5)
            .map(|e| format!("{} [{}]", e.text, e.kind))
            .collect();
        println!(" Entities : {}", entity_str.join("  •  "));
    }

    let preview_short: String = record.text.chars().take(200).collect();
    println!(" Preview : {}", preview_short.replace('\n', " "));
    println!("{bar}");
}

pub fn print_run_summary(accepted: usize, output_path: &str) {
    println!("\n╔══════════════════════════════════════════╗");
    println!("║           WISE Pipeline Summary           ║");
    println!("╠══════════════════════════════════════════╣");
    println!("║  Records kept  : {accepted:<25}║");
    println!("║  Output file   : {output_path:<25}║");
    println!("╚══════════════════════════════════════════╝");
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::ExtractedArticle;
    use tempfile::TempDir;

    fn sample() -> OutputRecord {
        OutputRecord::from_article(ExtractedArticle {
            url: "https://example.com/a".to_string(),
            title: "Title".to_string(),
            author: None,
            published_date: None,
            body_preview: "body".to_string(),
            word_count: 1,
            relevance_score: 0.5,
            top_keywords: vec!["market".to_string()],
            named_entities: vec![],
            inferred_category: "Business".to_string(),
        })
    }

    #[test]
    fn save_json_writes_array_without_html() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("a/b/out.json");
        save_json(&[sample()], &path).unwrap();

        let text = std::fs::read_to_string(&path).unwrap();
        let value: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(value.as_array().unwrap().len(), 1);
        assert!(!text.contains("\"html\""), "{text}");
    }

    #[test]
    fn save_json_empty_slice_writes_empty_array() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("out.json");
        save_json(&[], &path).unwrap();

        let value: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert!(value.as_array().unwrap().is_empty());
    }
}
