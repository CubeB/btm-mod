//! A small ordered JSON tree. It keeps fields in document order, rejects duplicate keys
//! (naming their JSON path) and keeps integers exact, so the manifest parser can report
//! precise errors. It also writes JSON back out with two-space indentation.

use std::fmt::{self, Write as _};

use serde::de::{self, DeserializeSeed, MapAccess, SeqAccess, Visitor};

/// One JSON value.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Node {
    Null,
    Bool(bool),
    /// Any integer JSON number (i64 and u64 both fit).
    Int(i128),
    /// A number with a fraction or exponent.
    Float(f64),
    Str(String),
    Arr(Vec<Node>),
    /// Fields in document order, no duplicate keys.
    Obj(Vec<(String, Node)>),
}

impl Node {
    /// The JSON type name, for "expected X, found Y" messages.
    pub(crate) fn type_name(&self) -> &'static str {
        match self {
            Node::Null => "null",
            Node::Bool(_) => "a boolean",
            Node::Int(_) | Node::Float(_) => "a number",
            Node::Str(_) => "a string",
            Node::Arr(_) => "an array",
            Node::Obj(_) => "an object",
        }
    }
}

/// Appends `.key` (identifier-like keys) or `["key"]` to a JSON path.
pub(crate) fn child(path: &str, key: &str) -> String {
    let plain = key
        .chars()
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && key.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
    if plain {
        format!("{path}.{key}")
    } else {
        format!("{path}[{}]", quote(key))
    }
}

/// Appends `[i]` to a JSON path.
pub(crate) fn index(path: &str, i: usize) -> String {
    format!("{path}[{i}]")
}

/// A JSON string literal.
pub(crate) fn quote(s: &str) -> String {
    serde_json::to_string(s).unwrap_or_else(|_| format!("\"{s}\""))
}

/// Parses JSON text into a [`Node`]. Comments, trailing commas, trailing data and
/// duplicate keys are all errors.
pub(crate) fn parse(text: &str) -> Result<Node, serde_json::Error> {
    let mut de = serde_json::Deserializer::from_str(text);
    let node = NodeSeed { path: "$".into() }.deserialize(&mut de)?;
    de.end()?;
    Ok(node)
}

struct NodeSeed {
    path: String,
}

impl<'de> DeserializeSeed<'de> for NodeSeed {
    type Value = Node;

    fn deserialize<D: de::Deserializer<'de>>(self, d: D) -> Result<Node, D::Error> {
        d.deserialize_any(NodeVisitor { path: self.path })
    }
}

struct NodeVisitor {
    path: String,
}

impl<'de> Visitor<'de> for NodeVisitor {
    type Value = Node;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("a JSON value")
    }

    fn visit_unit<E>(self) -> Result<Node, E> {
        Ok(Node::Null)
    }

    fn visit_bool<E>(self, v: bool) -> Result<Node, E> {
        Ok(Node::Bool(v))
    }

    fn visit_i64<E>(self, v: i64) -> Result<Node, E> {
        Ok(Node::Int(i128::from(v)))
    }

    fn visit_u64<E>(self, v: u64) -> Result<Node, E> {
        Ok(Node::Int(i128::from(v)))
    }

    fn visit_f64<E>(self, v: f64) -> Result<Node, E> {
        Ok(Node::Float(v))
    }

    fn visit_str<E>(self, v: &str) -> Result<Node, E> {
        Ok(Node::Str(v.to_owned()))
    }

    fn visit_string<E>(self, v: String) -> Result<Node, E> {
        Ok(Node::Str(v))
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Node, A::Error> {
        let mut items = Vec::new();
        while let Some(item) = seq.next_element_seed(NodeSeed {
            path: index(&self.path, items.len()),
        })? {
            items.push(item);
        }
        Ok(Node::Arr(items))
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Node, A::Error> {
        let mut fields: Vec<(String, Node)> = Vec::new();
        while let Some(key) = map.next_key::<String>()? {
            if fields.iter().any(|(k, _)| *k == key) {
                return Err(de::Error::custom(format!(
                    "{}: duplicate key {}",
                    self.path,
                    quote(&key)
                )));
            }
            let value = map.next_value_seed(NodeSeed {
                path: child(&self.path, &key),
            })?;
            fields.push((key, value));
        }
        Ok(Node::Obj(fields))
    }
}

/// Writes `node` as JSON with two-space indentation and a final newline.
pub(crate) fn to_pretty(node: &Node) -> String {
    let mut out = String::new();
    write_node(&mut out, node, 0);
    out.push('\n');
    out
}

fn write_node(out: &mut String, node: &Node, depth: usize) {
    match node {
        Node::Null => out.push_str("null"),
        Node::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        Node::Int(i) => {
            let _ = write!(out, "{i}");
        }
        Node::Float(f) => {
            let _ = write!(out, "{f}");
        }
        Node::Str(s) => out.push_str(&quote(s)),
        Node::Arr(items) => {
            if items.is_empty() {
                out.push_str("[]");
            } else if items
                .iter()
                .all(|i| !matches!(i, Node::Arr(_) | Node::Obj(_)))
            {
                out.push('[');
                for (i, item) in items.iter().enumerate() {
                    if i > 0 {
                        out.push_str(", ");
                    }
                    write_node(out, item, depth);
                }
                out.push(']');
            } else {
                out.push_str("[\n");
                for (i, item) in items.iter().enumerate() {
                    indent(out, depth + 1);
                    write_node(out, item, depth + 1);
                    if i + 1 < items.len() {
                        out.push(',');
                    }
                    out.push('\n');
                }
                indent(out, depth);
                out.push(']');
            }
        }
        Node::Obj(fields) => {
            if fields.is_empty() {
                out.push_str("{}");
                return;
            }
            out.push_str("{\n");
            for (i, (k, v)) in fields.iter().enumerate() {
                indent(out, depth + 1);
                out.push_str(&quote(k));
                out.push_str(": ");
                write_node(out, v, depth + 1);
                if i + 1 < fields.len() {
                    out.push(',');
                }
                out.push('\n');
            }
            indent(out, depth);
            out.push('}');
        }
    }
}

fn indent(out: &mut String, depth: usize) {
    for _ in 0..depth {
        out.push_str("  ");
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    #[test]
    fn duplicate_keys_name_their_path() {
        let e = parse(r#"{"a": {"b": 1, "b": 2}}"#).unwrap_err().to_string();
        assert!(e.contains("$.a: duplicate key \"b\""), "{e}");
    }

    #[test]
    fn comments_and_trailing_data_are_rejected() {
        assert!(parse("{} // x").is_err());
        assert!(parse("{\"a\": 1,}").is_err());
        assert!(parse("{} {}").is_err());
        assert!(parse("{\"a\": 1 /* c */}").is_err());
    }

    #[test]
    fn paths_quote_non_identifier_keys() {
        assert_eq!(child("$", "id"), "$.id");
        assert_eq!(child("$.requires", "io.x.y"), "$.requires[\"io.x.y\"]");
        assert_eq!(index("$.modules", 2), "$.modules[2]");
    }

    #[test]
    fn writes_back_what_it_reads() {
        let text = "{\n  \"a\": [1, \"x\"],\n  \"b\": {\n    \"c\": true\n  },\n  \"d\": []\n}\n";
        assert_eq!(to_pretty(&parse(text).unwrap()), text);
    }
}
