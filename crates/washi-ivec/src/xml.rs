// SPDX-License-Identifier: GPL-2.0-or-later
//! A small owned element tree on top of quick-xml, plus escape / pretty-print / redact helpers.
//!
//! Element and attribute names are reduced to their local part (the `ivec:` / `vcn:` prefix is
//! dropped), which keeps the typed parsers independent of the prefixes a firmware picks.

use quick_xml::Reader;
use quick_xml::events::{BytesText, Event};
use quick_xml::{Writer, XmlVersion};

use crate::error::IvecError;

const MAX_DEPTH: usize = 64;

/// Escape a value for use as XML text or as a double-quoted attribute value.
pub fn escape(raw: &str) -> String {
    quick_xml::escape::escape(raw).into_owned()
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Node {
    /// Local name (prefix stripped).
    pub name: String,
    pub attrs: Vec<(String, String)>,
    /// Concatenated character data (CDATA included), trimmed.
    pub text: String,
    pub children: Vec<Node>,
}

fn xml_err(e: impl std::fmt::Display) -> IvecError {
    IvecError::Xml(e.to_string())
}

fn local(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

fn start_node(e: &quick_xml::events::BytesStart<'_>) -> Result<Node, IvecError> {
    let mut node = Node { name: local(e.local_name().as_ref()), ..Node::default() };
    for attr in e.attributes() {
        let attr = attr.map_err(xml_err)?;
        let key = attr.key;
        let raw = key.as_ref();
        if raw == b"xmlns" || raw.starts_with(b"xmlns:") {
            continue;
        }
        let value = attr.normalized_value(XmlVersion::Implicit1_0).map_err(xml_err)?.into_owned();
        node.attrs.push((local(key.local_name().as_ref()), value));
    }
    Ok(node)
}

impl Node {
    /// Parse a document and return its root element.
    pub fn parse(xml: &str) -> Result<Node, IvecError> {
        let mut reader = Reader::from_str(xml);
        let mut stack: Vec<Node> = Vec::new();
        let mut root: Option<Node> = None;
        loop {
            match reader.read_event().map_err(xml_err)? {
                Event::Start(e) => {
                    if stack.len() >= MAX_DEPTH {
                        return Err(IvecError::Xml("element nesting too deep".into()));
                    }
                    stack.push(start_node(&e)?);
                }
                Event::Empty(e) => {
                    let node = start_node(&e)?;
                    match stack.last_mut() {
                        Some(parent) => parent.children.push(node),
                        None => root = Some(node),
                    }
                }
                Event::End(_) => {
                    let mut node =
                        stack.pop().ok_or_else(|| IvecError::Xml("unbalanced end tag".into()))?;
                    node.text = node.text.trim().to_string();
                    match stack.last_mut() {
                        Some(parent) => parent.children.push(node),
                        None => root = Some(node),
                    }
                }
                Event::Text(t) => {
                    if let Some(top) = stack.last_mut() {
                        top.text.push_str(&t.decode().map_err(xml_err)?);
                    }
                }
                Event::CData(t) => {
                    if let Some(top) = stack.last_mut() {
                        top.text.push_str(&t.decode().map_err(xml_err)?);
                    }
                }
                Event::GeneralRef(r) => {
                    if let Some(top) = stack.last_mut() {
                        if let Some(ch) = r.resolve_char_ref().map_err(xml_err)? {
                            top.text.push(ch);
                        } else {
                            let name = r.decode().map_err(xml_err)?;
                            if let Some(s) = quick_xml::escape::resolve_predefined_entity(&name) {
                                top.text.push_str(s);
                            }
                        }
                    }
                }
                Event::Eof => break,
                _ => {}
            }
        }
        if !stack.is_empty() {
            return Err(IvecError::Xml("unexpected end of document".into()));
        }
        root.ok_or_else(|| IvecError::Xml("empty document".into()))
    }

    pub fn child(&self, name: &str) -> Option<&Node> {
        self.children.iter().find(|c| c.name == name)
    }

    pub fn children_named<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a Node> + 'a {
        self.children.iter().filter(move |c| c.name == name)
    }

    /// Walk a chain of first-matching children.
    pub fn find(&self, path: &[&str]) -> Option<&Node> {
        let mut cur = self;
        for name in path {
            cur = cur.child(name)?;
        }
        Some(cur)
    }

    /// Trimmed text of the first child called `name` (`Some("")` for an empty element).
    pub fn child_text(&self, name: &str) -> Option<&str> {
        self.child(name).map(|c| c.text.as_str())
    }

    pub fn attr(&self, name: &str) -> Option<&str> {
        self.attrs.iter().find(|(k, _)| k == name).map(|(_, v)| v.as_str())
    }

    /// No text, no children and no attributes.
    pub fn is_blank(&self) -> bool {
        self.text.is_empty() && self.children.is_empty() && self.attrs.is_empty()
    }

    /// Leaf paths and values below this node, `a/b/c` style, attributes as `a/b[attr=val]`.
    pub fn flatten(&self) -> Vec<(String, String)> {
        let mut out = Vec::new();
        for child in &self.children {
            child.flatten_into("", &mut out);
        }
        out
    }

    fn flatten_into(&self, prefix: &str, out: &mut Vec<(String, String)>) {
        let mut path =
            if prefix.is_empty() { self.name.clone() } else { format!("{prefix}/{}", self.name) };
        if !self.attrs.is_empty() {
            let attrs: Vec<String> = self.attrs.iter().map(|(k, v)| format!("{k}={v}")).collect();
            path.push_str(&format!("[{}]", attrs.join(",")));
        }
        if self.children.is_empty() {
            out.push((path, self.text.clone()));
        } else {
            for child in &self.children {
                child.flatten_into(&path, out);
            }
        }
    }
}

/// Re-indent a document for display. Whitespace is added; content is unchanged.
pub fn pretty(xml: &str) -> Result<String, IvecError> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);
    let mut writer = Writer::new_with_indent(Vec::new(), b' ', 2);
    loop {
        match reader.read_event().map_err(xml_err)? {
            Event::Eof => break,
            ev => writer.write_event(ev).map_err(xml_err)?,
        }
    }
    String::from_utf8(writer.into_inner()).map_err(xml_err)
}

/// Element names (local) whose content identifies a specific unit.
fn is_identifier(name: &str) -> bool {
    matches!(name, "serialnumber" | "product_serialnumber" | "hwaddress" | "redirect_url" | "snma")
        || name.starts_with("gci")
}

/// Replace the content of identifier-bearing elements (serial numbers, hardware addresses,
/// cloud redirect URLs, `gci*` blocks) with `REDACTED`, leaving everything else byte-for-byte.
pub fn redact(xml: &str) -> Result<String, IvecError> {
    let mut reader = Reader::from_str(xml);
    let mut writer = Writer::new(Vec::new());
    // Depth of the redacted element currently being skipped, if any.
    let mut skipping: Option<usize> = None;
    let mut depth = 0usize;
    loop {
        let ev = reader.read_event().map_err(xml_err)?;
        match &ev {
            Event::Eof => break,
            Event::Start(e) => {
                depth += 1;
                writer.write_event(ev.borrow()).map_err(xml_err)?;
                if skipping.is_none() && is_identifier(&local(e.local_name().as_ref())) {
                    skipping = Some(depth);
                    writer.write_event(Event::Text(BytesText::new("REDACTED"))).map_err(xml_err)?;
                }
            }
            Event::End(_) => {
                if skipping == Some(depth) {
                    skipping = None;
                }
                depth = depth.saturating_sub(1);
                writer.write_event(ev.borrow()).map_err(xml_err)?;
            }
            Event::Text(_) | Event::CData(_) | Event::GeneralRef(_) if skipping.is_some() => {}
            _ => writer.write_event(ev.borrow()).map_err(xml_err)?,
        }
    }
    String::from_utf8(writer.into_inner()).map_err(xml_err)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_and_strips_prefixes() {
        let n = Node::parse(
            r#"<a:x xmlns:a="u"><a:y k="1 &amp; 2">t &lt; u</a:y><a:z/><a:c><![CDATA[<raw>]]></a:c></a:x>"#,
        )
        .unwrap();
        assert_eq!(n.name, "x");
        assert_eq!(n.child("y").unwrap().attr("k"), Some("1 & 2"));
        assert_eq!(n.child_text("y"), Some("t < u"));
        assert!(n.child("z").unwrap().is_blank());
        assert_eq!(n.child_text("c"), Some("<raw>"));
    }

    #[test]
    fn rejects_garbage() {
        assert!(Node::parse("").is_err());
        assert!(Node::parse("<a><b></a>").is_err());
    }

    #[test]
    fn escape_covers_all_specials() {
        assert_eq!(escape(r#"<&>"'"#), "&lt;&amp;&gt;&quot;&apos;");
    }

    #[test]
    fn redact_blanks_identifiers_only() {
        let src = "<c><i:serialnumber>SECRET1</i:serialnumber><i:model>M</i:model>\
                   <v:gci><v:gcia><![CDATA[SECRET2]]></v:gcia></v:gci>\
                   <i:hwaddress>aa:bb</i:hwaddress><v:redirect_url>http://x/?snma=SECRET3</v:redirect_url></c>";
        let out = redact(src).unwrap();
        assert!(!out.contains("SECRET"), "{out}");
        assert!(!out.contains("aa:bb"));
        assert!(out.contains("<i:model>M</i:model>"));
        assert_eq!(out.matches("REDACTED").count(), 4);
    }

    #[test]
    fn pretty_indents() {
        let p = pretty("<a><b>1</b><c/></a>").unwrap();
        assert!(p.contains("\n  <b>1</b>"), "{p}");
    }
}
