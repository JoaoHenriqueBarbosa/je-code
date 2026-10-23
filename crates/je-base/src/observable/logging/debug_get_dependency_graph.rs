//! Porte de `observableInternal/logging/debugGetDependencyGraph.ts` e do `formatValue` do
//! `consoleObservableLogger.ts`. O resto do logging (`logging.ts`, `ConsoleObservableLogger`,
//! `DevToolsLogger`) fica de fora: o `getLogger()` do original é `undefined` salvo quando alguém liga o
//! logger à mão, e nenhum comportamento depende dele.

use std::any::Any;
use std::collections::HashSet;

use crate::observable::base::{DebugInfo, DebugNode};

/// `formatValue(value, availableLen)`. Sem reflexão, só os tipos primitivos usuais são formatados; o
/// resto vira `[object]`.
pub fn format_value(value: &dyn Any, available_len: usize) -> String {
    macro_rules! numbers {
        ($($t:ty),*) => {$( if let Some(v) = value.downcast_ref::<$t>() { return v.to_string(); } )*};
    }
    numbers!(i8, i16, i32, i64, isize, u8, u16, u32, u64, usize, f32, f64);
    let quote = |s: &str| {
        if s.chars().count() + 2 <= available_len {
            format!("\"{s}\"")
        } else {
            let keep = available_len.saturating_sub(7);
            format!("\"{}\"+...", s.chars().take(keep).collect::<String>())
        }
    };
    if let Some(v) = value.downcast_ref::<String>() {
        return quote(v);
    }
    if let Some(v) = value.downcast_ref::<&'static str>() {
        return quote(v);
    }
    if let Some(v) = value.downcast_ref::<bool>() {
        return if *v { "true" } else { "false" }.into();
    }
    if value.is::<()>() {
        return "undefined".into();
    }
    "[object]".into()
}

/// O tipo de grafo pedido.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum GraphType {
    Dependencies,
    Observers,
}

pub struct GraphOptions {
    pub kind: GraphType,
    pub debug_name_post_processor: Option<Box<dyn Fn(&str) -> String>>,
}

fn node_id(node: &DebugNode) -> usize {
    match node {
        DebugNode::Observable(o) => std::rc::Rc::as_ptr(o).cast::<()>() as usize,
        DebugNode::Observer(o) => std::rc::Rc::as_ptr(o).cast::<()>() as usize,
    }
}

fn info_from(node: &DebugNode, options: &GraphOptions) -> Option<DebugInfo> {
    let mut info = match node {
        DebugNode::Observable(o) => o.debug_info(),
        DebugNode::Observer(o) => o.debug_info(),
    }?;
    if let Some(p) = &options.debug_name_post_processor {
        info.name = p(&info.name);
    }
    Some(info)
}

fn info_unknown(node: &DebugNode) -> DebugInfo {
    DebugInfo {
        source_id: node_id(node),
        name: "(unknown)".into(),
        kind: "unknown",
        value: "undefined".into(),
        state: "unknown".into(),
        dependencies: Vec::new(),
        observers: Vec::new(),
    }
}

/// `debugGetObservableGraph(obs, options)`.
pub fn debug_get_observable_graph(obs: &DebugNode, options: &GraphOptions) -> String {
    let Some(info) = info_from(obs, options) else { return String::new() };
    let mut already_listed = HashSet::new();
    format_info(&info, 0, &mut already_listed, options).trim().to_owned()
}

fn format_info(info: &DebugInfo, indent_level: usize, already_listed: &mut HashSet<usize>, options: &GraphOptions) -> String {
    let indent = "\t\t".repeat(indent_level);
    let mut lines: Vec<String> = Vec::new();
    if already_listed.contains(&info.source_id) {
        lines.push(format!("{indent}* {} {} (already listed)", info.kind, info.name));
        return lines.join("\n");
    }
    already_listed.insert(info.source_id);
    lines.push(format!("{indent}* {} {}:", info.kind, info.name));
    lines.push(format!("{indent}  value: {}", info.value));
    lines.push(format!("{indent}  state: {}", info.state));
    let (label, children) = match options.kind {
        GraphType::Dependencies => ("dependencies", &info.dependencies),
        GraphType::Observers => ("observers", &info.observers),
    };
    if !children.is_empty() {
        lines.push(format!("{indent}  {label}:"));
        for child in children {
            let child_info = info_from(child, options).unwrap_or_else(|| info_unknown(child));
            lines.push(format_info(&child_info, indent_level + 1, already_listed, options));
        }
    }
    lines.join("\n")
}
