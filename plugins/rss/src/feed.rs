//! RSS 2.0 and Atom 1.0 parsing into plain `Entry` values.
//!
//! One streaming pass with `quick-xml`. Element names are compared by local
//! name, so `content:encoded`, `dc:creator` and `dc:date` need no namespace
//! handling. Nothing here interprets feed content as code; the caller turns
//! HTML into text.

use quick_xml::Reader;
use quick_xml::events::{BytesStart, Event};
use time::OffsetDateTime;
use time::format_description::well_known::{Rfc2822, Rfc3339};

/// Most entries read from one feed, however large it is.
pub const MAX_ENTRIES_PER_FEED: usize = 500;

/// One feed entry, before HTML is turned into text.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Entry {
    /// `guid` / Atom `id`, when present.
    pub guid: Option<String>,
    /// The entry's link, when present.
    pub link: Option<String>,
    pub title: Option<String>,
    pub author: Option<String>,
    /// Unix epoch milliseconds.
    pub published_ms: Option<i64>,
    /// Full content (`content:encoded` / Atom `content`), HTML or text.
    pub content: Option<String>,
    /// Short summary (`description` / Atom `summary`), HTML or text.
    pub summary: Option<String>,
    pub categories: Vec<String>,
}

/// Parse an RFC 2822 or RFC 3339 date into epoch milliseconds.
pub fn parse_date(input: &str) -> Option<i64> {
    let input = input.trim();
    let parsed = OffsetDateTime::parse(input, &Rfc2822)
        .or_else(|_| OffsetDateTime::parse(input, &Rfc3339))
        .ok()?;
    let ms = parsed.unix_timestamp_nanos() / 1_000_000;
    i64::try_from(ms).ok().filter(|ms| *ms >= 0)
}

fn local_name(start: &BytesStart<'_>) -> String {
    String::from_utf8_lossy(start.local_name().as_ref()).to_ascii_lowercase()
}

fn attribute(start: &BytesStart<'_>, key: &str) -> Option<String> {
    start
        .attributes()
        .flatten()
        .find(|a| a.key.local_name().as_ref().eq_ignore_ascii_case(key.as_bytes()))
        .and_then(|a| a.unescape_value().ok().map(|v| v.into_owned()))
}

/// A flattened XML event. `Empty` elements become an `Open` and a `Close`.
enum Tok {
    Open {
        name: String,
        href: Option<String>,
        rel: Option<String>,
        term: Option<String>,
    },
    Close,
    Text(String),
}

/// Which entry field the open element feeds.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Field {
    Title,
    LinkText,
    Guid,
    Date,
    Updated,
    AuthorText,
    Summary,
    Content,
    Category,
}

fn field_for(name: &str) -> Option<Field> {
    Some(match name {
        "title" => Field::Title,
        "link" => Field::LinkText,
        "guid" | "id" => Field::Guid,
        "pubdate" | "published" | "date" => Field::Date,
        "updated" => Field::Updated,
        "creator" | "author" => Field::AuthorText,
        "description" | "summary" => Field::Summary,
        "encoded" | "content" => Field::Content,
        "category" => Field::Category,
        _ => return None,
    })
}

fn tokenize(bytes: &[u8]) -> Result<Vec<Tok>, String> {
    let mut reader = Reader::from_reader(bytes);
    let mut tokens = Vec::new();
    loop {
        let event = reader
            .read_event()
            .map_err(|_| "feed is not well-formed XML".to_string())?;
        match event {
            Event::Start(start) => tokens.push(open_tok(&start)),
            Event::Empty(start) => {
                tokens.push(open_tok(&start));
                tokens.push(Tok::Close);
            }
            Event::End(_) => tokens.push(Tok::Close),
            Event::Text(text) => {
                // An unknown entity (HTML-only ones such as `&nbsp;`) makes
                // `unescape` fail; keep the raw text then.
                let value = text
                    .unescape()
                    .map(|c| c.into_owned())
                    .unwrap_or_else(|_| String::from_utf8_lossy(&text).into_owned());
                tokens.push(Tok::Text(value));
            }
            Event::CData(data) => tokens.push(Tok::Text(
                String::from_utf8_lossy(&data.into_inner()).into_owned(),
            )),
            Event::Eof => break,
            _ => {}
        }
    }
    Ok(tokens)
}

fn open_tok(start: &BytesStart<'_>) -> Tok {
    Tok::Open {
        name: local_name(start),
        href: attribute(start, "href"),
        rel: attribute(start, "rel"),
        term: attribute(start, "term"),
    }
}

/// Parse a feed document. Error texts never repeat feed content, so they are
/// safe to log.
pub fn parse(bytes: &[u8]) -> Result<Vec<Entry>, String> {
    let tokens = tokenize(bytes)?;

    let mut entries: Vec<Entry> = Vec::new();
    // Local names of the open elements.
    let mut stack: Vec<String> = Vec::new();
    // Stack length at which the open `item` / `entry` sits.
    let mut entry_depth: Option<usize> = None;
    let mut current = Entry::default();
    let mut atom_updated: Option<i64> = None;
    let mut author_text: Option<String> = None;
    let mut author_name: Option<String> = None;
    // The field being collected, the stack length it was opened at, and its text.
    let mut open_field: Option<(Field, usize)> = None;
    let mut buf = String::new();
    let mut in_author_name = false;
    let mut name_buf = String::new();
    let mut saw_root = false;

    for tok in tokens {
        match tok {
            Tok::Open { name, href, rel, term } => {
                stack.push(name.clone());
                let depth = stack.len();
                match entry_depth {
                    None => {
                        if depth == 1 {
                            saw_root = matches!(name.as_str(), "rss" | "feed" | "rdf");
                            if !saw_root {
                                return Err("document is not an RSS or Atom feed".to_string());
                            }
                        } else if name == "item" || name == "entry" {
                            entry_depth = Some(depth);
                            current = Entry::default();
                            atom_updated = None;
                            author_text = None;
                            author_name = None;
                        }
                    }
                    Some(ed) => {
                        if depth == ed + 2 && stack[ed] == "author" && name == "name" {
                            in_author_name = true;
                            name_buf.clear();
                        } else if depth == ed + 1 && open_field.is_none() {
                            if name == "link" && href.is_some() {
                                // Atom link: keep the alternate (or unlabelled) one.
                                let wanted = rel.as_deref().is_none_or(|r| r == "alternate");
                                if wanted && current.link.is_none() {
                                    current.link = href;
                                }
                            } else if name == "category" && term.is_some() {
                                current.categories.extend(term);
                            } else if let Some(field) = field_for(&name) {
                                open_field = Some((field, depth));
                                buf.clear();
                            }
                        }
                    }
                }
            }
            Tok::Text(text) => {
                if in_author_name {
                    name_buf.push_str(&text);
                } else if let Some((field, depth)) = open_field {
                    // Atom `author` holds child elements; only its own text counts.
                    if field != Field::AuthorText || stack.len() == depth {
                        buf.push_str(&text);
                    }
                }
            }
            Tok::Close => {
                let depth = stack.len();
                if in_author_name && entry_depth.is_some_and(|ed| depth == ed + 2) {
                    let name = name_buf.trim();
                    if !name.is_empty() {
                        author_name = Some(name.to_string());
                    }
                    in_author_name = false;
                }
                if let Some((field, fd)) = open_field {
                    if depth == fd {
                        let value = buf.trim().to_string();
                        buf.clear();
                        open_field = None;
                        if !value.is_empty() {
                            match field {
                                Field::Title => current.title = Some(value),
                                Field::LinkText => {
                                    current.link.get_or_insert(value);
                                }
                                Field::Guid => {
                                    current.guid.get_or_insert(value);
                                }
                                Field::Date => {
                                    if current.published_ms.is_none() {
                                        current.published_ms = parse_date(&value);
                                    }
                                }
                                Field::Updated => atom_updated = parse_date(&value),
                                Field::AuthorText => {
                                    author_text.get_or_insert(value);
                                }
                                Field::Summary => {
                                    current.summary.get_or_insert(value);
                                }
                                Field::Content => {
                                    current.content.get_or_insert(value);
                                }
                                Field::Category => current.categories.push(value),
                            }
                        }
                    }
                }
                if entry_depth == Some(depth) {
                    current.author = author_name.take().or(author_text.take());
                    if current.published_ms.is_none() {
                        current.published_ms = atom_updated;
                    }
                    entries.push(std::mem::take(&mut current));
                    entry_depth = None;
                    if entries.len() >= MAX_ENTRIES_PER_FEED {
                        break;
                    }
                }
                stack.pop();
            }
        }
    }

    if !saw_root {
        return Err("document is not an RSS or Atom feed".to_string());
    }
    Ok(entries)
}

#[cfg(test)]
mod tests {
    use super::*;

    const RSS: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<rss version="2.0" xmlns:content="http://purl.org/rss/1.0/modules/content/"
     xmlns:dc="http://purl.org/dc/elements/1.1/" xmlns:atom="http://www.w3.org/2005/Atom">
<channel>
  <title>Site</title>
  <link>https://example.com</link>
  <atom:link href="https://example.com/feed/" rel="self" type="application/rss+xml"/>
  <item>
    <title>First &amp; best</title>
    <link>https://example.com/first/</link>
    <dc:creator><![CDATA[Ana Reyes]]></dc:creator>
    <pubDate>Tue, 10 Jun 2025 12:00:00 +0000</pubDate>
    <category><![CDATA[News]]></category>
    <guid isPermaLink="false">https://example.com/?p=1</guid>
    <description><![CDATA[<p>Short summary</p>]]></description>
    <content:encoded><![CDATA[<p>The <b>full</b> article.</p>]]></content:encoded>
  </item>
  <item>
    <title>Second</title>
    <link>https://example.com/second/</link>
    <pubDate>Wed, 11 Jun 2025 08:30:00 GMT</pubDate>
    <description>Plain &lt;i&gt;summary&lt;/i&gt;</description>
  </item>
</channel></rss>"#;

    const ATOM: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<feed xmlns="http://www.w3.org/2005/Atom">
  <title>Blog</title>
  <link href="https://blog.example/feed" rel="self"/>
  <entry>
    <title>Atom post</title>
    <link rel="alternate" href="https://blog.example/post-1"/>
    <link rel="replies" href="https://blog.example/post-1/comments"/>
    <id>tag:blog.example,2025:1</id>
    <updated>2025-06-12T09:00:00+08:00</updated>
    <author><name>Ben Cruz</name><email>ben@blog.example</email></author>
    <category term="tech"/>
    <summary>Sum</summary>
    <content type="html">&lt;p&gt;Body&lt;/p&gt;</content>
  </entry>
</feed>"#;

    #[test]
    fn parses_rss_two_items() {
        let entries = parse(RSS.as_bytes()).unwrap();
        assert_eq!(entries.len(), 2);
        let first = &entries[0];
        assert_eq!(first.title.as_deref(), Some("First & best"));
        assert_eq!(first.link.as_deref(), Some("https://example.com/first/"));
        assert_eq!(first.guid.as_deref(), Some("https://example.com/?p=1"));
        assert_eq!(first.author.as_deref(), Some("Ana Reyes"));
        assert_eq!(first.categories, vec!["News"]);
        assert_eq!(first.content.as_deref(), Some("<p>The <b>full</b> article.</p>"));
        assert_eq!(first.summary.as_deref(), Some("<p>Short summary</p>"));
        // 2025-06-10T12:00:00Z
        assert_eq!(first.published_ms, Some(1_749_556_800_000));
        // The second item's GMT date parses too and the channel-level atom:link is ignored.
        assert_eq!(entries[1].published_ms, Some(1_749_630_600_000));
        assert_eq!(entries[1].link.as_deref(), Some("https://example.com/second/"));
    }

    #[test]
    fn parses_atom_entry() {
        let entries = parse(ATOM.as_bytes()).unwrap();
        assert_eq!(entries.len(), 1);
        let e = &entries[0];
        assert_eq!(e.link.as_deref(), Some("https://blog.example/post-1"));
        assert_eq!(e.guid.as_deref(), Some("tag:blog.example,2025:1"));
        assert_eq!(e.author.as_deref(), Some("Ben Cruz"));
        assert_eq!(e.categories, vec!["tech"]);
        assert_eq!(e.content.as_deref(), Some("<p>Body</p>"));
        // `updated` is the fallback date: 2025-06-12T01:00:00Z
        assert_eq!(e.published_ms, Some(1_749_690_000_000));
    }

    #[test]
    fn html_error_page_is_not_a_feed() {
        let err = parse(b"<!DOCTYPE html><html><body>Not found</body></html>").unwrap_err();
        assert!(err.contains("not an RSS or Atom feed"), "{err}");
    }

    #[test]
    fn malformed_xml_error_carries_no_content() {
        let err = parse(b"<rss><channel><item><title>secret</item></channel></rss>").unwrap_err();
        assert!(!err.contains("secret"), "{err}");
    }

    #[test]
    fn entry_count_is_capped() {
        let mut doc = String::from("<rss><channel>");
        for i in 0..(MAX_ENTRIES_PER_FEED + 50) {
            doc.push_str(&format!("<item><guid>g{i}</guid></item>"));
        }
        doc.push_str("</channel></rss>");
        assert_eq!(parse(doc.as_bytes()).unwrap().len(), MAX_ENTRIES_PER_FEED);
    }

    #[test]
    fn date_formats() {
        assert_eq!(parse_date("Tue, 10 Jun 2025 12:00:00 +0000"), Some(1_749_556_800_000));
        assert_eq!(parse_date("2025-06-10T12:00:00Z"), Some(1_749_556_800_000));
        assert_eq!(parse_date("not a date"), None);
        assert_eq!(parse_date("1960-01-01T00:00:00Z"), None);
    }
}
