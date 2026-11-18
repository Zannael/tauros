//! A JSON tree that keeps every byte of its source (G3-003).
//!
//! Porymap's files are not all formatted alike (95 of the pinned expansion's map
//! files write empty arrays as `[\n\n  ]`), so re-serialising a parsed file would
//! rewrite lines nobody changed. Instead each scalar keeps its raw text and each
//! container keeps the whitespace around its members: rendering an unmodified tree
//! gives the input back, and an edit changes only the edited value's text. New
//! members follow Porymap's layout (two-space indent, `"key": value`).

use std::fmt;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JsonError {
    pub line: u32,
    pub message: String,
}

impl fmt::Display for JsonError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "line {}: {}", self.line, self.message)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Node {
    Scalar(Scalar),
    Array(Array),
    Object(Object),
}

/// A string, number, boolean or null, stored as its source text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Scalar {
    pub raw: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Array {
    pub items: Vec<Item>,
    /// Whitespace before `]`.
    pub tail: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Item {
    /// Whitespace before the value.
    pub lead: String,
    pub node: Node,
    /// Whitespace between the value and the following `,` or `]`.
    pub trail: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Object {
    pub members: Vec<Member>,
    pub tail: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Member {
    pub lead: String,
    pub key: String,
    pub key_raw: String,
    /// Everything between the key and the value, including `:`.
    pub colon: String,
    pub node: Node,
    pub trail: String,
}

/// A whole file: the root value and the whitespace around it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Document {
    pub lead: String,
    pub root: Node,
    pub tail: String,
}

impl Document {
    pub fn parse(text: &str) -> Result<Self, JsonError> {
        let mut p = Parser { s: text.as_bytes(), i: 0 };
        let lead = p.ws();
        let root = p.value()?;
        let tail = p.ws();
        if p.i != p.s.len() {
            return Err(p.err("trailing characters after the JSON value"));
        }
        Ok(Self { lead, root, tail })
    }

    pub fn render(&self) -> String {
        let mut out = self.lead.clone();
        self.root.render_into(&mut out);
        out.push_str(&self.tail);
        out
    }
}

impl Scalar {
    pub fn string(value: &str) -> Self {
        Self { raw: quote(value) }
    }

    pub fn int(value: i64) -> Self {
        Self { raw: value.to_string() }
    }

    pub fn bool(value: bool) -> Self {
        Self { raw: value.to_string() }
    }

    pub fn null() -> Self {
        Self { raw: "null".into() }
    }

    pub fn as_str(&self) -> Option<String> {
        self.raw.starts_with('"').then(|| unquote(&self.raw)).flatten()
    }

    pub fn as_i64(&self) -> Option<i64> {
        self.raw.parse().ok()
    }

    pub fn as_bool(&self) -> Option<bool> {
        match self.raw.as_str() {
            "true" => Some(true),
            "false" => Some(false),
            _ => None,
        }
    }

    pub fn is_null(&self) -> bool {
        self.raw == "null"
    }
}

impl Node {
    pub fn string(value: &str) -> Self {
        Node::Scalar(Scalar::string(value))
    }

    pub fn int(value: i64) -> Self {
        Node::Scalar(Scalar::int(value))
    }

    pub fn bool(value: bool) -> Self {
        Node::Scalar(Scalar::bool(value))
    }

    pub fn as_scalar(&self) -> Option<&Scalar> {
        match self {
            Node::Scalar(s) => Some(s),
            _ => None,
        }
    }

    pub fn as_str(&self) -> Option<String> {
        self.as_scalar().and_then(Scalar::as_str)
    }

    pub fn as_i64(&self) -> Option<i64> {
        self.as_scalar().and_then(Scalar::as_i64)
    }

    pub fn as_bool(&self) -> Option<bool> {
        self.as_scalar().and_then(Scalar::as_bool)
    }

    pub fn as_array(&self) -> Option<&Array> {
        match self {
            Node::Array(a) => Some(a),
            _ => None,
        }
    }

    pub fn as_object(&self) -> Option<&Object> {
        match self {
            Node::Object(o) => Some(o),
            _ => None,
        }
    }

    pub fn as_object_mut(&mut self) -> Option<&mut Object> {
        match self {
            Node::Object(o) => Some(o),
            _ => None,
        }
    }

    /// A new object laid out like Porymap's, its first line indented by `indent`.
    pub fn object(pairs: Vec<(String, Node)>, indent: &str) -> Self {
        let inner = format!("{indent}  ");
        let members = pairs
            .into_iter()
            .map(|(key, node)| Member {
                lead: format!("\n{inner}"),
                key_raw: quote(&key),
                key,
                colon: ": ".into(),
                node: node.reindent(&inner),
                trail: String::new(),
            })
            .collect::<Vec<_>>();
        let tail = if members.is_empty() { String::new() } else { format!("\n{indent}") };
        Node::Object(Object { members, tail })
    }

    /// A new array laid out like Porymap's.
    pub fn array(nodes: Vec<Node>, indent: &str) -> Self {
        let inner = format!("{indent}  ");
        let items = nodes
            .into_iter()
            .map(|node| Item { lead: format!("\n{inner}"), node: node.reindent(&inner), trail: String::new() })
            .collect::<Vec<_>>();
        let tail = if items.is_empty() { String::new() } else { format!("\n{indent}") };
        Node::Array(Array { items, tail })
    }

    /// Moves a freshly built container to a new indentation level.
    fn reindent(self, indent: &str) -> Self {
        match self {
            Node::Scalar(_) => self,
            Node::Array(a) => Node::array(a.items.into_iter().map(|i| i.node).collect(), indent),
            Node::Object(o) => Node::object(o.members.into_iter().map(|m| (m.key, m.node)).collect(), indent),
        }
    }

    pub fn render(&self) -> String {
        let mut out = String::new();
        self.render_into(&mut out);
        out
    }

    fn render_into(&self, out: &mut String) {
        match self {
            Node::Scalar(s) => out.push_str(&s.raw),
            Node::Array(a) => {
                out.push('[');
                for (n, item) in a.items.iter().enumerate() {
                    if n > 0 {
                        out.push(',');
                    }
                    out.push_str(&item.lead);
                    item.node.render_into(out);
                    out.push_str(&item.trail);
                }
                out.push_str(&a.tail);
                out.push(']');
            }
            Node::Object(o) => {
                out.push('{');
                for (n, m) in o.members.iter().enumerate() {
                    if n > 0 {
                        out.push(',');
                    }
                    out.push_str(&m.lead);
                    out.push_str(&m.key_raw);
                    out.push_str(&m.colon);
                    m.node.render_into(out);
                    out.push_str(&m.trail);
                }
                out.push_str(&o.tail);
                out.push('}');
            }
        }
    }

    /// Same JSON value, whatever the formatting.
    pub fn same(&self, other: &Node) -> bool {
        match (self, other) {
            (Node::Scalar(a), Node::Scalar(b)) => {
                a.raw == b.raw
                    || (a.as_str().is_some() && a.as_str() == b.as_str())
                    || (a.as_i64().is_some() && a.as_i64() == b.as_i64())
            }
            (Node::Array(a), Node::Array(b)) => {
                a.items.len() == b.items.len() && a.items.iter().zip(&b.items).all(|(x, y)| x.node.same(&y.node))
            }
            (Node::Object(a), Node::Object(b)) => {
                a.members.len() == b.members.len()
                    && a.members.iter().zip(&b.members).all(|(x, y)| x.key == y.key && x.node.same(&y.node))
            }
            _ => false,
        }
    }
}

impl Object {
    pub fn get(&self, key: &str) -> Option<&Node> {
        self.members.iter().find(|m| m.key == key).map(|m| &m.node)
    }

    /// Indentation of this object's members (from an existing member, else `parent + 2`).
    pub fn member_indent(&self, own_indent: &str) -> String {
        self.members
            .first()
            .and_then(|m| m.lead.rsplit('\n').next().filter(|_| m.lead.contains('\n')))
            .map(str::to_owned)
            .unwrap_or_else(|| format!("{own_indent}  "))
    }

    /// Rebuilds the members from `desired`, keeping each existing member (and its
    /// formatting) whose value is the same, replacing changed values in place,
    /// dropping keys not desired and appending new keys at the end.
    /// Keys listed in `exact` take their node as given (already laid out); other new
    /// nodes are laid out at this object's member indentation.
    pub fn merge(&mut self, desired: Vec<(String, Node)>, own_indent: &str, exact: &[&str]) {
        let inner = self.member_indent(own_indent);
        let place = |key: &str, node: Node| if exact.contains(&key) { node } else { node.reindent(&inner) };
        let mut desired: Vec<Option<(String, Node)>> = desired.into_iter().map(Some).collect();
        let mut kept = Vec::new();
        for mut member in std::mem::take(&mut self.members) {
            let slot = desired.iter_mut().find(|d| d.as_ref().is_some_and(|(k, _)| *k == member.key));
            if let Some(slot) = slot {
                let (_, node) = slot.take().unwrap();
                if !member.node.same(&node) {
                    member.node = place(&member.key, node);
                }
                kept.push(member);
            }
        }
        for (key, node) in desired.into_iter().flatten() {
            let node = place(&key, node);
            kept.push(Member { lead: format!("\n{inner}"), key_raw: quote(&key), key, colon: ": ".into(), node, trail: String::new() });
        }
        if self.tail.is_empty() && !kept.is_empty() {
            self.tail = format!("\n{own_indent}");
        }
        if kept.is_empty() {
            self.tail.clear();
        }
        self.members = kept;
    }

    /// Replaces one member's value (or appends it); unchanged values keep their text.
    pub fn set(&mut self, key: &str, node: Node, own_indent: &str) {
        let inner = self.member_indent(own_indent);
        match self.members.iter_mut().find(|m| m.key == key) {
            Some(m) if m.node.same(&node) => {}
            Some(m) => m.node = node.reindent(&inner),
            None => {
                if self.tail.is_empty() {
                    self.tail = format!("\n{own_indent}");
                }
                self.members.push(Member {
                    lead: format!("\n{inner}"),
                    key_raw: quote(key),
                    key: key.to_owned(),
                    colon: ": ".into(),
                    node: node.reindent(&inner),
                    trail: String::new(),
                })
            }
        }
    }

    pub fn get_mut(&mut self, key: &str) -> Option<&mut Node> {
        self.members.iter_mut().find(|m| m.key == key).map(|m| &mut m.node)
    }
}

impl Array {
    pub fn item_indent(&self, own_indent: &str) -> String {
        self.items
            .first()
            .and_then(|i| i.lead.rsplit('\n').next().filter(|_| i.lead.contains('\n')))
            .map(str::to_owned)
            .unwrap_or_else(|| format!("{own_indent}  "))
    }
}

/// Edit script from `old` to `new` with a longest common subsequence: `Keep(i, j)`
/// for equal elements, `Change(i, j)` pairs unequal elements between matches,
/// `Insert(j)` and `Delete(i)` for the rest.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    Keep(usize, usize),
    Change(usize, usize),
    Insert(usize),
    Delete(usize),
}

pub fn align<T: PartialEq>(old: &[T], new: &[T]) -> Vec<Step> {
    let (n, m) = (old.len(), new.len());
    let mut lcs = vec![vec![0usize; m + 1]; n + 1];
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            lcs[i][j] = if old[i] == new[j] { lcs[i + 1][j + 1] + 1 } else { lcs[i + 1][j].max(lcs[i][j + 1]) };
        }
    }
    let mut steps = Vec::new();
    let (mut i, mut j) = (0, 0);
    let (mut pending_old, mut pending_new) = (Vec::new(), Vec::new());
    let flush = |steps: &mut Vec<Step>, po: &mut Vec<usize>, pn: &mut Vec<usize>| {
        let pairs = po.len().min(pn.len());
        for k in 0..pairs {
            steps.push(Step::Change(po[k], pn[k]));
        }
        steps.extend(po[pairs..].iter().map(|&i| Step::Delete(i)));
        steps.extend(pn[pairs..].iter().map(|&j| Step::Insert(j)));
        po.clear();
        pn.clear();
    };
    while i < n || j < m {
        if i < n && j < m && old[i] == new[j] {
            flush(&mut steps, &mut pending_old, &mut pending_new);
            steps.push(Step::Keep(i, j));
            i += 1;
            j += 1;
        } else if j < m && (i == n || lcs[i][j + 1] >= lcs[i + 1][j]) {
            pending_new.push(j);
            j += 1;
        } else {
            pending_old.push(i);
            i += 1;
        }
    }
    flush(&mut steps, &mut pending_old, &mut pending_new);
    steps
}

pub fn quote(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    out.push('"');
    for c in value.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn unquote(raw: &str) -> Option<String> {
    let inner = raw.strip_prefix('"')?.strip_suffix('"')?;
    let mut out = String::with_capacity(inner.len());
    let mut chars = inner.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next()? {
            '"' => out.push('"'),
            '\\' => out.push('\\'),
            '/' => out.push('/'),
            'b' => out.push('\u{8}'),
            'f' => out.push('\u{c}'),
            'n' => out.push('\n'),
            'r' => out.push('\r'),
            't' => out.push('\t'),
            'u' => {
                let hex: String = chars.by_ref().take(4).collect();
                let code = u32::from_str_radix(&hex, 16).ok()?;
                out.push(char::from_u32(code).unwrap_or('\u{fffd}'));
            }
            _ => return None,
        }
    }
    Some(out)
}

struct Parser<'a> {
    s: &'a [u8],
    i: usize,
}

impl Parser<'_> {
    fn err(&self, message: &str) -> JsonError {
        let line = self.s[..self.i.min(self.s.len())].iter().filter(|&&b| b == b'\n').count() as u32 + 1;
        JsonError { line, message: message.to_owned() }
    }

    fn ws(&mut self) -> String {
        let start = self.i;
        while self.i < self.s.len() && matches!(self.s[self.i], b' ' | b'\t' | b'\n' | b'\r') {
            self.i += 1;
        }
        self.text(start)
    }

    fn text(&self, start: usize) -> String {
        String::from_utf8_lossy(&self.s[start..self.i]).into_owned()
    }

    fn value(&mut self) -> Result<Node, JsonError> {
        match self.s.get(self.i) {
            Some(b'{') => self.object(),
            Some(b'[') => self.array(),
            Some(b'"') => Ok(Node::Scalar(Scalar { raw: self.string()? })),
            Some(_) => {
                let start = self.i;
                while self.i < self.s.len() && matches!(self.s[self.i], b'-' | b'+' | b'.' | b'0'..=b'9' | b'a'..=b'z' | b'A'..=b'Z') {
                    self.i += 1;
                }
                let raw = self.text(start);
                let number = raw.parse::<f64>().is_ok() && !raw.starts_with('+');
                if !(number || matches!(raw.as_str(), "true" | "false" | "null")) {
                    return Err(self.err(&format!("unexpected token '{raw}'")));
                }
                Ok(Node::Scalar(Scalar { raw }))
            }
            None => Err(self.err("unexpected end of input")),
        }
    }

    fn string(&mut self) -> Result<String, JsonError> {
        let start = self.i;
        self.i += 1;
        loop {
            match self.s.get(self.i) {
                Some(b'"') => {
                    self.i += 1;
                    return Ok(self.text(start));
                }
                Some(b'\\') => self.i += 2,
                Some(_) => self.i += 1,
                None => return Err(self.err("unterminated string")),
            }
        }
    }

    fn expect(&mut self, byte: u8) -> Result<(), JsonError> {
        if self.s.get(self.i) == Some(&byte) {
            self.i += 1;
            Ok(())
        } else {
            Err(self.err(&format!("expected '{}'", byte as char)))
        }
    }

    fn array(&mut self) -> Result<Node, JsonError> {
        self.expect(b'[')?;
        let mut items = Vec::new();
        loop {
            let lead = self.ws();
            if self.s.get(self.i) == Some(&b']') {
                self.i += 1;
                return Ok(Node::Array(Array { items, tail: lead }));
            }
            let node = self.value()?;
            let trail = self.ws();
            items.push(Item { lead, node, trail });
            match self.s.get(self.i) {
                Some(b',') => self.i += 1,
                Some(b']') => {
                    self.i += 1;
                    let item = items.last_mut().unwrap();
                    let tail = std::mem::take(&mut item.trail);
                    return Ok(Node::Array(Array { items, tail }));
                }
                _ => return Err(self.err("expected ',' or ']'")),
            }
        }
    }

    fn object(&mut self) -> Result<Node, JsonError> {
        self.expect(b'{')?;
        let mut members = Vec::new();
        loop {
            let lead = self.ws();
            if self.s.get(self.i) == Some(&b'}') {
                self.i += 1;
                return Ok(Node::Object(Object { members, tail: lead }));
            }
            if self.s.get(self.i) != Some(&b'"') {
                return Err(self.err("expected a key"));
            }
            let key_raw = self.string()?;
            let key = unquote(&key_raw).ok_or_else(|| self.err("invalid key"))?;
            let start = self.i;
            self.ws();
            self.expect(b':')?;
            self.ws();
            let colon = self.text(start);
            let node = self.value()?;
            let trail = self.ws();
            members.push(Member { lead, key, key_raw, colon, node, trail });
            match self.s.get(self.i) {
                Some(b',') => self.i += 1,
                Some(b'}') => {
                    self.i += 1;
                    let member = members.last_mut().unwrap();
                    let tail = std::mem::take(&mut member.trail);
                    return Ok(Node::Object(Object { members, tail }));
                }
                _ => return Err(self.err("expected ',' or '}'")),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "{\n  \"id\": \"MAP_A\",\n  \"n\": 3,\n  \"flag\": true,\n  \"none\": null,\n  \"list\": [\n\n  ],\n  \"objs\": [\n    {\n      \"x\": 1\n    }\n  ]\n}\n";

    #[test]
    fn roundtrip_keeps_every_byte() {
        let doc = Document::parse(SAMPLE).unwrap();
        assert_eq!(doc.render(), SAMPLE);
        let odd = " [1 ,2,\t\"a\\\"b\\u00e9\" ] ";
        let doc = Document::parse(odd).unwrap();
        assert_eq!(doc.render(), odd);
        assert_eq!(doc.root.as_array().unwrap().items[2].node.as_str().unwrap(), "a\"bé");
    }

    #[test]
    fn edits_change_only_their_values() {
        let mut doc = Document::parse(SAMPLE).unwrap();
        let root = doc.root.as_object_mut().unwrap();
        root.set("n", Node::int(4), "");
        root.set("id", Node::string("MAP_A"), "");
        root.set("added", Node::array(vec![Node::object(vec![("y".into(), Node::int(2))], "")], ""), "");
        let out = doc.render();
        assert_eq!(
            out,
            "{\n  \"id\": \"MAP_A\",\n  \"n\": 4,\n  \"flag\": true,\n  \"none\": null,\n  \"list\": [\n\n  ],\n  \"objs\": [\n    {\n      \"x\": 1\n    }\n  ],\n  \"added\": [\n    {\n      \"y\": 2\n    }\n  ]\n}\n"
        );
        assert!(Document::parse(&out).is_ok());
    }

    #[test]
    fn alignment_is_minimal() {
        use Step::*;
        assert_eq!(align(&[1, 2, 3], &[1, 3]), [Keep(0, 0), Delete(1), Keep(2, 1)]);
        assert_eq!(align(&[1, 2, 3], &[1, 9, 3, 4]), [Keep(0, 0), Change(1, 1), Keep(2, 2), Insert(3)]);
        assert_eq!(align::<i32>(&[], &[]), []);
    }

    #[test]
    fn errors_have_lines() {
        assert_eq!(Document::parse("{\n  \"a\": nope\n}").unwrap_err().line, 2);
        assert!(Document::parse("[1,").is_err());
    }
}
