//! Porte de `src/vs/base/test/common/yaml.test.ts` (suíte `YAML Parser`), caso a caso e na mesma ordem.
//!
//! O `ensureNoDisposablesAreLeakedInTestSuite` não entra, porque o `yaml` não cria `IDisposable`.
#![allow(clippy::too_many_lines)]

use std::time::Instant;

use je_base::js_string::JsString;
use je_base::yaml::{
    MarkdownNode, ParseOptions, ScalarFormat, YamlMapNode, YamlNode, YamlParseError,
    YamlScalarNode, YamlSequenceNode, parse, parse_comma_separated_list, parse_front_matter,
};

use ScalarFormat::{Double, Folded, Literal, Plain, Single};

fn js(text: &str) -> JsString {
    JsString::from(text)
}

fn parse_with(input: &str, errors: &mut Vec<YamlParseError>, options: ParseOptions) -> Option<YamlNode> {
    parse(&js(input), errors, options).expect("sem RangeError")
}

fn parse_errs(input: &str, errors: &mut Vec<YamlParseError>) -> Option<YamlNode> {
    parse_with(input, errors, ParseOptions::default())
}

/// Helper to parse and assert no errors.
fn parse_ok(input: &str) -> Option<YamlNode> {
    let mut errors = Vec::new();
    let result = parse_errs(input, &mut errors);
    assert_eq!(errors, vec![], "Unexpected errors: {errors:?}");
    result
}

fn parse_markdown(input: &str) -> MarkdownNode {
    parse_front_matter(&js(input), &mut Vec::new(), ParseOptions::default()).expect("sem RangeError")
}

fn substring(input: &str, start: usize, end: usize) -> JsString {
    js(input).slice(start, end)
}

/// Asserts a scalar node and verifies its offsets match the raw value in the input.
fn assert_scalar_node(input: &str, scalar: &YamlScalarNode, value: &str, format: Option<ScalarFormat>) {
    assert_eq!(scalar.value, value);
    if let Some(format) = format {
        assert_eq!(scalar.format, format);
    }
    // Verify that the offsets correctly correspond to the rawValue in the input
    assert_eq!(
        substring(input, scalar.start_offset, scalar.end_offset),
        scalar.raw_value,
        "Offset mismatch: input[{}..{}] is {:?} but rawValue is {:?}",
        scalar.start_offset,
        scalar.end_offset,
        substring(input, scalar.start_offset, scalar.end_offset),
        scalar.raw_value
    );
}

fn assert_scalar<'a>(
    input: &str,
    node: impl Into<Option<&'a YamlNode>>,
    value: &str,
    format: Option<ScalarFormat>,
) {
    let node = node.into().expect("Expected a node but got undefined");
    let scalar = node.as_scalar().unwrap_or_else(|| panic!("expected scalar, got {}", node.type_name()));
    assert_scalar_node(input, scalar, value, format);
}

fn sc<'a>(input: &str, node: impl Into<Option<&'a YamlNode>>, value: &str) {
    assert_scalar(input, node, value, None);
}

fn key(input: &str, scalar: &YamlScalarNode, value: &str) {
    assert_scalar_node(input, scalar, value, None);
}

/// Asserts a map node and returns it for further assertions.
fn assert_map<'a>(node: impl Into<Option<&'a YamlNode>>, expected_key_count: usize) -> &'a YamlMapNode {
    let node = node.into().expect("Expected a node but got undefined");
    let map = node.as_map().unwrap_or_else(|| panic!("Expected map but got {}", node.type_name()));
    assert_eq!(map.properties.len(), expected_key_count, "properties");
    map
}

/// Asserts a sequence node and returns it for further assertions.
fn assert_sequence<'a>(node: impl Into<Option<&'a YamlNode>>, expected_item_count: usize) -> &'a YamlSequenceNode {
    let node = node.into().expect("Expected a node but got undefined");
    let seq = node.as_sequence().unwrap_or_else(|| panic!("Expected sequence but got {}", node.type_name()));
    assert_eq!(seq.items.len(), expected_item_count, "items");
    seq
}

fn has_code(errors: &[YamlParseError], code: &str) -> bool {
    errors.iter().any(|e| e.code == code)
}

fn scalar_of(node: &YamlNode) -> &YamlScalarNode {
    node.as_scalar().expect("scalar")
}

// -- Empty input ------------------------------------------------------------

#[test]
fn returns_undefined_for_empty_string() {
    assert_eq!(parse_ok(""), None);
}

#[test]
fn returns_undefined_for_whitespace_only_input() {
    assert_eq!(parse_ok("   "), None);
}

#[test]
fn returns_undefined_for_newline_only_input() {
    assert_eq!(parse_ok("\n\n"), None);
}

// -- Scalars ----------------------------------------------------------------

#[test]
fn unquoted_scalar() {
    let input = "hello world";
    let node = parse_ok(input);
    assert_scalar(input, node.as_ref(), "hello world", Some(Plain));
}

#[test]
fn literal_block_scalar_format() {
    let input = ["text: |", "  line one", "  line two"].join("\n");
    let node = parse_ok(&input);
    let map = assert_map(node.as_ref(), 1);
    assert_scalar(&input, &map.properties[0].value, "line one\nline two\n", Some(Literal));
}

#[test]
fn folded_block_scalar_format() {
    let input = ["text: >", "  line one", "  line two"].join("\n");
    let node = parse_ok(&input);
    let map = assert_map(node.as_ref(), 1);
    assert_scalar(&input, &map.properties[0].value, "line one line two\n", Some(Folded));
}

#[test]
fn literal_block_scalar_strip_chomping() {
    let input = ["text: |-", "  line one", "  line two"].join("\n");
    let node = parse_ok(&input);
    let map = assert_map(node.as_ref(), 1);
    assert_scalar(&input, &map.properties[0].value, "line one\nline two", Some(Literal));
}

#[test]
fn literal_block_scalar_keep_chomping() {
    let input = ["text: |+", "  line one", "  line two", ""].join("\n");
    let node = parse_ok(&input);
    let map = assert_map(node.as_ref(), 1);
    assert_scalar(&input, &map.properties[0].value, "line one\nline two\n", Some(Literal));
}

#[test]
fn folded_block_scalar_strip_chomping() {
    let input = ["text: >-", "  line one", "  line two"].join("\n");
    let node = parse_ok(&input);
    let map = assert_map(node.as_ref(), 1);
    assert_scalar(&input, &map.properties[0].value, "line one line two", Some(Folded));
}

#[test]
fn folded_block_scalar_keep_chomping() {
    let input = ["text: >+", "  line one", "  line two", ""].join("\n");
    let node = parse_ok(&input);
    let map = assert_map(node.as_ref(), 1);
    assert_scalar(&input, &map.properties[0].value, "line one line two\n", Some(Folded));
}

#[test]
fn single_quoted_scalar() {
    let input = "'hello world'";
    let node = parse_ok(input);
    assert_scalar(input, node.as_ref(), "hello world", Some(Single));
}

#[test]
fn double_quoted_scalar() {
    let input = "\"hello world\"";
    let node = parse_ok(input);
    assert_scalar(input, node.as_ref(), "hello world", Some(Double));
}

#[test]
fn double_quoted_scalar_with_escape_sequences() {
    let input = "\"hello\\nworld\"";
    let node = parse_ok(input);
    assert_scalar(input, node.as_ref(), "hello\nworld", Some(Double));
}

#[test]
fn single_quoted_scalar_with_escaped_single_quote() {
    let input = "'it''s a test'";
    let node = parse_ok(input);
    assert_scalar(input, node.as_ref(), "it's a test", Some(Single));
}

#[test]
fn scalar_offsets_are_correct() {
    let node = parse_ok("hello");
    let node = scalar_of(node.as_ref().unwrap());
    assert_eq!(node.start_offset, 0);
    assert_eq!(node.end_offset, 5);
}

// -- Block mappings ---------------------------------------------------------

#[test]
fn simple_key_value_pair() {
    let input = "name: John Doe";
    let node = parse_ok(input);
    let map = assert_map(node.as_ref(), 1);
    assert_eq!(map.properties[0].key.value, "name");
    sc(input, &map.properties[0].value, "John Doe");
}

#[test]
fn multiple_key_value_pairs() {
    let input = ["name: John Doe", "age: 30"].join("\n");
    let node = parse_ok(&input);
    let map = assert_map(node.as_ref(), 2);
    assert_eq!(map.properties[0].key.value, "name");
    sc(&input, &map.properties[0].value, "John Doe");
    assert_eq!(map.properties[1].key.value, "age");
    sc(&input, &map.properties[1].value, "30");
}

#[test]
fn nested_mappings() {
    let input = [
        "name: John Doe",
        "age: 30",
        "mother:",
        "  name: Susi Doe",
        "  age: 50",
        "  address:",
        "    street: 123 Main St",
        "    city: Example City",
    ]
    .join("\n");
    let node = parse_ok(&input);
    let map = assert_map(node.as_ref(), 3);
    assert_eq!(map.properties[0].key.value, "name");
    assert_eq!(map.properties[2].key.value, "mother");
    let mother = assert_map(&map.properties[2].value, 3);
    assert_eq!(mother.properties[0].key.value, "name");
    sc(&input, &mother.properties[0].value, "Susi Doe");
    let address = assert_map(&mother.properties[2].value, 2);
    assert_eq!(address.properties[0].key.value, "street");
    sc(&input, &address.properties[0].value, "123 Main St");
}

#[test]
fn mapping_with_quoted_keys_and_values() {
    let input = ["\"name\": 'John Doe'", "'age': \"30\""].join("\n");
    let node = parse_ok(&input);
    let map = assert_map(node.as_ref(), 2);
    assert_eq!(map.properties[0].key.format, Double);
    assert_eq!(scalar_of(&map.properties[0].value).format, Single);
}

#[test]
fn mapping_offsets() {
    let node = parse_ok("name: John").unwrap();
    assert_eq!(node.start_offset(), 0);
    assert_eq!(node.end_offset(), 10);
}

// -- Block sequences --------------------------------------------------------

#[test]
fn simple_sequence() {
    let input = ["- Apple", "- Banana", "- Cherry"].join("\n");
    let node = parse_ok(&input);
    let seq = assert_sequence(node.as_ref(), 3);
    sc(&input, &seq.items[0], "Apple");
    sc(&input, &seq.items[1], "Banana");
    sc(&input, &seq.items[2], "Cherry");
}

// Spec Example 2.4. Sequence of Mappings (229Q)
#[test]
fn spec_2_4_sequence_of_mappings_229q() {
    let input = [
        "-",
        "  name: Mark McGwire",
        "  hr:   65",
        "  avg:  0.278",
        "-",
        "  name: Sammy Sosa",
        "  hr:   63",
        "  avg:  0.288",
    ]
    .join("\n");
    let node = parse_ok(&input);
    let seq = assert_sequence(node.as_ref(), 2);

    let first = assert_map(&seq.items[0], 3);
    assert_eq!(first.properties[0].key.value, "name");
    sc(&input, &first.properties[0].value, "Mark McGwire");
    assert_eq!(first.properties[1].key.value, "hr");
    sc(&input, &first.properties[1].value, "65");
    assert_eq!(first.properties[2].key.value, "avg");
    sc(&input, &first.properties[2].value, "0.278");

    let second = assert_map(&seq.items[1], 3);
    assert_eq!(second.properties[0].key.value, "name");
    sc(&input, &second.properties[0].value, "Sammy Sosa");
    assert_eq!(second.properties[1].key.value, "hr");
    sc(&input, &second.properties[1].value, "63");
    assert_eq!(second.properties[2].key.value, "avg");
    sc(&input, &second.properties[2].value, "0.288");
}

#[test]
fn sequence_of_mappings() {
    let input = [
        "-",
        "  name: Mark McGwire",
        "  hr:   65",
        "  avg:  0.278",
        "-",
        "  name: Sammy Sosa",
        "  hr:   63",
        "  avg:  0.288",
    ]
    .join("\n");
    let node = parse_ok(&input);
    let seq = assert_sequence(node.as_ref(), 2);
    let first = assert_map(&seq.items[0], 3);
    sc(&input, &first.properties[0].value, "Mark McGwire");
    let second = assert_map(&seq.items[1], 3);
    sc(&input, &second.properties[0].value, "Sammy Sosa");
}

#[test]
fn map_of_sequences() {
    let input = [
        "american:",
        "  - Boston Red Sox",
        "  - Detroit Tigers",
        "  - New York Yankees",
        "national:",
        "  - New York Mets",
        "  - Chicago Cubs",
        "  - Atlanta Braves",
    ]
    .join("\n");
    let node = parse_ok(&input);
    let map = assert_map(node.as_ref(), 2);
    let american = assert_sequence(&map.properties[0].value, 3);
    sc(&input, &american.items[0], "Boston Red Sox");
    let national = assert_sequence(&map.properties[1].value, 3);
    sc(&input, &national.items[2], "Atlanta Braves");
}

#[test]
fn inline_mapping_after_dash() {
    let input = ["- name: Mark McGwire", "  hr: 65", "- name: Sammy Sosa", "  hr: 63"].join("\n");
    let node = parse_ok(&input);
    let seq = assert_sequence(node.as_ref(), 2);
    let first = assert_map(&seq.items[0], 2);
    sc(&input, &first.properties[0].value, "Mark McGwire");
}

// -- Flow mappings ----------------------------------------------------------

#[test]
fn simple_flow_mapping() {
    let input = "{hr: 65, avg: 0.278}";
    let node = parse_ok(input);
    let map = assert_map(node.as_ref(), 2);
    assert_eq!(map.properties[0].key.value, "hr");
    sc(input, &map.properties[0].value, "65");
    assert_eq!(map.properties[1].key.value, "avg");
    sc(input, &map.properties[1].value, "0.278");
}

#[test]
fn flow_mapping_offsets() {
    let node = parse_ok("{hr: 65}").unwrap();
    assert_eq!(node.start_offset(), 0);
    assert_eq!(node.end_offset(), 8);
}

// -- Flow sequences ---------------------------------------------------------

#[test]
fn simple_flow_sequence() {
    let input = "[Sammy Sosa  , 63, 0.288]";
    let node = parse_ok(input);
    let seq = assert_sequence(node.as_ref(), 3);
    sc(input, &seq.items[0], "Sammy Sosa");
    sc(input, &seq.items[1], "63");
    sc(input, &seq.items[2], "0.288");
}

#[test]
fn flow_sequence_with_quoted_strings() {
    let input = "[ 'Sammy Sosa', 63, 0.288]";
    let node = parse_ok(input);
    let seq = assert_sequence(node.as_ref(), 3);
    assert_scalar(input, &seq.items[0], "Sammy Sosa", Some(Single));
}

#[test]
fn flow_sequence_offsets() {
    let node = parse_ok("[a, b]").unwrap();
    assert_eq!(node.start_offset(), 0);
    assert_eq!(node.end_offset(), 6);
}

// -- Mixed structures -------------------------------------------------------

#[test]
fn object_with_scalars_arrays_inline_objects_and_arrays() {
    let input = [
        "object:",
        "    street: 123 Main St",
        "    city: \"Example City\"",
        "array:",
        "  - Boston Red Sox",
        "  - 'Detroit Tigers'",
        "inline object: {hr: 65, avg: 0.278}",
        "inline array: [ 'Sammy Sosa', 63, 0.288]",
        "bool: false",
    ]
    .join("\n");
    let node = parse_ok(&input);
    let map = assert_map(node.as_ref(), 5);

    // Nested object
    let obj = assert_map(&map.properties[0].value, 2);
    sc(&input, &obj.properties[0].value, "123 Main St");
    assert_scalar(&input, &obj.properties[1].value, "Example City", Some(Double));

    // Array
    let arr = assert_sequence(&map.properties[1].value, 2);
    sc(&input, &arr.items[0], "Boston Red Sox");
    assert_scalar(&input, &arr.items[1], "Detroit Tigers", Some(Single));

    // Inline object
    let inline_obj = assert_map(&map.properties[2].value, 2);
    sc(&input, &inline_obj.properties[0].value, "65");

    // Inline array
    let inline_arr = assert_sequence(&map.properties[3].value, 3);
    assert_scalar(&input, &inline_arr.items[0], "Sammy Sosa", Some(Single));

    // Boolean as scalar
    sc(&input, &map.properties[4].value, "false");
}

#[test]
fn arrays_of_inline_arrays() {
    let input = ["- [name        , hr, avg  ]", "- [Mark McGwire, 65, 0.278]", "- [Sammy Sosa  , 63, 0.288]"]
        .join("\n");
    let node = parse_ok(&input);
    let seq = assert_sequence(node.as_ref(), 3);

    let header = assert_sequence(&seq.items[0], 3);
    sc(&input, &header.items[0], "name");
    sc(&input, &header.items[1], "hr");
    sc(&input, &header.items[2], "avg");

    let row1 = assert_sequence(&seq.items[1], 3);
    sc(&input, &row1.items[0], "Mark McGwire");
}

// -- Comments ---------------------------------------------------------------

#[test]
fn comment_only_lines_are_ignored() {
    let input = ["# This is a comment", "name: John"].join("\n");
    let node = parse_ok(&input);
    let map = assert_map(node.as_ref(), 1);
    assert_eq!(map.properties[0].key.value, "name");
}

#[test]
fn inline_comment_after_value() {
    let input = [
        "hr: # 1998 hr ranking",
        "  - Mark McGwire",
        "  - Sammy Sosa",
        "rbi:",
        "  # 1998 rbi ranking",
        "  - Sammy Sosa",
        "  - Ken Griffey#part of the value, not a comment",
    ]
    .join("\n");
    let node = parse_ok(&input);
    let map = assert_map(node.as_ref(), 2);

    let hr = assert_sequence(&map.properties[0].value, 2);
    sc(&input, &hr.items[0], "Mark McGwire");

    let rbi = assert_sequence(&map.properties[1].value, 2);
    // '#' without leading space is part of the value
    sc(&input, &rbi.items[1], "Ken Griffey#part of the value, not a comment");
}

// -- Error handling ---------------------------------------------------------

#[test]
fn missing_value_emits_error_and_creates_empty_scalar() {
    let mut errors = Vec::new();
    let input = ["name:", "age: 30"].join("\n");
    let node = parse_errs(&input, &mut errors);
    let map = assert_map(node.as_ref(), 2);
    sc(&input, &map.properties[0].value, "");
    assert!(has_code(&errors, "missing-value"));
}

#[test]
fn duplicate_keys_emit_errors() {
    let mut errors = Vec::new();
    let input = ["name: John", "name: Jane"].join("\n");
    let node = parse_errs(&input, &mut errors);
    assert_map(node.as_ref(), 2);
    assert!(has_code(&errors, "duplicate-key"));
}

#[test]
fn duplicate_keys_allowed_with_option() {
    let mut errors = Vec::new();
    let input = ["name: John", "name: Jane"].join("\n");
    let node = parse_with(&input, &mut errors, ParseOptions { allow_duplicate_keys: true });
    assert_map(node.as_ref(), 2);
    assert_eq!(errors.len(), 0);
}

#[test]
fn wrong_indentation_emits_error_but_still_parses() {
    let mut errors = Vec::new();
    let input = ["parent:", "  child1: a", "    child2: b"].join("\n");
    let node = parse_errs(&input, &mut errors);
    assert!(node.is_some());
    // Should have produced an indentation error
    assert!(has_code(&errors, "unexpected-indentation"));
}

// -- Offset tracking --------------------------------------------------------

#[test]
fn scalar_offsets_in_mapping() {
    let node = parse_ok("key: value");
    let map = node.as_ref().unwrap().as_map().unwrap();
    assert_eq!(map.properties[0].key.start_offset, 0);
    assert_eq!(map.properties[0].key.end_offset, 3);
    let val = scalar_of(&map.properties[0].value);
    assert_eq!(val.start_offset, 5);
    assert_eq!(val.end_offset, 10);
}

#[test]
fn offsets_are_zero_based_and_end_offset_is_exclusive() {
    let node = parse_ok("\"hi\"");
    let node = scalar_of(node.as_ref().unwrap());
    assert_eq!(node.start_offset, 0);
    assert_eq!(node.end_offset, 4);
    assert_eq!(node.value, "hi");
    assert_eq!(node.raw_value, "\"hi\"");
}

#[test]
fn sequence_item_offsets() {
    let input = ["- a", "- b"].join("\n");
    let node = parse_ok(&input);
    let seq = node.as_ref().unwrap().as_sequence().unwrap();
    let first = scalar_of(&seq.items[0]);
    assert_eq!(first.start_offset, 2);
    assert_eq!(first.end_offset, 3);
}

// -- Nested sequences -------------------------------------------------------

#[test]
fn block_sequence_in_block_sequence_dash_dash() {
    let input = ["- - s1_i1", "  - s1_i2", "- s2"].join("\n");
    let node = parse_ok(&input);
    let outer = assert_sequence(node.as_ref(), 2);
    let inner = assert_sequence(&outer.items[0], 2);
    sc(&input, &inner.items[0], "s1_i1");
    sc(&input, &inner.items[1], "s1_i2");
    sc(&input, &outer.items[1], "s2");
}

#[test]
fn sequence_at_same_indent_as_parent_mapping_key() {
    let input = ["one:", "- 2", "- 3", "four: 5"].join("\n");
    let node = parse_ok(&input);
    let map = assert_map(node.as_ref(), 2);
    key(&input, &map.properties[0].key, "one");
    let seq = assert_sequence(&map.properties[0].value, 2);
    sc(&input, &seq.items[0], "2");
    sc(&input, &seq.items[1], "3");
    key(&input, &map.properties[1].key, "four");
    sc(&input, &map.properties[1].value, "5");
}

#[test]
fn sequence_indented_under_mapping_key() {
    let input = ["foo:", "  - 42", "bar:", "  - 44"].join("\n");
    let node = parse_ok(&input);
    let map = assert_map(node.as_ref(), 2);
    let seq1 = assert_sequence(&map.properties[0].value, 1);
    sc(&input, &seq1.items[0], "42");
    let seq2 = assert_sequence(&map.properties[1].value, 1);
    sc(&input, &seq2.items[0], "44");
}

// -- Multiline plain scalars ------------------------------------------------

#[test]
fn multiline_scalar_in_mapping_value() {
    let input = ["a: b", " c"].join("\n");
    let node = parse_ok(&input);
    let map = assert_map(node.as_ref(), 1);
    sc(&input, &map.properties[0].value, "b c");
}

#[test]
fn multiline_scalar_with_multiple_continuation_lines() {
    let input = ["plain:", "  This unquoted scalar", "  spans many lines."].join("\n");
    let node = parse_ok(&input);
    let map = assert_map(node.as_ref(), 1);
    sc(&input, &map.properties[0].value, "This unquoted scalar spans many lines.");
}

#[test]
fn multiline_scalar_at_top_level() {
    let input = ["a", "b", "  c", "d"].join("\n");
    let result = parse_ok(&input);
    sc(&input, result.as_ref(), "a b c d");
}

#[test]
fn multiline_scalar_with_empty_line_preserves_newline() {
    let input = ["a: val1", " val2", "", " val3"].join("\n");
    let node = parse_ok(&input);
    let map = assert_map(node.as_ref(), 1);
    // Empty line between val2 and val3 becomes \n
    sc(&input, &map.properties[0].value, "val1 val2\nval3");
}

#[test]
fn multiline_scalar_stops_at_same_indent_as_mapping() {
    let input = ["a: b", " c", "d: e"].join("\n");
    let node = parse_ok(&input);
    let map = assert_map(node.as_ref(), 2);
    sc(&input, &map.properties[0].value, "b c");
    sc(&input, &map.properties[1].value, "e");
}

#[test]
fn multiline_scalar_value_on_next_line() {
    let input = ["a:", "  b", "  c"].join("\n");
    let node = parse_ok(&input);
    let map = assert_map(node.as_ref(), 1);
    sc(&input, &map.properties[0].value, "b c");
}

#[test]
fn multiline_scalar_stops_at_comment() {
    let input = ["value1", "# a comment", "value2"].join("\n");
    // Comment terminates the scalar continuation, so value2 is not part of value1
    let result = parse_ok(&input);
    sc(&input, result.as_ref(), "value1");
}

#[test]
fn multiline_scalar_with_multiple_mappings() {
    let input = ["a: b", " c", "d:", " e", "  f"].join("\n");
    let node = parse_ok(&input);
    let map = assert_map(node.as_ref(), 2);
    sc(&input, &map.properties[0].value, "b c");
    sc(&input, &map.properties[1].value, "e f");
}

// -- Edge cases -------------------------------------------------------------

#[test]
fn colon_in_unquoted_value() {
    let input = "url: http://example.com";
    let node = parse_ok(input);
    let map = node.as_ref().unwrap().as_map().unwrap();
    sc(input, &map.properties[0].value, "http://example.com");
}

#[test]
fn trailing_whitespace_is_trimmed_from_unquoted_scalars() {
    let input = "name: John   ";
    let node = parse_ok(input);
    let map = node.as_ref().unwrap().as_map().unwrap();
    sc(input, &map.properties[0].value, "John");
}

#[test]
fn empty_flow_map() {
    let node = parse_ok("{}");
    let map = assert_map(node.as_ref(), 0);
    assert_eq!(map.start_offset, 0);
    assert_eq!(map.end_offset, 2);
}

#[test]
fn empty_flow_sequence() {
    let node = parse_ok("[]");
    let seq = assert_sequence(node.as_ref(), 0);
    assert_eq!(seq.start_offset, 0);
    assert_eq!(seq.end_offset, 2);
}

#[test]
fn crlf_line_endings() {
    let input = "name: John\r\nage: 30";
    let node = parse_ok(input);
    let map = assert_map(node.as_ref(), 2);
    sc(input, &map.properties[0].value, "John");
    sc(input, &map.properties[1].value, "30");
}

#[test]
fn multiple_document_separators_only_first_document_is_parsed() {
    let input = ["---", "key1: value1", "key2: value2", "---", "key3: value3"].join("\n");
    let node = parse_ok(&input);
    let map = assert_map(node.as_ref(), 2);
    key(&input, &map.properties[0].key, "key1");
    sc(&input, &map.properties[0].value, "value1");
    key(&input, &map.properties[1].key, "key2");
    sc(&input, &map.properties[1].value, "value2");
}

// -- Old test suite ---------------------------------------------------------

#[test]
fn mapping_value_on_next_line() {
    let input = ["name:", "  John Doe", "colors:", "  [ Red, Green, Blue ]"].join("\n");
    let node = parse_ok(&input);
    let map = assert_map(node.as_ref(), 2);
    sc(&input, &map.properties[0].value, "John Doe");
    let colors = assert_sequence(&map.properties[1].value, 3);
    sc(&input, &colors.items[0], "Red");
    sc(&input, &colors.items[1], "Green");
    sc(&input, &colors.items[2], "Blue");
}

#[test]
fn flow_map_with_different_data_types() {
    let input = "{active: true, score: 85.5, role: null}";
    let node = parse_ok(input);
    let map = assert_map(node.as_ref(), 3);
    key(input, &map.properties[0].key, "active");
    sc(input, &map.properties[0].value, "true");
    key(input, &map.properties[1].key, "score");
    sc(input, &map.properties[1].value, "85.5");
    key(input, &map.properties[2].key, "role");
    sc(input, &map.properties[2].value, "null");
}

#[test]
fn flow_map_with_quoted_keys_and_values() {
    let input = "{\"name\": \"John Doe\", \"age\": 30}";
    let node = parse_ok(input);
    let map = assert_map(node.as_ref(), 2);
    assert_scalar_node(input, &map.properties[0].key, "name", Some(Double));
    assert_scalar(input, &map.properties[0].value, "John Doe", Some(Double));
    assert_scalar_node(input, &map.properties[1].key, "age", Some(Double));
    sc(input, &map.properties[1].value, "30");
}

#[test]
fn special_characters_in_values() {
    let input = "key: value with \t special chars";
    let node = parse_ok(input);
    let map = assert_map(node.as_ref(), 1);
    sc(input, &map.properties[0].value, "value with \t special chars");
}

#[test]
fn various_whitespace_after_colon() {
    let input = "key:\t \t \t value";
    let node = parse_ok(input);
    let map = assert_map(node.as_ref(), 1);
    sc(input, &map.properties[0].value, "value");
}

#[test]
fn inline_array_with_comment_continuation() {
    let input = ["[one # comment about two", ",two, three]"].join("\n");
    let node = parse_ok(&input);
    let seq = assert_sequence(node.as_ref(), 3);
    sc(&input, &seq.items[0], "one");
    sc(&input, &seq.items[1], "two");
    sc(&input, &seq.items[2], "three");
}

#[test]
fn multi_line_flow_sequence() {
    let input = ["[", "    geen, ", "    yello, red]"].join("\n");
    let node = parse_ok(&input);
    let seq = assert_sequence(node.as_ref(), 3);
    sc(&input, &seq.items[0], "geen");
    sc(&input, &seq.items[1], "yello");
    sc(&input, &seq.items[2], "red");
}

#[test]
fn nested_block_sequences_dash_on_next_line() {
    let input = ["-", "  - Apple", "  - Banana", "  - Cherry"].join("\n");
    let node = parse_ok(&input);
    let outer = assert_sequence(node.as_ref(), 1);
    let inner = assert_sequence(&outer.items[0], 3);
    sc(&input, &inner.items[0], "Apple");
    sc(&input, &inner.items[1], "Banana");
    sc(&input, &inner.items[2], "Cherry");
}

#[test]
fn nested_flow_sequences() {
    let input = ["[", "  [ee], [ff, gg]", "]"].join("\n");
    let node = parse_ok(&input);
    let outer = assert_sequence(node.as_ref(), 2);
    let first = assert_sequence(&outer.items[0], 1);
    sc(&input, &first.items[0], "ee");
    let second = assert_sequence(&outer.items[1], 2);
    sc(&input, &second.items[0], "ff");
    sc(&input, &second.items[1], "gg");
}

#[test]
fn mapping_with_sequence_containing_a_mapping() {
    let input = ["items:", "- name: John", "  age: 30"].join("\n");
    let node = parse_ok(&input);
    let map = assert_map(node.as_ref(), 1);
    key(&input, &map.properties[0].key, "items");
    let seq = assert_sequence(&map.properties[0].value, 1);
    let item = assert_map(&seq.items[0], 2);
    sc(&input, &item.properties[0].value, "John");
    sc(&input, &item.properties[1].value, "30");
}

#[test]
fn sequence_of_mappings_with_varying_styles() {
    let input = ["-", "  name: one", "- name: two", "-", "  name: three"].join("\n");
    let node = parse_ok(&input);
    let seq = assert_sequence(node.as_ref(), 3);
    let first = assert_map(&seq.items[0], 1);
    sc(&input, &first.properties[0].value, "one");
    let second = assert_map(&seq.items[1], 1);
    sc(&input, &second.properties[0].value, "two");
    let third = assert_map(&seq.items[2], 1);
    sc(&input, &third.properties[0].value, "three");
}

#[test]
fn sequence_of_multi_property_mappings() {
    let input = [
        "products:",
        "  - name: Laptop",
        "    price: 999.99",
        "    in_stock: true",
        "  - name: Mouse",
        "    price: 25.50",
        "    in_stock: false",
    ]
    .join("\n");
    let node = parse_ok(&input);
    let map = assert_map(node.as_ref(), 1);
    let products = assert_sequence(&map.properties[0].value, 2);
    let laptop = assert_map(&products.items[0], 3);
    sc(&input, &laptop.properties[0].value, "Laptop");
    sc(&input, &laptop.properties[1].value, "999.99");
    sc(&input, &laptop.properties[2].value, "true");
    let mouse = assert_map(&products.items[1], 3);
    sc(&input, &mouse.properties[0].value, "Mouse");
    sc(&input, &mouse.properties[1].value, "25.50");
    sc(&input, &mouse.properties[2].value, "false");
}

#[test]
fn flow_sequence_with_mixed_types() {
    // Note: current parser treats all values as scalars (strings), not typed
    let input = "vals: [1, true, null, \"str\"]";
    let node = parse_ok(input);
    let map = assert_map(node.as_ref(), 1);
    let vals = assert_sequence(&map.properties[0].value, 4);
    sc(input, &vals.items[0], "1");
    sc(input, &vals.items[1], "true");
    sc(input, &vals.items[2], "null");
    assert_scalar(input, &vals.items[3], "str", Some(Double));
}

#[test]
fn flow_map_with_nested_flow_sequence() {
    let input = "config: {env: \"prod\", settings: [true, 42], debug: false}";
    let node = parse_ok(input);
    let map = assert_map(node.as_ref(), 1);
    let config = assert_map(&map.properties[0].value, 3);
    key(input, &config.properties[0].key, "env");
    assert_scalar(input, &config.properties[0].value, "prod", Some(Double));
    let settings = assert_sequence(&config.properties[1].value, 2);
    sc(input, &settings.items[0], "true");
    sc(input, &settings.items[1], "42");
    key(input, &config.properties[2].key, "debug");
    sc(input, &config.properties[2].value, "false");
}

#[test]
fn full_line_and_inline_comments() {
    let input = ["# This is a comment", "name: John Doe  # inline comment", "age: 30"].join("\n");
    let node = parse_ok(&input);
    let map = assert_map(node.as_ref(), 2);
    key(&input, &map.properties[0].key, "name");
    sc(&input, &map.properties[0].value, "John Doe");
    key(&input, &map.properties[1].key, "age");
    sc(&input, &map.properties[1].value, "30");
}

#[test]
fn unexpected_indentation_with_recovery() {
    let mut errors = Vec::new();
    let input = ["key: 1", "    stray: value"].join("\n");
    let node = parse_errs(&input, &mut errors);
    let map = assert_map(node.as_ref(), 2);
    key(&input, &map.properties[0].key, "key");
    sc(&input, &map.properties[0].value, "1");
    key(&input, &map.properties[1].key, "stray");
    sc(&input, &map.properties[1].value, "value");
    // Should report an indentation error
    assert!(has_code(&errors, "unexpected-indentation"));
}

#[test]
fn empty_value_followed_by_non_empty() {
    let input = ["empty:", "array: []"].join("\n");
    let mut errors = Vec::new();
    let node = parse_errs(&input, &mut errors);
    let map = assert_map(node.as_ref(), 2);
    key(&input, &map.properties[0].key, "empty");
    sc(&input, &map.properties[0].value, "");
    key(&input, &map.properties[1].key, "array");
    assert_sequence(&map.properties[1].value, 0);
}

#[test]
fn nested_mapping_with_empty_value() {
    let input = ["parent:", "  child:"].join("\n");
    let mut errors = Vec::new();
    let node = parse_errs(&input, &mut errors);
    let map = assert_map(node.as_ref(), 1);
    let parent = assert_map(&map.properties[0].value, 1);
    key(&input, &parent.properties[0].key, "child");
    sc(&input, &parent.properties[0].value, "");
}

#[test]
fn multiple_keys_with_empty_values() {
    let mut errors = Vec::new();
    let input = ["key1:", "key2:", "key3:"].join("\n");
    let node = parse_errs(&input, &mut errors);
    let map = assert_map(node.as_ref(), 3);
    for (i, name) in ["key1", "key2", "key3"].into_iter().enumerate() {
        key(&input, &map.properties[i].key, name);
        sc(&input, &map.properties[i].value, "");
    }
}

#[test]
fn large_input_performance() {
    let lines: Vec<String> = (0..1000).map(|i| format!("key{i}: value{i}")).collect();
    let input = lines.join("\n");
    let start = Instant::now();
    let node = parse_ok(&input);
    let duration = start.elapsed().as_millis();
    let map = assert_map(node.as_ref(), 1000);
    key(&input, &map.properties[0].key, "key0");
    key(&input, &map.properties[999].key, "key999");
    assert!(duration < 500, "Parsing took {duration}ms, expected < 500ms");
}

#[test]
fn deeply_nested_structure_performance() {
    let mut lines: Vec<String> = (0..50).map(|i| format!("{}level{i}:", "  ".repeat(i))).collect();
    lines.push(format!("{}deepValue: reached", "  ".repeat(50)));
    let input = lines.join("\n");
    let start = Instant::now();
    let mut errors = Vec::new();
    let result = parse_errs(&input, &mut errors);
    let duration = start.elapsed().as_millis();
    assert_eq!(result.as_ref().unwrap().type_name(), "map");
    assert!(duration < 500, "Parsing took {duration}ms, expected < 500ms");
}

#[test]
fn unclosed_flow_sequence_with_empty_lines() {
    let mut errors = Vec::new();
    let input = ["key: [", "", "", "", ""].join("\n");
    let node = parse_errs(&input, &mut errors);
    let map = assert_map(node.as_ref(), 1);
    key(&input, &map.properties[0].key, "key");
    let seq = map.properties[0].value.as_sequence().expect("sequence");
    assert_eq!(seq.items.len(), 0);
}

#[test]
fn deeply_nested_same_named_keys() {
    let input = ["a:", "  b:", "    a:", "      b:", "        value: test"].join("\n");
    let node = parse_ok(&input);
    let outer_a = assert_map(node.as_ref(), 1);
    key(&input, &outer_a.properties[0].key, "a");
    let outer_b = assert_map(&outer_a.properties[0].value, 1);
    key(&input, &outer_b.properties[0].key, "b");
    let inner_a = assert_map(&outer_b.properties[0].value, 1);
    key(&input, &inner_a.properties[0].key, "a");
    let inner_b = assert_map(&inner_a.properties[0].value, 1);
    key(&input, &inner_b.properties[0].key, "b");
    let leaf = assert_map(&inner_b.properties[0].value, 1);
    key(&input, &leaf.properties[0].key, "value");
    sc(&input, &leaf.properties[0].value, "test");
}

#[test]
fn flow_sequence_with_empty_lines_between_items() {
    let input = ["arr: [", "", "item1,", "", "item2", "", "]"].join("\n");
    let node = parse_ok(&input);
    let map = assert_map(node.as_ref(), 1);
    let seq = assert_sequence(&map.properties[0].value, 2);
    sc(&input, &seq.items[0], "item1");
    sc(&input, &seq.items[1], "item2");
}

#[test]
fn excessive_whitespace_after_colon() {
    let input = "key:      value";
    let node = parse_ok(input);
    let map = assert_map(node.as_ref(), 1);
    sc(input, &map.properties[0].value, "value");
}

#[test]
fn unclosed_double_quote() {
    let input = "name: \"John";
    let mut errors = Vec::new();
    let node = parse_errs(input, &mut errors);
    let map = assert_map(node.as_ref(), 1);
    key(input, &map.properties[0].key, "name");
    // Parser should recover: value should be 'John' (sans quote)
    sc(input, &map.properties[0].value, "John");
}

#[test]
fn unclosed_single_quote() {
    let input = "description: 'Hello world";
    let mut errors = Vec::new();
    let node = parse_errs(input, &mut errors);
    let map = assert_map(node.as_ref(), 1);
    key(input, &map.properties[0].key, "description");
    sc(input, &map.properties[0].value, "Hello world");
}

#[test]
fn comment_in_unclosed_flow_sequence() {
    let input = ["mode: agent", "tools: [#r"].join("\n");
    let mut errors = Vec::new();
    let node = parse_errs(&input, &mut errors);
    let map = assert_map(node.as_ref(), 2);
    key(&input, &map.properties[0].key, "mode");
    sc(&input, &map.properties[0].value, "agent");
    key(&input, &map.properties[1].key, "tools");
    let seq = map.properties[1].value.as_sequence().expect("sequence");
    assert_eq!(seq.items.len(), 0);
}

#[test]
fn duplicate_keys_emit_error() {
    let mut errors = Vec::new();
    let input = ["key: 1", "key: 2"].join("\n");
    let node = parse_errs(&input, &mut errors);
    let map = assert_map(node.as_ref(), 2);
    sc(&input, &map.properties[0].value, "1");
    sc(&input, &map.properties[1].value, "2");
    assert!(has_code(&errors, "duplicate-key"));
}

#[test]
fn duplicate_keys_allowed_via_option() {
    let mut errors = Vec::new();
    let input = ["key: 1", "key: 2"].join("\n");
    let node = parse_with(&input, &mut errors, ParseOptions { allow_duplicate_keys: true });
    assert_map(node.as_ref(), 2);
    assert_eq!(errors.len(), 0);
}

// -- parseMarkdown ----------------------------------------------------------

#[test]
fn no_frontmatter_returns_undefined_header_and_full_input_as_body() {
    let input = "Just some markdown text\nwithout frontmatter.";
    let result = parse_markdown(input);
    assert_eq!(result.header, None);
    assert_eq!(result.body, input);
}

#[test]
fn empty_input_returns_undefined_header_and_empty_body() {
    let result = parse_markdown("");
    assert_eq!(result.header, None);
    assert_eq!(result.body, "");
}

#[test]
fn frontmatter_with_body() {
    let input = ["---", "title: Hello", "author: World", "---", "# Heading", "Body text here."].join("\n");
    let result = parse_markdown(&input);
    let map = assert_map(result.header.as_ref(), 2);
    assert_eq!(scalar_of(&map.properties[0].value).value, "Hello");
    assert_eq!(scalar_of(&map.properties[1].value).value, "World");
    assert_eq!(result.get_string_value("title"), Some(js("Hello")));
    assert_eq!(result.get_string_value("author"), Some(js("World")));
    assert_eq!(result.body, "# Heading\nBody text here.");
}

#[test]
fn frontmatter_only_no_body() {
    let input = ["---", "key: value", "---"].join("\n");
    let result = parse_markdown(&input);
    let map = assert_map(result.header.as_ref(), 1);
    assert_eq!(scalar_of(&map.properties[0].value).value, "value");
    assert_eq!(result.get_string_value("key"), Some(js("value")));
    assert_eq!(result.body, "");
}

#[test]
fn empty_frontmatter_strips_delimiters() {
    let input = ["---", "---"].join("\n");
    let result = parse_markdown(&input);
    assert_eq!(result.header, None);
    assert_eq!(result.body, "");
}

#[test]
fn comment_only_frontmatter_strips_delimiters_and_preserves_body() {
    let input = ["---", "# note", "---", "Body text here."].join("\n");
    let result = parse_markdown(&input);
    assert_eq!(result.header, None);
    assert_eq!(result.body, "Body text here.");
}

#[test]
fn get_string_value_returns_the_scalar_for_a_known_key() {
    let input = ["---", "name: my-agent", "tools: foo, bar", "---", "body content"].join("\n");
    let result = parse_markdown(&input);
    assert_eq!(result.get_string_value("name"), Some(js("my-agent")));
    assert_eq!(result.get_string_array_value("tools"), Some(vec![js("foo"), js("bar")]));
}

#[test]
fn get_string_array_value_returns_array_for_a_sequence_key() {
    let input = ["---", "tags:", "  - foo", "  - bar", "  - baz", "---"].join("\n");
    let result = parse_markdown(&input);
    assert_eq!(result.get_string_array_value("tags"), Some(vec![js("foo"), js("bar"), js("baz")]));
}

#[test]
fn get_string_array_value_splits_comma_separated_scalar_into_array() {
    let input = ["---", "tags: foo, bar, baz", "---"].join("\n");
    let result = parse_markdown(&input);
    assert_eq!(result.get_string_array_value("tags"), Some(vec![js("foo"), js("bar"), js("baz")]));
}

#[test]
fn get_string_array_value_wraps_quoted_scalars_in_a_single_element_array() {
    let input = ["---", "tags: \"foo, bar\"", "---"].join("\n");
    let result = parse_markdown(&input);
    assert_eq!(result.get_string_array_value("tags"), Some(vec![js("foo, bar")]));
}

// -- parseCommaSeparatedList ------------------------------------------------

fn list(value: &str, offset: usize) -> Vec<YamlScalarNode> {
    parse_comma_separated_list(&js(value), offset).expect("sem RangeError")
}

fn values(items: &[YamlScalarNode]) -> Vec<JsString> {
    items.iter().map(|i| i.value.clone()).collect()
}

#[test]
fn empty_string_produces_empty_array() {
    assert_eq!(list("", 0), vec![]);
}

#[test]
fn single_unquoted_item() {
    let items = list("hello", 0);
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].value, "hello");
    assert_eq!(items[0].format, Plain);
}

#[test]
fn multiple_unquoted_items() {
    let items = list("foo, bar, baz", 0);
    assert_eq!(values(&items), vec![js("foo"), js("bar"), js("baz")]);
}

#[test]
fn double_quoted_items() {
    // Value is: "hello", "world"; pass it directly as a string with known offset.
    let items = list("\"hello\", \"world\"", 0);
    assert_eq!(items.len(), 2);
    assert_eq!(items[0].value, "hello");
    assert_eq!(items[0].format, Double);
    assert_eq!(items[1].value, "world");
    assert_eq!(items[1].format, Double);
}

#[test]
fn single_quoted_items() {
    let items = list("'foo', 'bar'", 0);
    assert_eq!(items.len(), 2);
    assert_eq!(items[0].value, "foo");
    assert_eq!(items[0].format, Single);
    assert_eq!(items[1].value, "bar");
    assert_eq!(items[1].format, Single);
}

#[test]
fn mixed_quoted_and_unquoted_items() {
    let items = list("plain, \"double\", 'single'", 0);
    assert_eq!(items.len(), 3);
    assert_eq!((items[0].value.clone(), items[0].format), (js("plain"), Plain));
    assert_eq!((items[1].value.clone(), items[1].format), (js("double"), Double));
    assert_eq!((items[2].value.clone(), items[2].format), (js("single"), Single));
}

#[test]
fn trailing_whitespace_trimmed_from_unquoted_items() {
    let items = list("  foo  ,  bar  ", 0);
    assert_eq!(items.len(), 2);
    assert_eq!(items[0].value, "foo");
    assert_eq!(items[1].value, "bar");
}

#[test]
fn offsets_are_relative_to_the_provided_offset() {
    let value = "a, b, c";
    let offset = 10;
    let items = list(value, offset);
    assert_eq!(items.len(), 3);
    // Each item's rawValue should appear at startOffset within `offset + value`
    let doc = format!("{}{value}", " ".repeat(offset));
    for item in &items {
        assert_eq!(substring(&doc, item.start_offset, item.end_offset), item.raw_value);
    }
}

#[test]
fn whitespace_only_string_produces_empty_array() {
    assert_eq!(list("   ", 0), vec![]);
}

// -- Divergência documentada: o RangeError de String.fromCodePoint ----------

#[test]
fn escape_with_invalid_code_point_is_a_range_error() {
    let err = parse(&js("\"\\UFFFFFFFF\""), &mut Vec::new(), ParseOptions::default()).unwrap_err();
    assert_eq!(err.to_string(), "Invalid code point 4294967295");
}
