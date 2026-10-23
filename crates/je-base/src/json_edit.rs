//! Porte de `src/vs/base/common/jsonEdit.ts`: edições que inserem, trocam ou removem uma propriedade
//! num texto JSON com comentários, preservando o resto do texto e formatando só o trecho tocado.
//!
//! Deslocamentos em unidades UTF-16, como no original.
//!
//! Divergência de tipo: o caminho do original mistura `string` e `number` com `-1` para "no fim do
//! array"; aqui é [`EditSegment`], cujo índice é `i64` por esse motivo. As exceções do original
//! (`Malformed AST`, `Can not add ...`) viram [`JsonEditError`].

use std::fmt;

use crate::JsString;
use crate::json::{self, Node, NodeType, ParseOptions, Scanner, Segment, SyntaxKind, Tree, Value};
use crate::json_formatter::{Edit, FormattingOptions, Range, format, is_eol, json_stringify};

/// Um passo do caminho: nome de propriedade ou índice de array (`-1` é "no fim").
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EditSegment {
    Property(JsString),
    Index(i64),
}

impl From<&str> for EditSegment {
    fn from(name: &str) -> Self {
        Self::Property(JsString::from(name))
    }
}

impl From<i64> for EditSegment {
    fn from(index: i64) -> Self {
        Self::Index(index)
    }
}

/// `getInsertionIndex`: recebe os nomes das propriedades existentes e devolve onde inserir a nova.
pub type GetInsertionIndex<'a> = &'a dyn Fn(&[JsString]) -> usize;

/// O `Error` que `setProperty` lança.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JsonEditError(pub String);

impl fmt::Display for JsonEditError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for JsonEditError {}

fn type_name(kind: NodeType) -> &'static str {
    match kind {
        NodeType::Object => "object",
        NodeType::Array => "array",
        NodeType::Property => "property",
        NodeType::String => "string",
        NodeType::Number => "number",
        NodeType::Boolean => "boolean",
        NodeType::Null => "null",
    }
}

fn malformed() -> JsonEditError {
    JsonEditError("Malformed AST".to_string())
}

fn to_usize(n: i64) -> Result<usize, JsonEditError> {
    usize::try_from(n).map_err(|_| malformed())
}

fn find_node(tree: &Tree, from: Option<usize>, path: &[EditSegment]) -> Option<usize> {
    let mut node = from?;
    for segment in path {
        let n = tree.node(node);
        match segment {
            EditSegment::Property(name) => {
                if n.kind != NodeType::Object {
                    return None;
                }
                let property = n.children.as_deref()?.iter().copied().find(|&p| {
                    tree.node(p).children.as_deref().and_then(|c| c.first()).is_some_and(|&k| {
                        tree.node(k).value.as_ref() == Some(&Value::String(name.clone()))
                    })
                })?;
                node = *tree.node(property).children.as_deref()?.get(1)?;
            }
            EditSegment::Index(index) => {
                if n.kind != NodeType::Array {
                    return None;
                }
                node = *n.children.as_deref()?.get(usize::try_from(*index).ok()?)?;
            }
        }
    }
    Some(node)
}

/// `removeProperty`.
pub fn remove_property(
    text: &[u16],
    path: &[EditSegment],
    formatting_options: &FormattingOptions,
) -> Result<Vec<Edit>, JsonEditError> {
    set_property(text, path, None, formatting_options, None)
}

/// `setProperty`. `value` `None` é `undefined` (remover). `get_insertion_index` recebe os nomes das
/// propriedades existentes e devolve onde inserir a nova.
pub fn set_property(
    text: &[u16],
    original_path: &[EditSegment],
    value: Option<Value>,
    formatting_options: &FormattingOptions,
    get_insertion_index: Option<GetInsertionIndex<'_>>,
) -> Result<Vec<Edit>, JsonEditError> {
    let mut path = original_path.to_vec();
    let mut errors = Vec::new();
    let tree = json::parse_tree(text, &mut errors, ParseOptions::default());
    let root = tree.root();
    let mut value = value;
    let mut parent: Option<usize> = None;
    let mut last_segment: Option<EditSegment> = None;
    while let Some(segment) = path.pop() {
        parent = find_node(&tree, root, &path);
        if parent.is_none()
            && let Some(inner) = value.take()
        {
            value = Some(match &segment {
                EditSegment::Property(name) => {
                    let mut object = json::Object::new();
                    object.insert(name.clone(), inner);
                    Value::Object(object)
                }
                EditSegment::Index(_) => Value::Array(vec![inner]),
            });
            last_segment = Some(segment);
        } else {
            last_segment = Some(segment);
            break;
        }
    }

    let stringify = |v: &Value| JsString::from(json_stringify(v, None));

    let Some(parent_id) = parent else {
        // Documento vazio.
        let Some(value) = value else {
            return Ok(Vec::new()); // remover o que não existe: nada a fazer
        };
        let (offset, length) = match root.map(|r| tree.node(r)) {
            Some(r) => (to_usize(r.offset)?, to_usize(r.length)?),
            None => (0, 0),
        };
        return with_formatting(text, Edit { offset, length, content: stringify(&value) }, formatting_options);
    };
    let parent_node: &Node = tree.node(parent_id);
    let children: &[usize] = parent_node.children.as_deref().unwrap_or_default();

    match (parent_node.kind, &last_segment) {
        (NodeType::Object, Some(EditSegment::Property(name))) if parent_node.children.is_some() => {
            let existing = find_node(&tree, Some(parent_id), std::slice::from_ref(&EditSegment::Property(name.clone())));
            if let Some(existing) = existing {
                let existing_node = tree.node(existing);
                match value {
                    None => {
                        let existing_parent = existing_node.parent.ok_or_else(malformed)?;
                        let property_index =
                            children.iter().position(|&c| c == existing_parent).ok_or_else(malformed)?;
                        let existing_parent_node = tree.node(existing_parent);
                        let mut remove_end = existing_parent_node.offset + existing_parent_node.length;
                        let remove_begin;
                        if property_index > 0 {
                            // Remove a vírgula do nó anterior.
                            let previous = tree.node(children[property_index - 1]);
                            remove_begin = previous.offset + previous.length;
                        } else {
                            remove_begin = parent_node.offset + 1;
                            if children.len() > 1 {
                                // Remove a vírgula do próximo nó.
                                remove_end = tree.node(children[1]).offset;
                            } else {
                                // Remove a vírgula final do único nó.
                                let mut scanner = Scanner::new(text, true);
                                scanner.set_position(to_usize(remove_end)?);
                                if scanner.scan() == SyntaxKind::CommaToken {
                                    remove_end = scanner.position() as i64;
                                }
                            }
                        }
                        let edit = Edit {
                            offset: to_usize(remove_begin)?,
                            length: to_usize(remove_end - remove_begin)?,
                            content: JsString::default(),
                        };
                        with_formatting(text, edit, formatting_options)
                    }
                    Some(value) => {
                        let edit = Edit {
                            offset: to_usize(existing_node.offset)?,
                            length: to_usize(existing_node.length)?,
                            content: stringify(&value),
                        };
                        with_formatting(text, edit, formatting_options)
                    }
                }
            } else {
                let Some(value) = value else {
                    return Ok(Vec::new()); // remover o que não existe: nada a fazer
                };
                let new_property = format!("{}: {}", crate::dynamic::json_quote(name), json_stringify(&value, None));
                let index = match get_insertion_index {
                    Some(get) => {
                        let names: Vec<JsString> = children
                            .iter()
                            .filter_map(|&p| {
                                let key = *tree.node(p).children.as_deref()?.first()?;
                                match &tree.node(key).value {
                                    Some(Value::String(s)) => Some(s.clone()),
                                    _ => None,
                                }
                            })
                            .collect();
                        get(&names)
                    }
                    None => children.len(),
                };
                let edit = if index > 0 {
                    let previous = tree.node(*children.get(index - 1).ok_or_else(malformed)?);
                    Edit {
                        offset: to_usize(previous.offset + previous.length)?,
                        length: 0,
                        content: JsString::from(format!(",{new_property}")),
                    }
                } else if children.is_empty() {
                    Edit { offset: to_usize(parent_node.offset + 1)?, length: 0, content: JsString::from(new_property) }
                } else {
                    Edit {
                        offset: to_usize(parent_node.offset + 1)?,
                        length: 0,
                        content: JsString::from(format!("{new_property},")),
                    }
                };
                with_formatting(text, edit, formatting_options)
            }
        }
        (NodeType::Array, Some(EditSegment::Index(segment))) if parent_node.children.is_some() => {
            let segment = *segment;
            if let Some(value) = value {
                // Inserção.
                let new_property = json_stringify(&value, None);
                let edit = if children.is_empty() || segment == 0 {
                    Edit {
                        offset: to_usize(parent_node.offset + 1)?,
                        length: 0,
                        content: JsString::from(if children.is_empty() { new_property } else { format!("{new_property},") }),
                    }
                } else {
                    let index =
                        if segment == -1 || segment as usize > children.len() { children.len() } else { segment as usize };
                    let previous = tree.node(children[index - 1]);
                    Edit {
                        offset: to_usize(previous.offset + previous.length)?,
                        length: 0,
                        content: JsString::from(format!(",{new_property}")),
                    }
                };
                with_formatting(text, edit, formatting_options)
            } else {
                // Remoção.
                let removal_index = usize::try_from(segment).map_err(|_| malformed())?;
                let to_remove = tree.node(*children.get(removal_index).ok_or_else(malformed)?);
                let edit = if children.len() == 1 {
                    // Único item.
                    Edit {
                        offset: to_usize(parent_node.offset + 1)?,
                        length: to_usize(parent_node.length - 2)?,
                        content: JsString::default(),
                    }
                } else if children.len() - 1 == removal_index {
                    // Último item.
                    let previous = tree.node(children[removal_index - 1]);
                    let offset = previous.offset + previous.length;
                    let parent_end_offset = parent_node.offset + parent_node.length;
                    Edit {
                        offset: to_usize(offset)?,
                        length: to_usize(parent_end_offset - 1 - offset)?,
                        content: JsString::default(),
                    }
                } else {
                    Edit {
                        offset: to_usize(to_remove.offset)?,
                        length: to_usize(tree.node(children[removal_index + 1]).offset - to_remove.offset)?,
                        content: JsString::default(),
                    }
                };
                with_formatting(text, edit, formatting_options)
            }
        }
        _ => Err(JsonEditError(format!(
            "Can not add {} to parent of type {}",
            if matches!(last_segment, Some(EditSegment::Index(_))) { "property" } else { "index" },
            type_name(parent_node.kind)
        ))),
    }
}

/// `withFormatting`: aplica `edit`, formata a(s) linha(s) tocada(s) e devolve uma única edição com tudo.
pub fn with_formatting(
    text: &[u16],
    edit: Edit,
    formatting_options: &FormattingOptions,
) -> Result<Vec<Edit>, JsonEditError> {
    // Aplica a edição.
    let mut new_text = apply_edit(text, &edit);

    // Formata o texto novo.
    let mut begin = edit.offset;
    let mut end = edit.offset + edit.content.len();
    if edit.length == 0 || edit.content.is_empty() {
        // inserção ou remoção
        while begin > 0 && !is_eol(&new_text, begin - 1) {
            begin -= 1;
        }
        while end < new_text.len() && !is_eol(&new_text, end) {
            end += 1;
        }
    }

    let edits = format(&new_text, Some(Range { offset: begin, length: end - begin }), formatting_options);

    // Aplica as edições de formatação e acompanha o início e o fim do que mudou.
    for curr in edits.iter().rev() {
        new_text = apply_edit(&new_text, curr);
        begin = begin.min(curr.offset);
        end = end.max(curr.offset + curr.length);
        end = end + curr.content.len() - curr.length;
    }
    // Uma única edição com todas as mudanças.
    let edit_length = text.len() - (new_text.len() - end) - begin;
    Ok(vec![Edit {
        offset: begin,
        length: edit_length,
        content: JsString::from_units(new_text[begin..end].to_vec()),
    }])
}

/// `applyEdit`.
pub fn apply_edit(text: &[u16], edit: &Edit) -> Vec<u16> {
    let mut out = Vec::with_capacity(text.len() + edit.content.len());
    out.extend_from_slice(&text[..edit.offset.min(text.len())]);
    out.extend_from_slice(edit.content.units());
    out.extend_from_slice(&text[(edit.offset + edit.length).min(text.len())..]);
    out
}

/// `applyEdits`: as edições não podem se sobrepor (erro `Overlapping edit`).
pub fn apply_edits(text: &[u16], edits: &[Edit]) -> Result<Vec<u16>, JsonEditError> {
    let mut sorted: Vec<&Edit> = edits.iter().collect();
    sorted.sort_by(|a, b| a.offset.cmp(&b.offset).then(a.length.cmp(&b.length)));
    let mut text = text.to_vec();
    let mut last_modified_offset = text.len();
    for e in sorted.into_iter().rev() {
        if e.offset + e.length <= last_modified_offset {
            text = apply_edit(&text, e);
        } else {
            return Err(JsonEditError("Overlapping edit".to_string()));
        }
        last_modified_offset = e.offset;
    }
    Ok(text)
}

impl From<&Segment> for EditSegment {
    fn from(segment: &Segment) -> Self {
        match segment {
            Segment::Property(name) => Self::Property(name.clone()),
            Segment::Index(index) => Self::Index(*index as i64),
        }
    }
}
