//! Porte de `src/vs/workbench/contrib/update/test/common/updateInfoParser.test.ts`, caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` vira o `LeakGuard` criado no começo de cada caso.
//! - `JSON.stringify` vira o texto JSON compacto escrito à mão, na mesma ordem de chaves.
//! - `deepStrictEqual` com `style: undefined` e propriedade ausente vira `None` nos dois casos.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_base::dynamic::Dynamic;
use je_workbench::contrib::update::update_info_parser::{
    ParsedUpdateInfoInput, UpdateInfoButton, UpdateInfoButtonStyle, UpdateInfoFeature, parse_update_info_input,
};

/// O `setup`/`teardown` da suíte: guarda de vazamento.
fn run(body: impl FnOnce()) {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    body();
}

fn button(label: &str, command_id: &str, style: Option<UpdateInfoButtonStyle>, args: Option<Vec<Dynamic>>) -> UpdateInfoButton {
    UpdateInfoButton { label: label.to_owned(), command_id: command_id.to_owned(), args, style }
}

fn feature(icon: Option<&str>, title: &str, description: &str) -> UpdateInfoFeature {
    UpdateInfoFeature { icon: icon.map(str::to_owned), title: title.to_owned(), description: description.to_owned() }
}

fn markdown_only(markdown: &str) -> ParsedUpdateInfoInput {
    ParsedUpdateInfoInput { markdown: markdown.to_owned(), ..ParsedUpdateInfoInput::default() }
}

// suite('updateInfoParser') > suite('parseUpdateInfoInput')

#[test]
fn plain_markdown_returns_as_is_with_no_buttons() {
    run(|| {
        assert_eq!(parse_update_info_input("Hello **world**"), markdown_only("Hello **world**"));
    });
}

#[test]
fn strips_bom_prefix() {
    run(|| {
        assert_eq!(parse_update_info_input("\u{FEFF}Hello"), markdown_only("Hello"));
    });
}

#[test]
fn json_envelope_with_markdown_and_buttons() {
    run(|| {
        let input = r#"{"markdown":"$(info) New feature","buttons":[{"label":"Release Notes","commandId":"cmd.releaseNotes","style":"secondary"},{"label":"Try It","commandId":"cmd.tryIt","style":"primary","args":["arg1"]}]}"#;

        assert_eq!(
            parse_update_info_input(input),
            ParsedUpdateInfoInput {
                markdown: "$(info) New feature".to_owned(),
                buttons: Some(vec![
                    button("Release Notes", "cmd.releaseNotes", Some(UpdateInfoButtonStyle::Secondary), None),
                    button("Try It", "cmd.tryIt", Some(UpdateInfoButtonStyle::Primary), Some(vec![Dynamic::string("arg1")])),
                ]),
                ..ParsedUpdateInfoInput::default()
            }
        );
    });
}

#[test]
fn json_envelope_without_buttons() {
    run(|| {
        let input = r#"{"markdown":"Just text"}"#;
        assert_eq!(parse_update_info_input(input), markdown_only("Just text"));
    });
}

#[test]
fn json_envelope_with_invalid_json_falls_back_to_plain_markdown() {
    run(|| {
        let input = "{ broken json";
        assert_eq!(parse_update_info_input(input), markdown_only("{ broken json"));
    });
}

#[test]
fn json_envelope_without_markdown_property_falls_back_to_plain_markdown() {
    run(|| {
        let input = r#"{"buttons":[{"label":"X","commandId":"y"}]}"#;
        assert_eq!(parse_update_info_input(input), markdown_only(input));
    });
}

#[test]
fn block_frontmatter_with_buttons() {
    run(|| {
        let buttons = r#"{"buttons":[{"label":"Open","commandId":"cmd.open"}]}"#;
        let input = format!("---\n{buttons}\n---\nBody text");

        assert_eq!(
            parse_update_info_input(&input),
            ParsedUpdateInfoInput {
                markdown: "Body text".to_owned(),
                buttons: Some(vec![button("Open", "cmd.open", None, None)]),
                ..ParsedUpdateInfoInput::default()
            }
        );
    });
}

#[test]
fn block_frontmatter_with_no_body() {
    run(|| {
        let buttons = r#"{"buttons":[{"label":"Open","commandId":"cmd.open"}]}"#;
        let input = format!("---\n{buttons}\n---");

        assert_eq!(
            parse_update_info_input(&input),
            ParsedUpdateInfoInput {
                markdown: String::new(),
                buttons: Some(vec![button("Open", "cmd.open", None, None)]),
                ..ParsedUpdateInfoInput::default()
            }
        );
    });
}

#[test]
fn inline_frontmatter_with_buttons() {
    run(|| {
        let buttons = r#"{"buttons":[{"label":"Go","commandId":"cmd.go","style":"primary"}]}"#;
        let input = format!("--- {buttons} ---\nMarkdown here");

        assert_eq!(
            parse_update_info_input(&input),
            ParsedUpdateInfoInput {
                markdown: "\nMarkdown here".to_owned(),
                buttons: Some(vec![button("Go", "cmd.go", Some(UpdateInfoButtonStyle::Primary), None)]),
                ..ParsedUpdateInfoInput::default()
            }
        );
    });
}

#[test]
fn inline_frontmatter_handles_nested_json_with_braces() {
    run(|| {
        let buttons = r#"{"buttons":[{"label":"Open","commandId":"cmd.open"},{"label":"Try","commandId":"cmd.try"}]}"#;
        let input = format!("--- {buttons} ---\nBody");

        assert_eq!(
            parse_update_info_input(&input),
            ParsedUpdateInfoInput {
                markdown: "\nBody".to_owned(),
                buttons: Some(vec![button("Open", "cmd.open", None, None), button("Try", "cmd.try", None, None)]),
                ..ParsedUpdateInfoInput::default()
            }
        );
    });
}

#[test]
fn frontmatter_with_invalid_json_falls_back_to_full_text() {
    run(|| {
        let input = "---\nnot json\n---\nBody";
        assert_eq!(parse_update_info_input(input), markdown_only(input));
    });
}

#[test]
fn skips_buttons_with_missing_required_properties() {
    run(|| {
        let input = r#"{"markdown":"text","buttons":[{"label":"Valid","commandId":"cmd.valid"},{"label":"MissingCmd"},{"commandId":"cmd.missingLabel"},"not an object",null]}"#;

        assert_eq!(
            parse_update_info_input(input),
            ParsedUpdateInfoInput {
                markdown: "text".to_owned(),
                buttons: Some(vec![button("Valid", "cmd.valid", None, None)]),
                ..ParsedUpdateInfoInput::default()
            }
        );
    });
}

#[test]
fn returns_undefined_buttons_when_all_buttons_are_invalid() {
    run(|| {
        let input = r#"{"markdown":"text","buttons":[{"noLabel":true}]}"#;
        assert_eq!(parse_update_info_input(input), markdown_only("text"));
    });
}

#[test]
fn ignores_invalid_style_values() {
    run(|| {
        let input = r#"{"markdown":"text","buttons":[{"label":"X","commandId":"cmd","style":"danger"}]}"#;

        assert_eq!(
            parse_update_info_input(input),
            ParsedUpdateInfoInput {
                markdown: "text".to_owned(),
                buttons: Some(vec![button("X", "cmd", None, None)]),
                ..ParsedUpdateInfoInput::default()
            }
        );
    });
}

#[test]
fn json_envelope_with_banner_image_url_badge_title_and_features() {
    run(|| {
        let input = r#"{"markdown":"Body","bannerImageUrl":"https://example.com/banner.png","badge":"New","title":"What's New","features":[{"icon":"$(sparkle)","title":"Feature A","description":"Does A"},{"title":"Feature B","description":"Does B"}]}"#;

        assert_eq!(
            parse_update_info_input(input),
            ParsedUpdateInfoInput {
                markdown: "Body".to_owned(),
                buttons: None,
                banner_image_url: Some("https://example.com/banner.png".to_owned()),
                badge: Some("New".to_owned()),
                title: Some("What's New".to_owned()),
                features: Some(vec![feature(Some("$(sparkle)"), "Feature A", "Does A"), feature(None, "Feature B", "Does B")]),
            }
        );
    });
}

#[test]
fn block_frontmatter_with_banner_image_url_badge_title_and_features() {
    run(|| {
        let meta = r#"{"bannerImageUrl":"https://example.com/banner.png","badge":"Preview","title":"Highlights","features":[{"title":"Feature","description":"Desc"}]}"#;
        let input = format!("---\n{meta}\n---\nBody text");

        assert_eq!(
            parse_update_info_input(&input),
            ParsedUpdateInfoInput {
                markdown: "Body text".to_owned(),
                buttons: None,
                banner_image_url: Some("https://example.com/banner.png".to_owned()),
                badge: Some("Preview".to_owned()),
                title: Some("Highlights".to_owned()),
                features: Some(vec![feature(None, "Feature", "Desc")]),
            }
        );
    });
}

#[test]
fn ignores_non_string_banner_image_url_badge_and_title() {
    run(|| {
        let input = r#"{"markdown":"text","bannerImageUrl":123,"badge":{"not":"a string"},"title":["nope"]}"#;
        assert_eq!(parse_update_info_input(input), markdown_only("text"));
    });
}

#[test]
fn caps_features_at_5_entries_and_skips_invalid_ones() {
    run(|| {
        // O segundo destaque não tem description; o 7o passa do limite e é descartado.
        let input = r#"{"markdown":"text","features":[{"title":"F1","description":"D1"},{"title":"F2"},"not an object",null,{"title":"F3","description":"D3","icon":"$(star)"},{"title":"F4","description":"D4"},{"title":"F5","description":"D5"},{"title":"F6","description":"D6"},{"title":"F7","description":"D7"}]}"#;

        assert_eq!(
            parse_update_info_input(input),
            ParsedUpdateInfoInput {
                markdown: "text".to_owned(),
                features: Some(vec![
                    feature(None, "F1", "D1"),
                    feature(Some("$(star)"), "F3", "D3"),
                    feature(None, "F4", "D4"),
                    feature(None, "F5", "D5"),
                    feature(None, "F6", "D6"),
                ]),
                ..ParsedUpdateInfoInput::default()
            }
        );
    });
}

#[test]
fn returns_undefined_features_when_all_features_are_invalid() {
    run(|| {
        let input = r#"{"markdown":"text","features":[{"title":"OnlyTitle"},"string",null]}"#;
        assert_eq!(parse_update_info_input(input), markdown_only("text"));
    });
}

#[test]
fn ignores_non_string_feature_icon() {
    run(|| {
        let input = r#"{"markdown":"text","features":[{"icon":42,"title":"F","description":"D"}]}"#;

        assert_eq!(
            parse_update_info_input(input),
            ParsedUpdateInfoInput {
                markdown: "text".to_owned(),
                features: Some(vec![feature(None, "F", "D")]),
                ..ParsedUpdateInfoInput::default()
            }
        );
    });
}
