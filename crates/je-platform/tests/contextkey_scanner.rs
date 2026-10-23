//! Porte de `src/vs/platform/contextkey/test/common/scanner.test.ts` (suíte `Context Key Scanner`), caso a caso e na mesma ordem.

use je_platform::contextkey::scanner::{Scanner, Token, TokenType};

fn token_type_to_str(token: &Token) -> &'static str {
    match token.ty {
        TokenType::LParen => "(",
        TokenType::RParen => ")",
        TokenType::Neg => "!",
        TokenType::Eq => if token.is_triple_eq { "===" } else { "==" },
        TokenType::NotEq => if token.is_triple_eq { "!==" } else { "!=" },
        TokenType::Lt => "<",
        TokenType::LtEq => "<=",
        TokenType::Gt => ">",
        TokenType::GtEq => ">=",
        TokenType::RegexOp => "=~",
        TokenType::RegexStr => "RegexStr",
        TokenType::True => "true",
        TokenType::False => "false",
        TokenType::In => "in",
        TokenType::Not => "not",
        TokenType::And => "&&",
        TokenType::Or => "||",
        TokenType::Str => "Str",
        TokenType::QuotedStr => "QuotedStr",
        TokenType::Error => "ErrorToken",
        TokenType::Eof => "EOF",
    }
}

/// `(tipo, offset, lexema)`: o lexema só existe nos tokens que o têm.
type Scanned = (&'static str, usize, Option<String>);

fn scan(input: &str) -> Vec<Scanned> {
    Scanner::new()
        .reset(input)
        .scan()
        .iter()
        .map(|token| (token_type_to_str(token), token.offset, token.lexeme.clone()))
        .collect()
}

fn t(ty: &'static str, offset: usize) -> Scanned {
    (ty, offset, None)
}

fn l(ty: &'static str, offset: usize, lexeme: &str) -> Scanned {
    (ty, offset, Some(lexeme.to_string()))
}

mod scanning_various_cases_of_context_keys {
    use super::*;

    #[test]
    fn foo_bar_c_shift_2() {
        assert_eq!(scan("foo.bar<C-shift+2>"), vec![l("Str", 0, "foo.bar<C-shift+2>"), t("EOF", 18)]);
    }

    #[test]
    fn not_foo() {
        assert_eq!(scan("!foo"), vec![t("!", 0), l("Str", 1, "foo"), t("EOF", 4)]);
    }

    #[test]
    fn foo_triple_equals_bar() {
        assert_eq!(scan("foo === bar"), vec![l("Str", 0, "foo"), t("===", 4), l("Str", 8, "bar"), t("EOF", 11)]);
    }

    #[test]
    fn foo_triple_not_equals_bar() {
        assert_eq!(scan("foo  !== bar"), vec![l("Str", 0, "foo"), t("!==", 5), l("Str", 9, "bar"), t("EOF", 12)]);
    }

    #[test]
    fn not_foo_and_bar() {
        assert_eq!(
            scan("!(foo && bar)"),
            vec![t("!", 0), t("(", 1), l("Str", 2, "foo"), t("&&", 6), l("Str", 9, "bar"), t(")", 12), t("EOF", 13)]
        );
    }

    #[test]
    fn matches_operator_alone() {
        assert_eq!(scan("=~ "), vec![t("=~", 0), t("EOF", 3)]);
    }

    #[test]
    fn foo_matches_bar() {
        assert_eq!(scan("foo =~ /bar/"), vec![l("Str", 0, "foo"), t("=~", 4), l("RegexStr", 7, "/bar/"), t("EOF", 12)]);
    }

    #[test]
    fn foo_matches_zee_i() {
        assert_eq!(scan("foo =~ /zee/i"), vec![l("Str", 0, "foo"), t("=~", 4), l("RegexStr", 7, "/zee/i"), t("EOF", 13)]);
    }

    #[test]
    fn foo_matches_zee_gm() {
        assert_eq!(scan("foo =~ /zee/gm"), vec![l("Str", 0, "foo"), t("=~", 4), l("RegexStr", 7, "/zee/gm"), t("EOF", 14)]);
    }

    #[test]
    fn foo_in_barrr() {
        assert_eq!(scan("foo in barrr  "), vec![l("Str", 0, "foo"), t("in", 4), l("Str", 7, "barrr"), t("EOF", 14)]);
    }

    #[test]
    fn resource_matches_file_cabinet() {
        let input = "resource =~ //FileCabinet/(SuiteScripts|Templates/(E-mail%20Templates|Marketing%20Templates)|Web%20Site%20Hosting%20Files)(/.*)*$/";
        assert_eq!(
            scan(input),
            vec![
                l("Str", 0, "resource"),
                t("=~", 9),
                l("RegexStr", 12, "//"),
                l("Str", 14, "FileCabinet/"),
                t("(", 26),
                l("Str", 27, "SuiteScripts"),
                l("ErrorToken", 39, "|"),
                l("Str", 40, "Templates/"),
                t("(", 50),
                l("Str", 51, "E-mail%20Templates"),
                l("ErrorToken", 69, "|"),
                l("Str", 70, "Marketing%20Templates"),
                t(")", 91),
                l("ErrorToken", 92, "|"),
                l("Str", 93, "Web%20Site%20Hosting%20Files"),
                t(")", 121),
                t("(", 122),
                l("RegexStr", 123, "/.*)*$/"),
                t("EOF", 130),
            ]
        );
    }

    #[test]
    fn editor_lang_id_in_testely_supported_lang_ids_and_resource_filename_matches_test() {
        // No original, `\w` num literal de string é só `w`.
        let input = "editorLangId in testely.supportedLangIds && resourceFilename =~ /^.+(.test.(w+))$/gm";
        assert_eq!(
            scan(input),
            vec![
                l("Str", 0, "editorLangId"),
                t("in", 13),
                l("Str", 16, "testely.supportedLangIds"),
                t("&&", 41),
                l("Str", 44, "resourceFilename"),
                t("=~", 61),
                l("RegexStr", 64, "/^.+(.test.(w+))$/gm"),
                t("EOF", 84),
            ]
        );
    }

    #[test]
    fn not_foo_and_bar_and_baz() {
        assert_eq!(
            scan("!(foo && bar) && baz"),
            vec![
                t("!", 0),
                t("(", 1),
                l("Str", 2, "foo"),
                t("&&", 6),
                l("Str", 9, "bar"),
                t(")", 12),
                t("&&", 14),
                l("Str", 17, "baz"),
                t("EOF", 20),
            ]
        );
    }

    #[test]
    fn foo_bar_zed_completed_equality_with_no_space() {
        assert_eq!(scan("foo.bar:zed==completed"), vec![l("Str", 0, "foo.bar:zed"), t("==", 11), l("Str", 13, "completed"), t("EOF", 22)]);
    }

    #[test]
    fn a_and_b_or_c() {
        assert_eq!(scan("a && b || c"), vec![l("Str", 0, "a"), t("&&", 2), l("Str", 5, "b"), t("||", 7), l("Str", 10, "c"), t("EOF", 11)]);
    }

    #[test]
    fn foo_bar_and_baz_jar_and_fee_bee_k_loo_1() {
        assert_eq!(
            scan("fooBar && baz.jar && fee.bee<K-loo+1>"),
            vec![l("Str", 0, "fooBar"), t("&&", 7), l("Str", 10, "baz.jar"), t("&&", 18), l("Str", 21, "fee.bee<K-loo+1>"), t("EOF", 37)]
        );
    }

    #[test]
    fn foo_bar_baz_c_r_less_than_2() {
        assert_eq!(scan("foo.barBaz<C-r> < 2"), vec![l("Str", 0, "foo.barBaz<C-r>"), t("<", 16), l("Str", 18, "2"), t("EOF", 19)]);
    }

    #[test]
    fn foo_bar_greater_equals_minus_1() {
        assert_eq!(scan("foo.bar >= -1"), vec![l("Str", 0, "foo.bar"), t(">=", 8), l("Str", 11, "-1"), t("EOF", 13)]);
    }

    #[test]
    fn foo_bar_less_equals_minus_1() {
        assert_eq!(scan("foo.bar <= -1"), vec![l("Str", 0, "foo.bar"), t("<=", 8), l("Str", 11, "-1"), t("EOF", 13)]);
    }

    #[test]
    fn resource_matches_objects_xml() {
        assert_eq!(
            scan("resource =~ /\\/Objects\\/.+\\.xml$/"),
            vec![l("Str", 0, "resource"), t("=~", 9), l("RegexStr", 12, "/\\/Objects\\/.+\\.xml$/"), t("EOF", 33)]
        );
    }

    #[test]
    fn view_equals_vsc_packages_activitybar_folders_and_vsc_packages_folders_loaded() {
        assert_eq!(
            scan("view == vsc-packages-activitybar-folders && vsc-packages-folders-loaded"),
            vec![
                l("Str", 0, "view"),
                t("==", 5),
                l("Str", 8, "vsc-packages-activitybar-folders"),
                t("&&", 41),
                l("Str", 44, "vsc-packages-folders-loaded"),
                t("EOF", 71),
            ]
        );
    }

    #[test]
    fn sfdx_project_opened_and_resource_matches_functions() {
        // No original, `\.` num template literal é só `.`.
        let input = "sfdx:project_opened && resource =~ /.*\\/functions\\/.*\\/[^\\/]+(\\/[^\\/]+.(ts|js|java|json|toml))?$/ && resourceFilename != package.json && resourceFilename != package-lock.json && resourceFilename != tsconfig.json";
        assert_eq!(
            scan(input),
            vec![
                l("Str", 0, "sfdx:project_opened"),
                t("&&", 20),
                l("Str", 23, "resource"),
                t("=~", 32),
                l("RegexStr", 35, "/.*\\/functions\\/.*\\/[^\\/]+(\\/[^\\/]+.(ts|js|java|json|toml))?$/"),
                t("&&", 98),
                l("Str", 101, "resourceFilename"),
                t("!=", 118),
                l("Str", 121, "package.json"),
                t("&&", 134),
                l("Str", 137, "resourceFilename"),
                t("!=", 154),
                l("Str", 157, "package-lock.json"),
                t("&&", 175),
                l("Str", 178, "resourceFilename"),
                t("!=", 195),
                l("Str", 198, "tsconfig.json"),
                t("EOF", 211),
            ]
        );
    }

    #[test]
    fn view_matches_servers_and_view_item_matches_starting_started() {
        assert_eq!(
            scan("view =~ '/(servers)/' && viewItem =~ '/^(Starting|Started|Debugging|Stopping|Stopped)/'"),
            vec![
                l("Str", 0, "view"),
                t("=~", 5),
                l("QuotedStr", 9, "/(servers)/"),
                t("&&", 22),
                l("Str", 25, "viewItem"),
                t("=~", 34),
                l("QuotedStr", 38, "/^(Starting|Started|Debugging|Stopping|Stopped)/"),
                t("EOF", 87),
            ]
        );
    }

    #[test]
    fn resource_path_matches_md_yml_txt_gim() {
        assert_eq!(
            scan("resourcePath =~ /.md(.yml|.txt)*$/gim"),
            vec![l("Str", 0, "resourcePath"), t("=~", 13), l("RegexStr", 16, "/.md(.yml|.txt)*$/gim"), t("EOF", 37)]
        );
    }
}

#[test]
fn foo_triple_equals_bar_quote() {
    assert_eq!(scan("foo === bar'"), vec![l("Str", 0, "foo"), t("===", 4), l("Str", 8, "bar"), l("ErrorToken", 11, "'"), t("EOF", 12)]);
}

mod handling_lexical_errors {
    use super::*;

    #[test]
    fn foo_triple_equals_quote() {
        assert_eq!(scan("foo === '"), vec![l("Str", 0, "foo"), t("===", 4), l("ErrorToken", 8, "'"), t("EOF", 9)]);
    }

    #[test]
    fn foo_and_bar_unterminated_single_quote() {
        assert_eq!(scan("foo && 'bar"), vec![l("Str", 0, "foo"), t("&&", 4), l("ErrorToken", 7, "'bar"), t("EOF", 11)]);
    }

    #[test]
    fn vim_c_r_equals_1_and_vim_2_less_equals_3_with_spaces() {
        assert_eq!(
            scan("vim<c-r> == 1 && vim<2 <= 3"),
            vec![l("Str", 0, "vim<c-r>"), t("==", 9), l("Str", 12, "1"), t("&&", 14), l("Str", 17, "vim<2"), t("<=", 23), l("Str", 26, "3"), t("EOF", 27)]
        );
    }

    #[test]
    fn vim_c_r_equals_1_and_vim_2_less_equals_3_without_spaces() {
        assert_eq!(
            scan("vim<c-r>==1 && vim<2<=3"),
            vec![
                l("Str", 0, "vim<c-r>"),
                t("==", 8),
                l("Str", 10, "1"),
                t("&&", 12),
                l("Str", 15, "vim<2<"),
                l("ErrorToken", 21, "="),
                l("Str", 22, "3"),
                t("EOF", 23),
            ]
        );
    }

    #[test]
    fn foo_pipe_bar() {
        assert_eq!(scan("foo|bar"), vec![l("Str", 0, "foo"), l("ErrorToken", 3, "|"), l("Str", 4, "bar"), t("EOF", 7)]);
    }

    #[test]
    fn resource_matches_regex_with_unescaped_slashes_foo() {
        assert_eq!(
            scan("resource =~ //foo/(barr|door/(Foo-Bar%20Templates|Soo%20Looo)|Web%20Site%Jjj%20Llll)(/.*)*$/"),
            vec![
                l("Str", 0, "resource"),
                t("=~", 9),
                l("RegexStr", 12, "//"),
                l("Str", 14, "foo/"),
                t("(", 18),
                l("Str", 19, "barr"),
                l("ErrorToken", 23, "|"),
                l("Str", 24, "door/"),
                t("(", 29),
                l("Str", 30, "Foo-Bar%20Templates"),
                l("ErrorToken", 49, "|"),
                l("Str", 50, "Soo%20Looo"),
                t(")", 60),
                l("ErrorToken", 61, "|"),
                l("Str", 62, "Web%20Site%Jjj%20Llll"),
                t(")", 83),
                t("(", 84),
                l("RegexStr", 85, "/.*)*$/"),
                t("EOF", 92),
            ]
        );
    }

    #[test]
    fn regex_with_unescaped_slashes_at_start() {
        assert_eq!(
            scan("/((/foo/(?!bar)(.*)/)|((/src/).*/)).*$/"),
            vec![
                l("RegexStr", 0, "/((/"),
                l("Str", 4, "foo/"),
                t("(", 8),
                l("Str", 9, "?"),
                t("!", 10),
                l("Str", 11, "bar"),
                t(")", 14),
                t("(", 15),
                l("Str", 16, ".*"),
                t(")", 18),
                l("RegexStr", 19, "/)|((/s"),
                l("Str", 26, "rc/"),
                t(")", 29),
                l("Str", 30, ".*/"),
                t(")", 33),
                t(")", 34),
                l("Str", 35, ".*$/"),
                t("EOF", 39),
            ]
        );
    }

    #[test]
    fn resource_path_matches_foo_barr_or_resource_path_matches_view_in_foo_bar() {
        assert_eq!(
            scan("resourcePath =~ //foo/barr// || resourcePath =~ //view/(jarrr|doooor|bees)/(web|templates)// && resourceExtname in foo.Bar"),
            vec![
                l("Str", 0, "resourcePath"),
                t("=~", 13),
                l("RegexStr", 16, "//"),
                l("Str", 18, "foo/barr//"),
                t("||", 29),
                l("Str", 32, "resourcePath"),
                t("=~", 45),
                l("RegexStr", 48, "//"),
                l("Str", 50, "view/"),
                t("(", 55),
                l("Str", 56, "jarrr"),
                l("ErrorToken", 61, "|"),
                l("Str", 62, "doooor"),
                l("ErrorToken", 68, "|"),
                l("Str", 69, "bees"),
                t(")", 73),
                l("RegexStr", 74, "/(web|templates)/"),
                l("ErrorToken", 91, "/ && resourceExtname in foo.Bar"),
                t("EOF", 122),
            ]
        );
    }

    #[test]
    fn foo_matches_file_slash_or_bar() {
        // `JSON.parse('"foo =~ /file:\// || bar"')`: o `\/` do JSON vira `/`.
        assert_eq!(
            scan("foo =~ /file:// || bar"),
            vec![l("Str", 0, "foo"), t("=~", 4), l("RegexStr", 7, "/file:/"), l("ErrorToken", 14, "/ || bar"), t("EOF", 22)]
        );
    }
}
