//! Porte de `src/vs/platform/terminal/test/common/terminalProfiles.test.ts` (suíte `terminalProfiles`), caso a
//! caso e na mesma ordem.
//!
//! Divergências: `ensureNoDisposablesAreLeakedInTestSuite` não tem o que vigiar (nada aqui é descartável).
//! `Codicon.zap` é o `ThemeIcon` de id `zap` (o `je-platform` não depende do `je-codicons`). O resultado do
//! `deepStrictEqual` é o [`ProfileSchemaEnums`], com o `null` dos valores como `None`.

use je_base::arrays::OneOrMany;
use je_base::themables::ThemeIcon;
use je_platform::terminal::terminal::{ITerminalProfile, TerminalEnvironmentValue, TerminalIcon};
use je_platform::terminal::terminal_profiles::{ProfileSchemaEnums, create_profile_schema_enums};

fn enums(values: &[Option<&str>], markdown_descriptions: &[&str]) -> ProfileSchemaEnums {
    ProfileSchemaEnums {
        values: values.iter().map(|value| value.map(str::to_owned)).collect(),
        markdown_descriptions: markdown_descriptions.iter().map(|description| (*description).to_owned()).collect(),
    }
}

// suite('createProfileSchemaEnums')

#[test]
fn create_profile_schema_enums_should_return_an_empty_array_when_there_are_no_profiles() {
    assert_eq!(create_profile_schema_enums(&[], None), enums(&[None], &["Automatically detect the default"]));
}

#[test]
fn create_profile_schema_enums_should_return_a_single_entry_when_there_is_one_profile() {
    let profile = ITerminalProfile { profile_name: "name".into(), path: "path".into(), is_default: true, ..Default::default() };
    assert_eq!(
        create_profile_schema_enums(&[profile], None),
        enums(&[None, Some("name")], &["Automatically detect the default", "$(terminal) name\n- path: path"])
    );
}

#[test]
fn create_profile_schema_enums_should_show_all_profile_information() {
    let profile = ITerminalProfile {
        profile_name: "name".into(),
        path: "path".into(),
        is_default: true,
        args: Some(OneOrMany::Many(vec!["a".into(), "b".into()])),
        color: Some("terminal.ansiRed".into()),
        env: Some(vec![
            ("c".into(), TerminalEnvironmentValue::Value("d".into())),
            ("e".into(), TerminalEnvironmentValue::Value("f".into())),
        ]),
        icon: Some(TerminalIcon::Theme(ThemeIcon::from_id("zap"))),
        override_name: Some(true),
        ..Default::default()
    };
    assert_eq!(
        create_profile_schema_enums(&[profile], None),
        enums(
            &[None, Some("name")],
            &[
                "Automatically detect the default",
                "$(zap) name\n- path: path\n- args: ['a','b']\n- overrideName: true\n- color: terminal.ansiRed\n- env: {\"c\":\"d\",\"e\":\"f\"}"
            ]
        )
    );
}

#[test]
fn create_profile_schema_enums_should_return_a_multiple_entries_when_there_are_multiple_profiles() {
    let profile1 = ITerminalProfile { profile_name: "name".into(), path: "path".into(), is_default: true, ..Default::default() };
    let profile2 = ITerminalProfile { profile_name: "foo".into(), path: "bar".into(), is_default: false, ..Default::default() };
    assert_eq!(
        create_profile_schema_enums(&[profile1, profile2], None),
        enums(
            &[None, Some("name"), Some("foo")],
            &["Automatically detect the default", "$(terminal) name\n- path: path", "$(terminal) foo\n- path: bar"]
        )
    );
}
