//! Porte de `src/vs/platform/contextkey/test/common/parser.test.ts` (suíte `Context Key Parser`), caso a caso e na mesma ordem.

use je_platform::contextkey::{Parser, ParserConfig};

fn parse_to_str(input: &str) -> String {
    let mut parser = Parser::new(ParserConfig::default());
    let mut prints: Vec<String> = Vec::new();

    let expr = parser.parse(input);
    match expr {
        None => {
            if !parser.lexing_errors().is_empty() {
                prints.push("Lexing errors:".into());
                prints.push("\n\n".into());
                for lexing_error in parser.lexing_errors() {
                    prints.push(format!(
                        "Unexpected token '{}' at offset {}. {}",
                        lexing_error.lexeme,
                        lexing_error.offset,
                        lexing_error.additional_info.as_deref().unwrap_or("undefined")
                    ));
                    prints.push("\n".into());
                }
            }
            if !parser.parsing_errors().is_empty() {
                if !parser.lexing_errors().is_empty() {
                    prints.push("\n --- \n".into());
                }
                prints.push("Parsing errors:".into());
                prints.push("\n\n".into());
                for parsing_error in parser.parsing_errors() {
                    prints.push(format!("Unexpected '{}' at offset {}.", parsing_error.lexeme, parsing_error.offset));
                    prints.push("\n".into());
                }
            }
        }
        Some(expr) => prints.push(expr.serialize()),
    }
    prints.concat()
}

#[test]
fn foo() {
    assert_eq!(parse_to_str(" foo"), "foo");
}

#[test]
fn bang_foo() {
    assert_eq!(parse_to_str("!foo"), "!foo");
}

#[test]
fn foo_matches_bar() {
    assert_eq!(parse_to_str("foo =~ /bar/"), "foo =~ /bar/");
}

#[test]
fn foo_or_foo_matches_bar_and_baz() {
    assert_eq!(parse_to_str("foo || (foo =~ /bar/ && baz)"), "foo || baz && foo =~ /bar/");
}

#[test]
fn foo_or_foo_matches_bar_or_baz() {
    assert_eq!(parse_to_str("foo || (foo =~ /bar/ || baz)"), "baz || foo || foo =~ /bar/");
}

#[test]
fn foo_or_bar_and_jee_or_jar() {
    assert_eq!(parse_to_str("(foo || bar) && (jee || jar)"), "bar && jar || bar && jee || foo && jar || foo && jee");
}

#[test]
fn foo_and_foo_matches_zee_i() {
    assert_eq!(parse_to_str("foo && foo =~ /zee/i"), "foo && foo =~ /zee/i");
}

#[test]
fn foo_bar_enabled() {
    assert_eq!(parse_to_str("foo.bar==enabled"), "foo.bar == 'enabled'");
}

#[test]
fn foo_bar_quoted_enabled() {
    assert_eq!(parse_to_str("foo.bar == 'enabled'"), "foo.bar == 'enabled'");
}

#[test]
fn foo_bar_zed_completed_equality_with_no_space() {
    assert_eq!(parse_to_str("foo.bar:zed==completed"), "foo.bar:zed == 'completed'");
}

#[test]
fn a_and_b_or_c() {
    assert_eq!(parse_to_str("a && b || c"), "c || a && b");
}

#[test]
fn foo_bar_and_baz_jar_and_fee_bee_k_loo_1() {
    assert_eq!(parse_to_str("fooBar && baz.jar && fee.bee<K-loo+1>"), "baz.jar && fee.bee<K-loo+1> && fooBar");
}

#[test]
fn foo_bar_baz_c_r_less_than_2() {
    assert_eq!(parse_to_str("foo.barBaz<C-r> < 2"), "foo.barBaz<C-r> < 2");
}

#[test]
fn foo_bar_greater_equals_minus_1() {
    assert_eq!(parse_to_str("foo.bar >= -1"), "foo.bar >= -1");
}

#[test]
fn key_contains_nbsp_view_vsc_packages_activitybar_folders_and_vsc_packages_folders_loaded() {
    assert_eq!(
        parse_to_str("view == vsc-packages-activitybar-folders && vsc-packages-folders-loaded"),
        "vsc-packages-folders-loaded && view == 'vsc-packages-activitybar-folders'"
    );
}

#[test]
fn foo_bar_less_equals_minus_1() {
    assert_eq!(parse_to_str("foo.bar <= -1"), "foo.bar <= -1");
}

#[test]
fn cmake_hide_build_command_and_cmake_enable_full_feature_set() {
    assert_eq!(parse_to_str("!cmake:hideBuildCommand \u{26}\u{26} cmake:enableFullFeatureSet"), "cmake:enableFullFeatureSet && !cmake:hideBuildCommand");
}

#[test]
fn not_foo_and_bar() {
    assert_eq!(parse_to_str("!(foo && bar)"), "!bar || !foo");
}

#[test]
fn not_foo_and_bar_or_boar_or_deer() {
    assert_eq!(parse_to_str("!(foo && bar || boar) || deer"), "deer || !bar && !boar || !boar && !foo");
}

#[test]
fn not_not_foo() {
    assert_eq!(parse_to_str("!(!foo)"), "foo");
}

mod controversial {
    use super::*;

    #[test]
    fn debug_state_equals_double_quoted_stopped() {
        assert_eq!(parse_to_str("debugState == \"stopped\""), "debugState == '\"stopped\"'");
    }

    #[test]
    fn view_item_equals_vscode_workspace() {
        assert_eq!(parse_to_str(" viewItem == VSCode WorkSpace"), "Parsing errors:\n\nUnexpected 'WorkSpace' at offset 20.\n");
    }
}

mod regex {
    use super::*;

    #[test]
    fn resource_matches_regex_with_unescaped_slashes_foo() {
        assert_eq!(
            parse_to_str("resource =~ //foo/(barr|door/(Foo-Bar%20Templates|Soo%20Looo)|Web%20Site%Jjj%20Llll)(/.*)*$/"),
            "resource =~ /\\/foo\\/(barr|door\\/(Foo-Bar%20Templates|Soo%20Looo)|Web%20Site%Jjj%20Llll)(\\/.*)*$/"
        );
    }

    #[test]
    fn resource_matches_regex_with_unescaped_slashes_scratch() {
        assert_eq!(
            parse_to_str("resource =~ /((/scratch/(?!update)(.*)/)|((/src/).*/)).*$/"),
            "resource =~ /((\\/scratch\\/(?!update)(.*)\\/)|((\\/src\\/).*\\/)).*$/"
        );
    }

    #[test]
    fn resource_path_matches_md_yml_txt_giym() {
        // No original, `\.` num template literal é só `.`.
        assert_eq!(parse_to_str("resourcePath =~ /.md(.yml|.txt)*$/giym"), "resourcePath =~ /.md(.yml|.txt)*$/im");
    }
}

mod error_handling {
    use super::*;

    #[test]
    fn slash_foo() {
        assert_eq!(
            parse_to_str("/foo"),
            "Lexing errors:\n\nUnexpected token '/foo' at offset 0. Did you forget to escape the '/' (slash) character? Put two backslashes before it to escape, e.g., '\\\\/'.\n\n --- \nParsing errors:\n\nUnexpected '/foo' at offset 0.\n"
        );
    }

    #[test]
    fn not_b_equals_true() {
        assert_eq!(parse_to_str("!b == 'true'"), "Parsing errors:\n\nUnexpected '==' at offset 3.\n");
    }

    #[test]
    fn not_foo_and_and_in_bar() {
        assert_eq!(parse_to_str("!foo &&  in bar"), "Parsing errors:\n\nUnexpected 'in' at offset 9.\n");
    }

    #[test]
    fn vim_c_r_equals_1_and_vim_2_less_equals_3() {
        assert_eq!(
            parse_to_str("vim<c-r> == 1 && vim<2<=3"),
            "Lexing errors:\n\nUnexpected token '=' at offset 23. Did you mean == or =~?\n\n --- \nParsing errors:\n\nUnexpected '=' at offset 23.\n"
        );
    }

    #[test]
    fn foo_and_quote_bar() {
        assert_eq!(
            parse_to_str("foo && 'bar"),
            "Lexing errors:\n\nUnexpected token ''bar' at offset 7. Did you forget to open or close the quote?\n\n --- \nParsing errors:\n\nUnexpected ''bar' at offset 7.\n"
        );
    }

    #[test]
    fn config_foo_and_and_and_bar_matches_regex_and_not_foo() {
        assert_eq!(
            parse_to_str("config.foo &&  &&bar =~ /^foo$|^bar-foo$|^joo$|^jar$/ && !foo"),
            "Parsing errors:\n\nUnexpected '&&' at offset 15.\n"
        );
    }

    #[test]
    fn not_foo_equals_test() {
        assert_eq!(parse_to_str("!foo == 'test'"), "Parsing errors:\n\nUnexpected '==' at offset 5.\n");
    }

    #[test]
    fn not_not_foo() {
        assert_eq!(parse_to_str("!!foo"), "Parsing errors:\n\nUnexpected '!' at offset 1.\n");
    }
}
