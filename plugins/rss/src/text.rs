//! HTML-to-text and small string helpers. Feed content is untrusted: every
//! function here only shrinks or sanitises, none interprets markup as code.

/// Decode the common named and numeric character references.
fn decode_entities(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut rest = input;
    while let Some(amp) = rest.find('&') {
        out.push_str(&rest[..amp]);
        let tail = &rest[amp..];
        // An entity is `&` + at most 10 chars + `;`.
        let end = tail.char_indices().take(12).find(|(_, c)| *c == ';').map(|(i, _)| i);
        let decoded = end.and_then(|end| {
            let name = &tail[1..end];
            let c = match name {
                "amp" => Some('&'),
                "lt" => Some('<'),
                "gt" => Some('>'),
                "quot" => Some('"'),
                "apos" => Some('\''),
                "nbsp" => Some(' '),
                "hellip" => Some('…'),
                "mdash" => Some('—'),
                "ndash" => Some('–'),
                "lsquo" => Some('‘'),
                "rsquo" => Some('’'),
                "ldquo" => Some('“'),
                "rdquo" => Some('”'),
                _ => name.strip_prefix('#').and_then(|n| {
                    let code = match n.strip_prefix(['x', 'X']) {
                        Some(hex) => u32::from_str_radix(hex, 16).ok(),
                        None => n.parse::<u32>().ok(),
                    };
                    code.and_then(char::from_u32)
                }),
            };
            c.map(|c| (c, end))
        });
        match decoded {
            Some((c, end)) => {
                out.push(c);
                rest = &tail[end + 1..];
            }
            None => {
                out.push('&');
                rest = &tail[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

/// Tags whose end or start breaks a line in the plain text.
fn is_block_tag(name: &str) -> bool {
    matches!(
        name,
        "p" | "br" | "div" | "li" | "ul" | "ol" | "h1" | "h2" | "h3" | "h4" | "h5" | "h6"
            | "blockquote" | "tr" | "table" | "section" | "article" | "pre" | "hr"
    )
}

/// Turn an HTML fragment into plain text: tags are dropped, `script` and
/// `style` bodies are removed, block tags become line breaks, entities are
/// decoded and whitespace is collapsed. Control characters other than newline
/// are removed so the text can reach a terminal safely.
pub fn html_to_text(html: &str) -> String {
    let mut out = String::with_capacity(html.len());
    let mut chars = html.char_indices().peekable();
    let mut skip_until: Option<&'static str> = None;
    while let Some((i, c)) = chars.next() {
        if c != '<' {
            if skip_until.is_none() {
                out.push(c);
            }
            continue;
        }
        // Find the end of the tag; an unclosed `<` is plain text.
        let Some(close) = html[i..].find('>') else {
            if skip_until.is_none() {
                out.push(c);
            }
            continue;
        };
        let tag = &html[i + 1..i + close];
        // Advance past the tag.
        while chars.peek().is_some_and(|(j, _)| *j <= i + close) {
            chars.next();
        }
        let is_end = tag.starts_with('/');
        let name: String = tag
            .trim_start_matches('/')
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric())
            .collect::<String>()
            .to_ascii_lowercase();
        if let Some(target) = skip_until {
            if is_end && name == target {
                skip_until = None;
            }
            continue;
        }
        if !is_end && (name == "script" || name == "style") && !tag.ends_with('/') {
            skip_until = Some(if name == "script" { "script" } else { "style" });
            continue;
        }
        if is_block_tag(&name) {
            out.push('\n');
        }
    }
    let decoded = decode_entities(&out);
    collapse_whitespace(&decoded)
}

/// Collapse runs of spaces, trim each line and keep at most one blank line.
fn collapse_whitespace(input: &str) -> String {
    let mut lines: Vec<String> = Vec::new();
    for line in input.split('\n') {
        let cleaned: String = line
            .chars()
            .map(|c| if c.is_control() || c.is_whitespace() { ' ' } else { c })
            .collect();
        let cleaned = cleaned.split_whitespace().collect::<Vec<_>>().join(" ");
        if cleaned.is_empty() && lines.last().is_none_or(|l| l.is_empty()) {
            continue;
        }
        lines.push(cleaned);
    }
    while lines.last().is_some_and(|l| l.is_empty()) {
        lines.pop();
    }
    lines.join("\n")
}

/// Plain single-line text for a title or author: HTML-free, no line breaks.
pub fn plain_line(input: &str) -> String {
    html_to_text(input).split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Cut `text` to at most `max_chars` characters on a character boundary,
/// ending with an ellipsis when it was cut.
pub fn truncate_chars(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text.to_string();
    }
    let mut cut: String = text.chars().take(max_chars.saturating_sub(1)).collect();
    cut.push('…');
    cut
}

/// FNV-1a over the bytes: a stable, dependency-free 32-bit hash for cursor keys.
pub fn fnv1a(input: &str) -> u32 {
    let mut hash: u32 = 0x811c_9dc5;
    for byte in input.as_bytes() {
        hash ^= u32::from(*byte);
        hash = hash.wrapping_mul(0x0100_0193);
    }
    hash
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_tags_and_decodes_entities() {
        let html = "<p>Hello &amp; <b>welcome</b></p><p>Caf&#233; &#x2019;ok&#x2019;</p>";
        assert_eq!(html_to_text(html), "Hello & welcome\n\nCafé ’ok’");
    }

    #[test]
    fn drops_script_and_style_bodies() {
        let html = "a<script>alert('x')</script>b<style>p{}</style>c";
        assert_eq!(html_to_text(html), "abc");
    }

    #[test]
    fn unknown_entity_and_stray_angle_survive() {
        assert_eq!(html_to_text("1 < 2 &bogus; 3"), "1 < 2 &bogus; 3");
    }

    #[test]
    fn control_bytes_are_removed() {
        assert_eq!(plain_line("a\u{1b}[31mb\r\nc"), "a [31mb c");
    }

    #[test]
    fn truncation_respects_char_boundaries() {
        assert_eq!(truncate_chars("héllo wörld", 6), "héllo…");
        assert_eq!(truncate_chars("short", 10), "short");
    }
}
