//! Porte de `src/vs/platform/webContentExtractor/test/electron-main/cdpAccessibilityDomain.test.ts` (suíte `CDP Accessibility Domain`), caso a caso e na mesma ordem.
//!
//! Divergências: os literais de nó do original (`{ nodeId, childIds, ignored, role, name, properties }`)
//! são montados por `node(...)` e pelos métodos de [`NodeExt`], na mesma ordem de campos; `childIds`
//! ausente ou vazio dá o mesmo resultado (um nó sem filhos). `createAXValue(type, value)` e
//! `createAXProperty(name, value, type)` mantêm os nomes e os parâmetros, com o valor como
//! [`Dynamic`]. `assert.strictEqual(result.includes(x), true)` é `assert!(result.contains(x))`.
//! `ensureNoDisposablesAreLeakedInTestSuite` não tem o que vigiar (a conversão não cria nada descartável).

use je_base::dynamic::Dynamic;
use je_base::uri::URI;
use je_platform::web_content_extractor::cdp_accessibility_domain::{
    AXNode, AXProperty, AXPropertyName, AXValue, AXValueType, convert_ax_tree_to_markdown,
};

fn test_uri() -> URI {
    URI::parse("https://example.com/test", false)
}

fn create_ax_value(type_: AXValueType, value: Dynamic) -> AXValue {
    AXValue { type_, value: Some(value), related_nodes: None, sources: None }
}

fn create_ax_property(name: AXPropertyName, value: Dynamic, type_: AXValueType) -> AXProperty {
    AXProperty { name, value: create_ax_value(type_, value) }
}

fn string_value(text: &str) -> Dynamic {
    Dynamic::string(text)
}

fn level(value: f64) -> AXProperty {
    create_ax_property(AXPropertyName::Level, Dynamic::Number(value), AXValueType::Integer)
}

fn url(value: &str) -> AXProperty {
    create_ax_property(AXPropertyName::Url, string_value(value), AXValueType::String)
}

/// Um nó não ignorado com o papel dado.
fn node(node_id: &str, role: &str) -> AXNode {
    AXNode { node_id: node_id.to_owned(), ignored: false, role: Some(create_ax_value(AXValueType::Role, string_value(role))), ..AXNode::default() }
}

trait NodeExt {
    fn name(self, name: &str) -> Self;
    fn children(self, child_ids: &[&str]) -> Self;
    fn properties(self, properties: Vec<AXProperty>) -> Self;
}

impl NodeExt for AXNode {
    fn name(mut self, name: &str) -> Self {
        self.name = Some(create_ax_value(AXValueType::String, string_value(name)));
        self
    }

    fn children(mut self, child_ids: &[&str]) -> Self {
        self.child_ids = Some(child_ids.iter().map(|id| (*id).to_owned()).collect());
        self
    }

    fn properties(mut self, properties: Vec<AXProperty>) -> Self {
        self.properties = Some(properties);
        self
    }
}

#[test]
fn empty_tree_returns_empty_string() {
    let result = convert_ax_tree_to_markdown(&test_uri(), &[]);
    assert_eq!(result, "");
}

//#region Heading Tests

#[test]
fn simple_heading_conversion() {
    let nodes = vec![
        node("node1", "heading").children(&["node2"]).name("Test Heading").properties(vec![level(2.0)]),
        node("node2", "StaticText").children(&[]).name("Test Heading"),
    ];

    let result = convert_ax_tree_to_markdown(&test_uri(), &nodes);
    assert_eq!(result.trim(), "## Test Heading");
}

//#endregion

//#region Paragraph Tests

#[test]
fn paragraph_with_text_conversion() {
    let nodes = vec![node("node1", "paragraph").children(&["node2"]), node("node2", "StaticText").name("This is a paragraph of text.")];

    let result = convert_ax_tree_to_markdown(&test_uri(), &nodes);
    assert_eq!(result.trim(), "This is a paragraph of text.");
}

#[test]
fn really_long_paragraph_should_insert_newlines_at_the_space_before_80_characters() {
    let long_str = [
        "This is a paragraph of text. It is really long. Like really really really really",
        "really really really really really really really long. That long.",
    ];

    let nodes = vec![node("node2", "StaticText").name(&long_str.join(" "))];

    let result = convert_ax_tree_to_markdown(&test_uri(), &nodes);
    assert_eq!(result.trim(), long_str.join("\n"));
}

//#endregion

//#region List Tests

#[test]
fn list_conversion() {
    let nodes = vec![
        node("node1", "list").children(&["node2", "node3"]),
        node("node2", "listitem").children(&["node4", "node6"]),
        node("node3", "listitem").children(&["node5", "node7"]),
        node("node4", "ListMarker").name("1. "),
        node("node5", "ListMarker").name("2. "),
        node("node6", "StaticText").name("Item 1"),
        node("node7", "StaticText").name("Item 2"),
    ];

    let result = convert_ax_tree_to_markdown(&test_uri(), &nodes);
    let expected = "
1. Item 1
2. Item 2

";
    assert_eq!(result, expected);
}

#[test]
fn nested_list_conversion() {
    let nodes = vec![
        node("list1", "list").children(&["item1", "item2"]),
        node("item1", "listitem").children(&["marker1", "text1", "nestedList"]).properties(vec![level(1.0)]),
        node("marker1", "ListMarker").name("- "),
        node("text1", "StaticText").name("Item 1"),
        node("nestedList", "list").children(&["nestedItem"]),
        node("nestedItem", "listitem").children(&["nestedMarker", "nestedText"]).properties(vec![level(2.0)]),
        node("nestedMarker", "ListMarker").name("- "),
        node("nestedText", "StaticText").name("Item 1a"),
        node("item2", "listitem").children(&["marker2", "text2"]).properties(vec![level(1.0)]),
        node("marker2", "ListMarker").name("- "),
        node("text2", "StaticText").name("Item 2"),
    ];

    let result = convert_ax_tree_to_markdown(&test_uri(), &nodes);
    let indent = "  ";
    let expected = format!(
        "
- Item 1
{indent}- Item 1a
- Item 2

"
    );
    assert_eq!(result, expected);
}

//#endregion

//#region Links Tests

#[test]
fn links_conversion() {
    let nodes = vec![
        node("node1", "paragraph").children(&["node2"]),
        node("node2", "link").name("Test Link").properties(vec![url("https://test.com")]),
    ];

    let result = convert_ax_tree_to_markdown(&test_uri(), &nodes);
    assert_eq!(result.trim(), "[Test Link](https://test.com)");
}

#[test]
fn links_to_same_page_are_not_converted_to_markdown_links() {
    let page_uri = URI::parse("https://example.com/page", false);
    let nodes = vec![node("link", "link").name("Current page link").properties(vec![url("https://example.com/page?section=1#header")])];

    let result = convert_ax_tree_to_markdown(&page_uri, &nodes);
    assert!(result.contains("Current page link"));
    assert!(!result.contains("[Current page link]"));
}

//#endregion

//#region Image Tests

#[test]
fn image_conversion() {
    let nodes = vec![node("node1", "image").name("Alt text").properties(vec![url("https://test.com/image.png")])];

    let result = convert_ax_tree_to_markdown(&test_uri(), &nodes);
    assert_eq!(result.trim(), "![Alt text](https://test.com/image.png)");
}

#[test]
fn image_without_url_shows_alt_text() {
    let nodes = vec![node("node1", "image").name("Alt text")];

    let result = convert_ax_tree_to_markdown(&test_uri(), &nodes);
    assert_eq!(result.trim(), "[Image: Alt text]");
}

//#endregion

//#region Description List Tests

#[test]
fn description_list_conversion() {
    let nodes = vec![
        node("dl", "DescriptionList").children(&["term1", "def1", "term2", "def2"]),
        node("term1", "term").children(&["termText1"]),
        node("termText1", "StaticText").name("Term 1"),
        node("def1", "definition").children(&["defText1"]),
        node("defText1", "StaticText").name("Definition 1"),
        node("term2", "term").children(&["termText2"]),
        node("termText2", "StaticText").name("Term 2"),
        node("def2", "definition").children(&["defText2"]),
        node("defText2", "StaticText").name("Definition 2"),
    ];

    let result = convert_ax_tree_to_markdown(&test_uri(), &nodes);
    assert!(result.contains("- **Term 1** Definition 1"));
    assert!(result.contains("- **Term 2** Definition 2"));
}

//#endregion

//#region Blockquote Tests

#[test]
fn blockquote_conversion() {
    let nodes = vec![node("node1", "blockquote").name("This is a blockquote\nWith multiple lines")];

    let result = convert_ax_tree_to_markdown(&test_uri(), &nodes);
    let expected = "> This is a blockquote
> With multiple lines";
    assert_eq!(result.trim(), expected);
}

//#endregion

//#region Code Tests

#[test]
fn preformatted_text_conversion() {
    let nodes = vec![node("node1", "pre").name("function test() {\n  return true;\n}")];

    let result = convert_ax_tree_to_markdown(&test_uri(), &nodes);
    let expected = "```\nfunction test() {\n  return true;\n}\n```";
    assert_eq!(result.trim(), expected);
}

#[test]
fn code_block_conversion() {
    let nodes = vec![node("code", "code").children(&["codeText"]), node("codeText", "StaticText").name("const x = 42;\nconsole.log(x);")];

    let result = convert_ax_tree_to_markdown(&test_uri(), &nodes);
    assert!(result.contains("```"));
    assert!(result.contains("const x = 42;"));
    assert!(result.contains("console.log(x);"));
}

#[test]
fn inline_code_conversion() {
    let nodes = vec![node("code", "code").children(&["codeText"]), node("codeText", "StaticText").name("const x = 42;")];

    let result = convert_ax_tree_to_markdown(&test_uri(), &nodes);
    assert!(result.contains("`const x = 42;`"));
}

//#endregion

//#region Table Tests

#[test]
fn table_conversion() {
    let nodes = vec![
        node("table1", "table").children(&["row1", "row2"]),
        node("row1", "row").children(&["cell1", "cell2"]),
        node("row2", "row").children(&["cell3", "cell4"]),
        node("cell1", "cell").name("Header 1"),
        node("cell2", "cell").name("Header 2"),
        node("cell3", "cell").name("Data 1"),
        node("cell4", "cell").name("Data 2"),
    ];

    let result = convert_ax_tree_to_markdown(&test_uri(), &nodes);
    let expected = "
| Header 1 | Header 2 |
| --- | --- |
| Data 1 | Data 2 |
";
    assert_eq!(result.trim(), expected.trim());
}

#[test]
fn table_with_columnheader_role_th_elements() {
    let nodes = vec![
        node("table1", "table").children(&["row1", "row2"]),
        node("row1", "row").children(&["header1", "header2"]),
        node("row2", "row").children(&["cell3", "cell4"]),
        node("header1", "columnheader").name("Header 1"),
        node("header2", "columnheader").name("Header 2"),
        node("cell3", "cell").name("Data 1"),
        node("cell4", "cell").name("Data 2"),
    ];

    let result = convert_ax_tree_to_markdown(&test_uri(), &nodes);
    let expected = "
| Header 1 | Header 2 |
| --- | --- |
| Data 1 | Data 2 |
";
    assert_eq!(result.trim(), expected.trim());
}

#[test]
fn table_with_rowheader_role() {
    let nodes = vec![
        node("table1", "table").children(&["row1", "row2"]),
        node("row1", "row").children(&["rowheader1", "cell2"]),
        node("row2", "row").children(&["rowheader2", "cell4"]),
        node("rowheader1", "rowheader").name("Row 1"),
        node("cell2", "cell").name("Data 1"),
        node("rowheader2", "rowheader").name("Row 2"),
        node("cell4", "cell").name("Data 2"),
    ];

    let result = convert_ax_tree_to_markdown(&test_uri(), &nodes);
    let expected = "
| Row 1 | Data 1 |
| --- | --- |
| Row 2 | Data 2 |
";
    assert_eq!(result.trim(), expected.trim());
}

#[test]
fn table_with_mixed_cell_types() {
    let nodes = vec![
        node("table1", "table").children(&["row1", "row2", "row3"]),
        node("row1", "row").children(&["header1", "header2", "header3"]),
        node("row2", "row").children(&["rowheader1", "cell2", "cell3"]),
        node("row3", "row").children(&["rowheader2", "cell4", "cell5"]),
        node("header1", "columnheader").name("Name"),
        node("header2", "columnheader").name("Age"),
        node("header3", "columnheader").name("City"),
        node("rowheader1", "rowheader").name("John"),
        node("cell2", "cell").name("25"),
        node("cell3", "cell").name("NYC"),
        node("rowheader2", "rowheader").name("Jane"),
        node("cell4", "cell").name("30"),
        node("cell5", "cell").name("LA"),
    ];

    let result = convert_ax_tree_to_markdown(&test_uri(), &nodes);
    let expected = "
| Name | Age | City |
| --- | --- | --- |
| John | 25 | NYC |
| Jane | 30 | LA |
";
    assert_eq!(result.trim(), expected.trim());
}

#[test]
fn table_with_gridcell_role() {
    let nodes = vec![
        node("table1", "table").children(&["row1", "row2"]),
        node("row1", "row").children(&["cell1", "cell2"]),
        node("row2", "row").children(&["cell3", "cell4"]),
        node("cell1", "gridcell").name("Header 1"),
        node("cell2", "gridcell").name("Header 2"),
        node("cell3", "gridcell").name("Data 1"),
        node("cell4", "gridcell").name("Data 2"),
    ];

    let result = convert_ax_tree_to_markdown(&test_uri(), &nodes);
    let expected = "
| Header 1 | Header 2 |
| --- | --- |
| Data 1 | Data 2 |
";
    assert_eq!(result.trim(), expected.trim());
}

//#endregion
