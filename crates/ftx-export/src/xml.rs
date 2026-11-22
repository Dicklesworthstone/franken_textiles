//! Just enough XML for the acceptance checks: elements with their attributes (entities resolved) and their
//! children; text, comments, processing instructions and doctypes are skipped.

/// An element.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Element {
    /// Tag name (with any prefix).
    pub name: String,
    /// Attributes in document order.
    pub attrs: Vec<(String, String)>,
    /// Child elements.
    pub children: Vec<Element>,
}

impl Element {
    /// An attribute's value.
    pub(crate) fn get(&self, key: &str) -> Option<&str> {
        self.attrs
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.as_str())
    }

    /// Every element below this one, depth first.
    pub(crate) fn descendants(&self) -> Vec<&Element> {
        let mut out = Vec::new();
        for c in &self.children {
            out.push(c);
            out.extend(c.descendants());
        }
        out
    }
}

/// Maximum element nesting accepted by [`parse`], bounding recursive traversal in [`Element::descendants`].
const MAX_XML_DEPTH: usize = 256;

fn unescape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find('&') {
        out.push_str(&rest[..i]);
        let tail = &rest[i..];
        let Some(j) = tail.find(';') else {
            out.push_str(tail);
            return out;
        };
        let ent = &tail[1..j];
        let ch = match ent {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            _ => ent
                .strip_prefix("#x")
                .filter(|h| !h.is_empty() && h.bytes().all(|b| b.is_ascii_hexdigit()))
                .and_then(|h| u32::from_str_radix(h, 16).ok())
                .or_else(|| {
                    ent.strip_prefix('#')
                        .filter(|d| !d.is_empty() && d.bytes().all(|b| b.is_ascii_digit()))
                        .and_then(|d| d.parse().ok())
                })
                .and_then(char::from_u32),
        };
        match ch {
            Some(c) => out.push(c),
            None => out.push_str(&tail[..=j]),
        }
        rest = &tail[j + 1..];
    }
    out.push_str(rest);
    out
}

fn valid_xml_name(name: &str) -> bool {
    !name.is_empty()
        && !name
            .chars()
            .any(|c| c.is_whitespace() || matches!(c, '<' | '>' | '"' | '\'' | '=' | '/'))
}

fn find_tag_end(s: &str) -> Option<usize> {
    let mut quote: Option<u8> = None;
    for (i, b) in s.bytes().enumerate() {
        match quote {
            Some(q) if b == q => quote = None,
            Some(_) => {}
            None if b == b'"' || b == b'\'' => quote = Some(b),
            None if b == b'>' => return Some(i),
            None => {}
        }
    }
    None
}

fn parse_tag(body: &str) -> Option<(String, Vec<(String, String)>)> {
    let body = body.trim();
    let name_end = body.find(char::is_whitespace).unwrap_or(body.len());
    let name = &body[..name_end];
    if !valid_xml_name(name) {
        return None;
    }
    let name = name.to_string();
    let mut attrs = Vec::new();
    let mut rest = body[name_end..].trim_start();
    while !rest.is_empty() {
        let eq = rest.find('=')?;
        let key = rest[..eq].trim();
        if !valid_xml_name(key) {
            return None;
        }
        let after = rest[eq + 1..].trim_start();
        let quote = after.chars().next()?;
        if quote != '"' && quote != '\'' {
            return None;
        }
        let close = after[1..].find(quote)? + 1;
        attrs.push((key.to_string(), unescape(&after[1..close])));
        rest = after[close + 1..].trim_start();
    }
    Some((name, attrs))
}

/// The root element of a document, or `None` when it is not well formed enough to read.
pub(crate) fn parse(text: &str) -> Option<Element> {
    let mut stack: Vec<Element> = vec![Element::default()];
    let mut rest = text;
    while let Some(i) = rest.find('<') {
        rest = &rest[i..];
        if let Some(r) = rest.strip_prefix("<!--") {
            rest = &r[r.find("-->")? + 3..];
            continue;
        }
        if let Some(r) = rest.strip_prefix("<![CDATA[") {
            rest = &r[r.find("]]>")? + 3..];
            continue;
        }
        if let Some(r) = rest.strip_prefix("<?") {
            rest = &r[r.find("?>")? + 2..];
            continue;
        }
        if rest.starts_with("<!") {
            rest = &rest[rest.find('>')? + 1..];
            continue;
        }
        let end = find_tag_end(&rest[1..])? + 1;
        let inner = &rest[1..end];
        rest = &rest[end + 1..];
        if let Some(name) = inner.strip_prefix('/') {
            let close_name = name.trim();
            if !valid_xml_name(close_name) {
                return None;
            }
            let el = stack.pop()?;
            if el.name != close_name || stack.is_empty() {
                return None;
            }
            stack.last_mut()?.children.push(el);
        } else if let Some(body) = inner.strip_suffix('/') {
            let (name, attrs) = parse_tag(body)?;
            stack.last_mut()?.children.push(Element {
                name,
                attrs,
                children: Vec::new(),
            });
        } else {
            if stack.len() > MAX_XML_DEPTH {
                return None;
            }
            let (name, attrs) = parse_tag(inner)?;
            stack.push(Element {
                name,
                attrs,
                children: Vec::new(),
            });
        }
    }
    if stack.len() != 1 {
        return None;
    }
    let mut roots = stack.pop()?.children;
    if roots.len() != 1 {
        return None;
    }
    roots.pop()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_quoted_gt_entities_and_rejects_malformed_xml() {
        let doc = r#"<?xml version="1.0"?><!-- c --><svg width="10mm" data-note="a > b &amp; &#65; &#x42;"><g id="p1"/></svg>"#;
        let root = parse(doc).expect("valid svg");
        assert_eq!(root.name, "svg");
        assert_eq!(root.get("data-note"), Some("a > b & A B"));
        assert_eq!(root.descendants().len(), 1);

        // Signed numeric entities are not valid XML character references and must stay unexpanded.
        let root = parse(r#"<svg a="&#+65;" b="&#x+41;"/>"#).unwrap();
        assert_eq!(root.get("a"), Some("&#+65;"));
        assert_eq!(root.get("b"), Some("&#x+41;"));

        // Malformed tags, multiple roots, unclosed quotes, and excessive depth must be refused.
        assert!(parse("<>").is_none());
        assert!(parse("< />").is_none());
        assert!(parse("<a =\"1\"/>").is_none());
        assert!(parse("<a x y=\"1\"/>").is_none());
        assert!(parse("<svg/><extra/>").is_none());
        assert!(parse("<svg width=\"10mm/>").is_none());

        let deep = format!(
            "{}{}",
            "<g>".repeat(MAX_XML_DEPTH + 1),
            "</g>".repeat(MAX_XML_DEPTH + 1)
        );
        assert!(parse(&deep).is_none());
    }
}
