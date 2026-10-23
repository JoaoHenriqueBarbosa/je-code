//! Porte de `src/vs/platform/webContentExtractor/electron-main/cdpAccessibilityDomain.ts`: a conversão da
//! árvore de acessibilidade do CDP (`Accessibility.getFullAXTree`) em Markdown.
//!
//! Divergências:
//! - Os tipos do domínio (`AXValue`, `AXNode`...) são structs e enums de Rust. As uniões de strings
//!   (`AXValueType`, `AXPropertyName`...) são enums com `as_str` e `parse`, para o carregador que
//!   decodificar o JSON do CDP. `AXValue.value` (`unknown`) é um [`Dynamic`].
//! - Onde o original faz `value as string || ''`, aqui só um valor que é string não vazia conta; um
//!   valor de outro tipo é tratado como ausente (o original seguiria com um não-string e quebraria
//!   adiante).
//! - A árvore (`AXNodeTree`, com `parent` e `children` por referência) é uma arena de índices.
//! - O texto sai em `String`. O quebra de linha em 80 e a contagem de caracteres do código contam
//!   unidades UTF-16, como o `length` do JavaScript; um surrogate isolado (que o JSON do CDP quase
//!   nunca traz) vira U+FFFD ao voltar para `String`.
//! - `Number(level) || 1` com nível negativo daria `RangeError` no `repeat` do original; aqui o
//!   número de repetições é truncado e limitado em zero.
//! - O parâmetro `depth` de `processNode` e derivados nunca é lido no original (só repassado), então
//!   não existe aqui.
//! - `URI.parse` nunca lança no modo não estrito, então o `try/catch` de `isSameUriIgnoringQueryAndFragment`
//!   não tem o que capturar.

use std::collections::HashMap;

use indexmap::IndexMap;
use je_base::JsString;
use je_base::dynamic::Dynamic;
use je_base::js_string;
use je_base::uri::URI;

use crate::js::to_number;

//#region Types

/// Gera um enum de variantes que correspondem, uma a uma, a uma união de strings do CDP.
macro_rules! string_enum {
    ($(#[$meta:meta])* $name:ident { $($variant:ident => $text:literal),+ $(,)? }) => {
        $(#[$meta])*
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        pub enum $name {
            $($variant),+
        }

        impl $name {
            /// O texto da união no CDP.
            pub fn as_str(self) -> &'static str {
                match self {
                    $(Self::$variant => $text),+
                }
            }

            /// A variante para o texto da união no CDP.
            pub fn parse(text: &str) -> Option<Self> {
                match text {
                    $($text => Some(Self::$variant),)+
                    _ => None,
                }
            }
        }
    };
}

string_enum! {
    /// `AXValueType`.
    AXValueType {
        Boolean => "boolean",
        Tristate => "tristate",
        BooleanOrUndefined => "booleanOrUndefined",
        Idref => "idref",
        IdrefList => "idrefList",
        Integer => "integer",
        Node => "node",
        NodeList => "nodeList",
        Number => "number",
        String => "string",
        ComputedString => "computedString",
        Token => "token",
        TokenList => "tokenList",
        DomRelation => "domRelation",
        Role => "role",
        InternalRole => "internalRole",
        ValueUndefined => "valueUndefined",
    }
}

string_enum! {
    /// `AXValueSourceType`.
    AXValueSourceType {
        Attribute => "attribute",
        Implicit => "implicit",
        Style => "style",
        Contents => "contents",
        Placeholder => "placeholder",
        RelatedElement => "relatedElement",
    }
}

string_enum! {
    /// `AXValueNativeSourceType`.
    AXValueNativeSourceType {
        Description => "description",
        Figcaption => "figcaption",
        Label => "label",
        Labelfor => "labelfor",
        Labelwrapped => "labelwrapped",
        Legend => "legend",
        Rubyannotation => "rubyannotation",
        Tablecaption => "tablecaption",
        Title => "title",
        Other => "other",
    }
}

string_enum! {
    /// `AXPropertyName`.
    AXPropertyName {
        Url => "url",
        Busy => "busy",
        Disabled => "disabled",
        Editable => "editable",
        Focusable => "focusable",
        Focused => "focused",
        Hidden => "hidden",
        HiddenRoot => "hiddenRoot",
        Invalid => "invalid",
        Keyshortcuts => "keyshortcuts",
        Settable => "settable",
        Roledescription => "roledescription",
        Live => "live",
        Atomic => "atomic",
        Relevant => "relevant",
        Root => "root",
        Autocomplete => "autocomplete",
        HasPopup => "hasPopup",
        Level => "level",
        Multiselectable => "multiselectable",
        Orientation => "orientation",
        Multiline => "multiline",
        Readonly => "readonly",
        Required => "required",
        Valuemin => "valuemin",
        Valuemax => "valuemax",
        Valuetext => "valuetext",
        Checked => "checked",
        Expanded => "expanded",
        Pressed => "pressed",
        Selected => "selected",
        Activedescendant => "activedescendant",
        Controls => "controls",
        Describedby => "describedby",
        Details => "details",
        Errormessage => "errormessage",
        Flowto => "flowto",
        Labelledby => "labelledby",
        Owns => "owns",
    }
}

/// `AXValue`.
#[derive(Clone, Debug)]
pub struct AXValue {
    pub type_: AXValueType,
    pub value: Option<Dynamic>,
    pub related_nodes: Option<Vec<AXNode>>,
    pub sources: Option<Vec<AXValueSource>>,
}

/// `AXValueSource`.
#[derive(Clone, Debug)]
pub struct AXValueSource {
    pub type_: AXValueSourceType,
    pub value: Option<AXValue>,
    pub attribute: Option<String>,
    pub attribute_value: Option<String>,
    pub superseded: Option<bool>,
    pub native_source: Option<AXValueNativeSourceType>,
    pub native_source_value: Option<String>,
    pub invalid: Option<bool>,
    pub invalid_reason: Option<String>,
}

/// `AXNode`.
#[derive(Clone, Debug, Default)]
pub struct AXNode {
    pub node_id: String,
    pub ignored: bool,
    pub ignored_reasons: Option<Vec<AXProperty>>,
    pub role: Option<AXValue>,
    pub chrome_role: Option<AXValue>,
    pub name: Option<AXValue>,
    pub description: Option<AXValue>,
    pub value: Option<AXValue>,
    pub properties: Option<Vec<AXProperty>>,
    pub child_ids: Option<Vec<String>>,
    pub backend_dom_node_id: Option<i64>,
}

/// `AXProperty`.
#[derive(Clone, Debug)]
pub struct AXProperty {
    pub name: AXPropertyName,
    pub value: AXValue,
}

//#endregion

/// `AXNodeTree`: um nó da árvore, com os filhos e o pai como índices da arena de [`AXForest`].
struct AXNodeTree<'a> {
    node: &'a AXNode,
    children: Vec<usize>,
    parent: Option<usize>,
}

/// A floresta de árvores de nós, em arena, e os índices das raízes.
struct AXForest<'a> {
    nodes: Vec<AXNodeTree<'a>>,
    roots: Vec<usize>,
}

/// Todos os descendentes não ignorados de um nó (os ignorados são atravessados).
fn get_non_ignored_descendants<'a>(node_lookup: &HashMap<&'a str, &'a AXNode>, node_id: &str) -> Vec<&'a str> {
    let Some(child_ids) = node_lookup.get(node_id).and_then(|node| node.child_ids.as_ref()) else {
        return Vec::new();
    };

    let mut result = Vec::new();
    for child_id in child_ids {
        let Some(child_node) = node_lookup.get(child_id.as_str()) else {
            continue;
        };

        if child_node.ignored {
            // If child is ignored, add its non-ignored descendants instead
            result.extend(get_non_ignored_descendants(node_lookup, child_id));
        } else {
            // Otherwise, add the child itself
            result.push(child_id.as_str());
        }
    }
    result
}

/// Creates a forest of node trees from the given AXNodes.
/// When nodes come from multiple frames (e.g., main frame + iframes),
/// each frame has its own RootWebArea, resulting in multiple trees.
fn create_node_trees(nodes: &[AXNode]) -> AXForest<'_> {
    // Create a map of node IDs to their corresponding nodes for quick lookup
    let mut node_lookup: HashMap<&str, &AXNode> = HashMap::new();
    for node in nodes {
        node_lookup.insert(node.node_id.as_str(), node);
    }

    // Create tree nodes only for non-ignored nodes
    let mut arena: Vec<AXNodeTree<'_>> = Vec::new();
    let mut node_map: IndexMap<&str, usize> = IndexMap::new();
    for node in nodes {
        if !node.ignored {
            node_map.insert(node.node_id.as_str(), arena.len());
            arena.push(AXNodeTree { node, children: Vec::new(), parent: None });
        }
    }

    // Establish parent-child relationships, bypassing ignored nodes
    for node in nodes {
        if node.ignored {
            continue;
        }

        let tree_index = node_map[node.node_id.as_str()];
        let Some(child_ids) = &node.child_ids else {
            continue;
        };
        for child_id in child_ids {
            let Some(child_node) = node_lookup.get(child_id.as_str()) else {
                continue;
            };

            let attached = if child_node.ignored {
                // If child is ignored, connect its non-ignored descendants to this node
                get_non_ignored_descendants(&node_lookup, child_id)
            } else {
                // Normal case: add non-ignored child directly
                vec![child_id.as_str()]
            };
            for attached_id in attached {
                if let Some(&child_index) = node_map.get(attached_id) {
                    arena[child_index].parent = Some(tree_index);
                    arena[tree_index].children.push(child_index);
                }
            }
        }
    }

    // Find all root nodes (nodes without a parent)
    // When nodes come from multiple frames, each frame has its own root
    let roots = node_map.values().copied().filter(|&index| arena[index].parent.is_none()).collect();

    AXForest { nodes: arena, roots }
}

/// When possible, we will make sure lines are no longer than 80. This is to help
/// certain pieces of software that can't handle long lines.
const LINE_MAX_LENGTH: usize = 80;

/// Converts an accessibility tree represented by AXNode objects into a markdown string.
/// Handles multiple root nodes (e.g., from main frame + iframes) by processing each tree
/// and combining the results.
///
/// `uri` is the URI of the document and `ax_nodes` the array of AXNode objects representing the
/// accessibility tree. Returns a markdown representation of the accessibility tree.
pub fn convert_ax_tree_to_markdown(uri: &URI, ax_nodes: &[AXNode]) -> String {
    let forest = create_node_trees(ax_nodes);
    if forest.roots.is_empty() {
        return String::new(); // Return empty string for empty tree
    }

    let converter = Converter { uri, forest: &forest };

    // Process each tree and collect main content and navigation links
    let mut all_main_content: Vec<String> = Vec::new();
    let mut all_nav_links: Vec<String> = Vec::new();

    for &tree in &forest.roots {
        let main_content = converter.extract_main_content(tree);
        let nav_links = converter.collect_navigation_links(tree);

        if !js_trim(&main_content).is_empty() {
            all_main_content.push(main_content);
        }
        all_nav_links.extend(nav_links);
    }

    // Combine all main content from all trees
    let combined_main_content = all_main_content.join("\n\n");

    // Combine main content and navigation links
    let additional_links = if all_nav_links.is_empty() { String::new() } else { format!("\n\n## Additional Links\n{}", all_nav_links.join("\n")) };
    combined_main_content + &additional_links
}

/// `String.prototype.trim` do JavaScript (o conjunto de espaços difere do de `str::trim`).
fn js_trim(text: &str) -> String {
    let units: Vec<u16> = text.encode_utf16().collect();
    String::from_utf16_lossy(js_string::trim(&units))
}

/// `text.length` do JavaScript.
fn js_length(text: &str) -> usize {
    text.encode_utf16().count()
}

/// `String.prototype.repeat` com a contagem já limitada em zero (ver as divergências).
fn repeat(text: &str, count: f64) -> String {
    text.repeat(count.max(0.0) as usize)
}

/// O texto de um `AXValue` quando o valor é uma string não vazia (o `value as string || ''` do original).
fn truthy_string(value: Option<&AXValue>) -> Option<&JsString> {
    match value?.value.as_ref()? {
        Dynamic::String(text) if !text.is_empty() => Some(text),
        _ => None,
    }
}

fn find_property(node: &AXNode, name: AXPropertyName) -> Option<&AXProperty> {
    node.properties.as_ref()?.iter().find(|property| property.name == name)
}

fn get_node_role(node: &AXNode) -> String {
    truthy_string(node.role.as_ref()).map(JsString::to_string_lossy).unwrap_or_default()
}

fn get_node_text(node: &AXNode, allow_wrap: bool) -> String {
    let text = truthy_string(node.name.as_ref()).or_else(|| truthy_string(node.value.as_ref())).cloned().unwrap_or_default();
    if !allow_wrap || text.len() <= LINE_MAX_LENGTH {
        return text.to_string_lossy();
    }

    let mut chars = text.units().to_vec();
    let mut last_space_index: Option<usize> = None;
    for i in 1..chars.len() {
        if chars[i] == u16::from(b' ') {
            last_space_index = Some(i);
        }
        // Check if we reached the line max length, try to break at the last space
        // before the line max length
        if i % LINE_MAX_LENGTH == 0 {
            if let Some(space_index) = last_space_index {
                // replace the space with a new line
                chars[space_index] = u16::from(b'\n');
                last_space_index = Some(i);
            }
        }
    }
    String::from_utf16_lossy(&chars)
}

fn get_level(node: &AXNode) -> f64 {
    match find_property(node, AXPropertyName::Level) {
        Some(level_prop) => {
            let level = level_prop.value.value.as_ref().map_or(f64::NAN, to_number);
            // `Number(...) || 1`: NaN e zero caem no padrão.
            let level = if level.is_nan() || level == 0.0 { 1.0 } else { level };
            level.min(6.0)
        }
        None => 1.0,
    }
}

/// A propriedade `url` (link ou imagem), quando é uma string não vazia.
fn get_property_url(node: &AXNode) -> Option<String> {
    truthy_string(find_property(node, AXPropertyName::Url).map(|property| &property.value)).map(JsString::to_string_lossy)
}

fn get_link_url(node: &AXNode) -> String {
    // Find URL in properties
    get_property_url(node).unwrap_or_else(|| "#".to_owned())
}

fn is_same_uri_ignoring_query_and_fragment(uri: &URI, node: &AXNode) -> bool {
    // Check if this link is an anchor link
    let link = get_link_url(node);
    let parsed = URI::parse(link, false);
    parsed.scheme() == uri.scheme() && parsed.authority() == uri.authority() && parsed.path() == uri.path()
}

fn is_table_cell(role: &str) -> bool {
    // Match cell, gridcell, columnheader, rowheader roles
    matches!(role, "cell" | "gridcell" | "columnheader" | "rowheader")
}

/// A conversão de uma floresta: o documento e a arena que os métodos percorrem.
struct Converter<'a> {
    uri: &'a URI,
    forest: &'a AXForest<'a>,
}

impl Converter<'_> {
    fn tree(&self, index: usize) -> &AXNodeTree<'_> {
        &self.forest.nodes[index]
    }

    fn role(&self, index: usize) -> String {
        get_node_role(self.tree(index).node)
    }

    fn extract_main_content(&self, tree: usize) -> String {
        let mut content_buffer: Vec<String> = Vec::new();
        self.process_node(tree, &mut content_buffer, true);
        content_buffer.join("")
    }

    fn process_children(&self, index: usize, buffer: &mut Vec<String>, allow_wrap: bool) {
        for &child in &self.tree(index).children {
            self.process_node(child, buffer, allow_wrap);
        }
    }

    fn process_node(&self, index: usize, buffer: &mut Vec<String>, allow_wrap: bool) {
        let node = self.tree(index).node;
        let role = get_node_role(node);

        match role.as_str() {
            "navigation" => return, // Skip navigation nodes

            "heading" => {
                self.process_heading_node(index, buffer);
                return;
            }

            "paragraph" => {
                self.process_paragraph_node(index, buffer, allow_wrap);
                return;
            }

            "list" => {
                buffer.push("\n".to_owned());
                self.process_children(index, buffer, true);
                buffer.push("\n".to_owned());
                return;
            }

            "ListMarker" => {
                // TODO: Should we normalize these ListMarkers to `-` and normal lists?
                buffer.push(get_node_text(node, allow_wrap));
                return;
            }

            "listitem" => {
                let mut temp_buffer: Vec<String> = Vec::new();
                // Process the children of the list item
                self.process_children(index, &mut temp_buffer, true);
                let level = get_level(node);
                let indent = if level > 1.0 { repeat(" ", level) } else { String::new() };
                buffer.push(format!("{indent}{}\n", js_trim(&temp_buffer.join(""))));
                return;
            }

            "link" => {
                if !self.is_navigation_link(index) {
                    let link_text = get_node_text(node, allow_wrap);
                    let url = get_link_url(node);
                    if !is_same_uri_ignoring_query_and_fragment(self.uri, node) {
                        buffer.push(format!("[{link_text}]({url})"));
                    } else {
                        buffer.push(link_text);
                    }
                }
                return;
            }

            "StaticText" => {
                let static_text = get_node_text(node, allow_wrap);
                if !static_text.is_empty() {
                    buffer.push(static_text);
                }
            }

            "image" => {
                let text = get_node_text(node, allow_wrap);
                let alt_text = if text.is_empty() { "Image".to_owned() } else { text };
                match get_property_url(node) {
                    Some(image_url) => buffer.push(format!("![{alt_text}]({image_url})\n\n")),
                    None => buffer.push(format!("[Image: {alt_text}]\n\n")),
                }
            }

            "DescriptionList" => {
                self.process_description_list_node(index, buffer);
                return;
            }

            "blockquote" => {
                buffer.push(format!("> {}\n\n", get_node_text(node, allow_wrap).replace('\n', "\n> ")));
            }

            // TODO: Is this the correct way to handle the generic role?
            "generic" => buffer.push(" ".to_owned()),

            "code" => {
                self.process_code_node(index, buffer);
                return;
            }

            "pre" => buffer.push(format!("```\n{}\n```\n\n", get_node_text(node, false))),

            "table" => {
                self.process_table_node(index, buffer);
                return;
            }

            _ => {}
        }

        // Process children if not already handled in specific cases
        self.process_children(index, buffer, allow_wrap);
    }

    fn is_navigation_link(&self, index: usize) -> bool {
        // Check if this link is part of navigation
        let mut current = Some(index);
        while let Some(current_index) = current {
            if matches!(self.role(current_index).as_str(), "navigation" | "menu" | "menubar") {
                return true;
            }
            current = self.tree(current_index).parent;
        }
        false
    }

    fn process_paragraph_node(&self, index: usize, buffer: &mut Vec<String>, allow_wrap: bool) {
        buffer.push("\n".to_owned());
        // Process the children of the paragraph
        self.process_children(index, buffer, allow_wrap);
        buffer.push("\n\n".to_owned());
    }

    fn process_heading_node(&self, index: usize, buffer: &mut Vec<String>) {
        buffer.push("\n".to_owned());
        let level = get_level(self.tree(index).node);
        buffer.push(format!("{} ", repeat("#", level)));
        // Process children nodes of the heading
        for &child in &self.tree(index).children {
            if self.role(child) == "StaticText" {
                buffer.push(get_node_text(self.tree(child).node, false));
            } else {
                self.process_node(child, buffer, false);
            }
        }
        buffer.push("\n\n".to_owned());
    }

    fn process_description_list_node(&self, index: usize, buffer: &mut Vec<String>) {
        buffer.push("\n".to_owned());

        // Process each child of the description list
        for &child in &self.tree(index).children {
            match self.role(child).as_str() {
                "term" => {
                    buffer.push("- **".to_owned());
                    // Process term nodes
                    self.process_children(child, buffer, true);
                    buffer.push("** ".to_owned());
                }
                "definition" => {
                    // Process description nodes
                    self.process_children(child, buffer, true);
                    buffer.push("\n".to_owned());
                }
                _ => {}
            }
        }

        buffer.push("\n".to_owned());
    }

    /// As células de uma linha da tabela, já como texto (vazio vira um espaço).
    fn table_row_content(&self, row: usize) -> Vec<String> {
        self.tree(row)
            .children
            .iter()
            .filter(|&&cell| is_table_cell(&self.role(cell)))
            .map(|&cell| {
                let text = get_node_text(self.tree(cell).node, false);
                if text.is_empty() { " ".to_owned() } else { text }
            })
            .collect()
    }

    fn process_table_node(&self, index: usize, buffer: &mut Vec<String>) {
        buffer.push("\n".to_owned());

        // Find rows
        let rows: Vec<usize> = self.tree(index).children.iter().copied().filter(|&child| self.role(child).contains("row")).collect();

        if let Some((&header_row, data_rows)) = rows.split_first() {
            // First row as header
            let header_content = self.table_row_content(header_row);

            // Generate header row
            buffer.push(format!("| {} |\n", header_content.join(" | ")));

            // Generate separator row
            buffer.push(format!("| {} |\n", vec!["---"; header_content.len()].join(" | ")));

            // Generate data rows
            for &row in data_rows {
                buffer.push(format!("| {} |\n", self.table_row_content(row).join(" | ")));
            }
        }

        buffer.push("\n".to_owned());
    }

    fn process_code_node(&self, index: usize, buffer: &mut Vec<String>) {
        let mut temp_buffer: Vec<String> = Vec::new();
        // Process the children of the code node
        self.process_children(index, &mut temp_buffer, false);
        let is_codeblock = temp_buffer.iter().any(|text| text.contains('\n'));
        if is_codeblock {
            buffer.push("\n```\n".to_owned());
            // Append the processed text to the buffer
            buffer.push(temp_buffer.join(""));
            buffer.push("\n```\n".to_owned());
        } else {
            buffer.push("`".to_owned());
            let mut character_count = 0;
            // Append the processed text to the buffer
            for temp_item in temp_buffer {
                character_count += js_length(&temp_item);
                if character_count > LINE_MAX_LENGTH {
                    buffer.push("\n".to_owned());
                    character_count = 0;
                }
                buffer.push(temp_item);
                buffer.push("`".to_owned());
            }
        }
    }

    fn collect_navigation_links(&self, tree: usize) -> Vec<String> {
        let mut links = Vec::new();
        self.collect_links(tree, &mut links);
        links
    }

    fn collect_links(&self, index: usize, links: &mut Vec<String>) {
        let node = self.tree(index).node;

        if self.role(index) == "link" && self.is_navigation_link(index) {
            let link_text = get_node_text(node, true);
            let url = get_link_url(node);
            let description = truthy_string(node.description.as_ref()).map(JsString::to_string_lossy).unwrap_or_default();

            let suffix = if description.is_empty() { String::new() } else { format!(" - {description}") };
            links.push(format!("- [{link_text}]({url}){suffix}"));
        }

        // Process children
        for &child in &self.tree(index).children {
            self.collect_links(child, links);
        }
    }
}
