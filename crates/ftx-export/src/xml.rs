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
                .and_then(|h| u32::from_str_radix(h, 16).ok())
                .or_else(|| ent.strip_prefix('#').and_then(|d| d.parse().ok()))
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

fn parse_tag(body: &str) -> Option<(String, Vec<(String, String)>)> {
    let body = body.trim();
    let name_end = body.find(char::is_whitespace).unwrap_or(body.len());
    let name = body[..name_end].to_string();
    let mut attrs = Vec::new();
    let mut rest = body[name_end..].trim_start();
    while !rest.is_empty() {
        let eq = rest.find('=')?;
        let key = rest[..eq].trim().to_string();
        let after = rest[eq + 1..].trim_start();
        let quote = after.chars().next()?;
        if quote != '"' && quote != '\'' {
            return None;
        }
        let close = after[1..].find(quote)? + 1;
        attrs.push((key, unescape(&after[1..close])));
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
        if rest.starts_with("<?") || rest.starts_with("<!") {
            rest = &rest[rest.find('>')? + 1..];
            continue;
        }
        let end = rest.find('>')?;
        let inner = &rest[1..end];
        rest = &rest[end + 1..];
        if let Some(name) = inner.strip_prefix('/') {
            let el = stack.pop()?;
            if el.name != name.trim() || stack.is_empty() {
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
    stack.pop()?.children.into_iter().next()
}
