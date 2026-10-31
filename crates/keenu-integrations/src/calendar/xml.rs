// SPDX-License-Identifier: GPL-3.0-or-later

//! Just enough XML for WebDAV: a tree of elements by local name. Namespaces are ignored since no two
//! elements Keenu reads share a local name.

use quick_xml::Reader;
use quick_xml::escape::resolve_xml_entity;
use quick_xml::events::{BytesRef, Event};

/// Deeper than any WebDAV answer. Walking and freeing the tree recurse, so without a limit a hostile
/// server's nesting could overflow the stack, which ends the program rather than failing a sync.
const MAX_DEPTH: usize = 64;

/// One element.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Node {
    pub name: String,
    pub text: String,
    pub children: Vec<Node>,
}

impl Node {
    /// Every descendant with this local name, depth first.
    pub fn descendants<'a>(&'a self, name: &str) -> Vec<&'a Node> {
        let mut found = Vec::new();
        for child in &self.children {
            if child.name == name {
                found.push(child);
            }
            found.extend(child.descendants(name));
        }
        found
    }

    /// The text of the first descendant with this local name.
    pub fn find_text(&self, name: &str) -> Option<&str> {
        self.descendants(name)
            .first()
            .map(|node| node.text.as_str())
    }

    pub fn has(&self, name: &str) -> bool {
        !self.descendants(name).is_empty()
    }
}

/// Parses `text`, returning its root. External entities are not expanded: quick-xml has no DTD support,
/// so a hostile server's XML cannot read files or call out.
pub fn parse(text: &str) -> Result<Node, String> {
    let mut reader = Reader::from_str(text);
    let mut stack: Vec<Node> = Vec::new();
    let mut root = None;
    loop {
        match reader.read_event().map_err(|err| err.to_string())? {
            Event::Start(start) => {
                if stack.len() == MAX_DEPTH {
                    return Err("the document is nested too deeply".into());
                }
                stack.push(Node {
                    name: start.local_name().into_inner().to_owned(),
                    ..Node::default()
                });
            }
            Event::Empty(start) => {
                let node = Node {
                    name: start.local_name().into_inner().to_owned(),
                    ..Node::default()
                };
                match stack.last_mut() {
                    Some(parent) => parent.children.push(node),
                    None => root = Some(node),
                }
            }
            Event::Text(text) => {
                if let Some(node) = stack.last_mut() {
                    node.text.push_str(&text);
                }
            }
            Event::GeneralRef(entity) => {
                if let Some(node) = stack.last_mut() {
                    push_entity(&mut node.text, &entity)?;
                }
            }
            Event::CData(data) => {
                if let Some(node) = stack.last_mut() {
                    node.text.push_str(&data);
                }
            }
            Event::End(_) => {
                let Some(node) = stack.pop() else {
                    return Err("an element closes that never opened".into());
                };
                match stack.last_mut() {
                    Some(parent) => parent.children.push(node),
                    None => root = Some(node),
                }
            }
            Event::Eof => break,
            _ => {}
        }
    }
    if !stack.is_empty() {
        return Err("the document ends inside an element".into());
    }
    root.ok_or_else(|| "the document is empty".to_owned())
}

/// Adds what `&entity;` stands for: a character by number, or one of XML's five named ones. There are no others,
/// as there are no DTDs to define them.
fn push_entity(text: &mut String, entity: &BytesRef) -> Result<(), String> {
    if let Some(character) = entity.resolve_char_ref().map_err(|err| err.to_string())? {
        text.push(character);
        return Ok(());
    }
    let name: &str = entity;
    match resolve_xml_entity(name) {
        Some(value) => text.push_str(value),
        None => return Err(format!("the document uses an unknown entity, {name}")),
    }
    Ok(())
}

/// Escapes text for an XML element.
pub fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_elements_by_local_name_with_text_and_cdata() {
        let root = parse(
            r#"<?xml version="1.0"?><D:multistatus xmlns:D="DAV:"><D:response>
               <D:href>/cal/a&amp;b.ics</D:href><C:data xmlns:C="x"><![CDATA[BEGIN:VEVENT]]></C:data><D:empty/>
               </D:response></D:multistatus>"#,
        )
        .unwrap();
        assert_eq!(root.name, "multistatus");
        let response = root.children.first().unwrap();
        assert_eq!(response.find_text("href"), Some("/cal/a&b.ics"));
        assert_eq!(response.find_text("data"), Some("BEGIN:VEVENT"));
        assert!(response.has("empty"));
    }

    #[test]
    fn damaged_documents_are_errors() {
        assert!(parse("<a><b></a>").is_err());
        assert!(parse("<a>").is_err());
        assert!(parse("").is_err());
    }

    #[test]
    fn entities_become_what_they_stand_for() {
        let root =
            parse("<a>1 &lt; 2 &amp;&#32;&#x41;&apos;&quot;&gt;<![CDATA[&amp;]]></a>").unwrap();
        assert_eq!(root.text, "1 < 2 & A'\">&amp;");
        assert!(parse("<a>&nothing;</a>").is_err());
        assert!(parse("<a>&#0;</a>").is_err());
    }

    #[test]
    fn line_ends_in_calendar_data_are_kept() {
        let root = parse(
            "<a>BEGIN:VEVENT
END:VEVENT
</a>",
        )
        .unwrap();
        assert_eq!(
            root.text,
            "BEGIN:VEVENT
END:VEVENT
"
        );
    }

    #[test]
    fn a_document_defining_entities_gains_nothing() {
        let bomb = r#"<!DOCTYPE a [<!ENTITY x SYSTEM "file:///etc/passwd">]><a>&x;</a>"#;
        assert!(parse(bomb).is_err());
    }

    #[test]
    fn nesting_is_limited_rather_than_overflowing_the_stack() {
        let fits = format!("{}{}", "<a>".repeat(MAX_DEPTH), "</a>".repeat(MAX_DEPTH));
        assert!(parse(&fits).is_ok());
        let hostile = format!("{}{}", "<a>".repeat(2_000_000), "</a>".repeat(2_000_000));
        assert_eq!(
            parse(&hostile).unwrap_err(),
            "the document is nested too deeply"
        );
    }
}
