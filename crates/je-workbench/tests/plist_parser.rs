//! Testes do `parse` (`plistParser.ts`). O oráculo não tem teste dele: estes casos cobrem
//! dict, array, string, integer, real, true/false, entidades XML, comentários, as mensagens de
//! erro e um `.tmTheme` mínimo.

use je_workbench::services::themes::plist_parser::parse;
use serde_json::{json, Value};

fn wrap(body: &str) -> String {
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n<plist version=\"1.0\">{body}</plist>"
    )
}

fn error_of(content: &str) -> String {
    parse(content).expect_err("a entrada deveria falhar").message
}

#[test]
fn parses_dict_with_scalars() {
    let value = parse(&wrap(
        "<dict><key>a</key><string>x</string><key>n</key><integer>42</integer><key>r</key><real>1.5</real>\
         <key>t</key><true/><key>f</key><false/></dict>",
    ))
    .unwrap();
    assert_eq!(value, json!({"a": "x", "n": 42, "r": 1.5, "t": true, "f": false}));
}

#[test]
fn preserves_key_order() {
    let value = parse("<dict><key>z</key><string>1</string><key>a</key><string>2</string></dict>").unwrap();
    let keys: Vec<&String> = value.as_object().unwrap().keys().collect();
    assert_eq!(keys, ["z", "a"]);
}

#[test]
fn parses_nested_array_and_dict() {
    let value = parse(
        "<dict><key>list</key><array><string>a</string><integer>-3</integer><dict><key>k</key><array/></dict>\
         <array><true/></array></array><key>empty</key><dict/></dict>",
    )
    .unwrap();
    assert_eq!(value, json!({"list": ["a", -3, {"k": []}, [true]], "empty": {}}));
}

#[test]
fn root_can_be_array_or_scalar() {
    assert_eq!(parse("<array><string>a</string></array>").unwrap(), json!(["a"]));
    assert_eq!(parse("<string>solto</string>").unwrap(), json!("solto"));
    assert_eq!(parse("").unwrap(), Value::Null);
}

#[test]
fn decodes_entities() {
    let value = parse(
        "<dict><key>a&amp;b</key><string>&lt;x&gt; &quot;q&quot; &apos;s&apos; &#65;&#x42;&#xe9; &#128512;</string></dict>",
    )
    .unwrap();
    assert_eq!(value, json!({"a&b": "<x> \"q\" 's' ABé \u{1F600}"}));
}

#[test]
fn decodes_entities_in_sequence_like_the_original() {
    // `&amp;#65;` vira `&#65;` na terceira passada e não é decodificado de novo.
    let value = parse("<string>&amp;#65;</string>").unwrap();
    assert_eq!(value, json!("&#65;"));
    // `&#38;amp;` vira `&amp;` na primeira passada e `&` na terceira.
    assert_eq!(parse("<string>&#38;amp;</string>").unwrap(), json!("&"));
}

#[test]
fn skips_comments_declarations_and_bom() {
    let value = parse("\u{FEFF}<!-- um comentário <dict> --><dict><!-- outro --><key>a</key><string>b</string></dict>").unwrap();
    assert_eq!(value, json!({"a": "b"}));
}

#[test]
fn empty_string_and_closed_tags() {
    let value = parse("<dict><key>a</key><string/><key>b</key><string></string></dict>").unwrap();
    assert_eq!(value, json!({"a": "", "b": ""}));
}

#[test]
fn parses_number_prefixes_like_js() {
    let value = parse("<array><real> 2.5e2abc</real><integer>12px</integer><real>.5</real><real>-0.25</real></array>").unwrap();
    assert_eq!(value, json!([250, 12, 0.5, -0.25]));
}

#[test]
fn parses_date_as_iso_string() {
    let value = parse("<array><date>2020-01-02T03:04:05Z</date><date>nada</date></array>").unwrap();
    assert_eq!(value, json!(["2020-01-02T03:04:05.000Z", null]));
}

#[test]
fn keeps_data_text() {
    assert_eq!(parse("<array><data>QUJD</data></array>").unwrap(), json!(["QUJD"]));
}

#[test]
fn errors_carry_offset_and_context() {
    assert_eq!(error_of("x"), "Near offset 1: expected < ~~~~~~");
    assert_eq!(error_of("<"), "Near offset 1: unexpected end of input ~~~~~~");
    assert_eq!(error_of("</foo>"), "Near offset 2: unexpected closed tag ~~~foo>~~~");
    assert_eq!(error_of("<bogus>"), "Near offset 7: unexpected opened tag bogus ~~~~~~");
    assert_eq!(error_of("<string>a</string>x"), "Near offset 19: expected < ~~~~~~");
}

#[test]
fn structural_errors() {
    assert!(error_of("<dict><string>a</string></dict>").starts_with("Near offset 24: missing <key> ~~~"));
    assert!(error_of("<dict><key>a</key><key>b</key></dict>").contains("too many <key>"));
    assert!(error_of("<array><key>a</key></array>").contains("unexpected <key>"));
    assert!(error_of("<key>a</key>").contains("unexpected <key>"));
    assert!(error_of("<array></dict>").contains("unexpected </dict>"));
    assert!(error_of("<dict></array>").contains("unexpected </array>"));
    assert!(error_of("</dict>").contains("unexpected </dict>"));
    assert!(error_of("<dict><dict/></dict>").contains("missing <key>"));
    assert!(error_of("<real>abc</real>").contains("cannot parse float"));
    assert!(error_of("<integer>abc</integer>").contains("cannot parse integer"));
}

#[test]
fn error_context_is_limited_to_fifty_units() {
    let long = format!("<bogus>{}", "z".repeat(80));
    let message = error_of(&long);
    assert_eq!(message, "Near offset 7: unexpected opened tag bogus ~~~".to_string() + &"z".repeat(50) + "~~~");
}

#[test]
fn offsets_count_utf16_units() {
    // O `😀` ocupa duas unidades UTF-16.
    let message = error_of("<string>😀</string>x");
    assert_eq!(message, "Near offset 20: expected < ~~~~~~");
}

#[test]
fn truncated_input_returns_innermost_open_container() {
    let value = parse("<dict><key>a</key><dict><key>b</key><string>c</string>").unwrap();
    assert_eq!(value, json!({"b": "c"}));
}

#[test]
fn parses_minimal_tm_theme() {
    let theme = r##"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
	<key>name</key>
	<string>Minimal &amp; Clean</string>
	<key>settings</key>
	<array>
		<dict>
			<key>settings</key>
			<dict>
				<key>background</key>
				<string>#1E1E1E</string>
				<key>foreground</key>
				<string>#D4D4D4</string>
			</dict>
		</dict>
		<dict>
			<key>name</key>
			<string>Comment</string>
			<key>scope</key>
			<string>comment, punctuation.definition.comment</string>
			<key>settings</key>
			<dict>
				<key>fontStyle</key>
				<string>italic</string>
				<key>foreground</key>
				<string>#6A9955</string>
			</dict>
		</dict>
	</array>
	<key>uuid</key>
	<string>00000000-0000-0000-0000-000000000000</string>
</dict>
</plist>
"##;
    let value = parse(theme).unwrap();
    assert_eq!(
        value,
        json!({
            "name": "Minimal & Clean",
            "settings": [
                {"settings": {"background": "#1E1E1E", "foreground": "#D4D4D4"}},
                {
                    "name": "Comment",
                    "scope": "comment, punctuation.definition.comment",
                    "settings": {"fontStyle": "italic", "foreground": "#6A9955"}
                }
            ],
            "uuid": "00000000-0000-0000-0000-000000000000"
        })
    );
}
