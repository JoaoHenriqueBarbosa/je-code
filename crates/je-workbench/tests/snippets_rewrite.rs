//! Porte de `src/vs/workbench/contrib/snippets/test/browser/snippetsRewrite.test.ts` (suíte `SnippetRewrite`), caso a caso
//! e na mesma ordem.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` vira o `LeakGuard` criado no começo de cada caso.
//! - `assertRewrite(input, expected)` com `expected` booleano (`typeof expected === 'boolean'`) é o [`Rewrite::Unchanged`]:
//!   o `codeSnippet` deve ser igual à entrada; com texto é o [`Rewrite::To`].
//! - `new Snippet(false, ['foo'], 'foo', 'foo', 'foo', input, 'foo', SnippetSource.User, generateUuid())` é o
//!   `Snippet::new` do porte, sem `include`, `exclude` e `extensionId`.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_base::uuid::generate_uuid;
use je_workbench::contrib::snippets::snippets_file::{Snippet, SnippetSource};

enum Rewrite<'a> {
    Unchanged,
    To(&'a str),
}

fn assert_rewrite(input: &str, expected: Rewrite<'_>) {
    let actual = Snippet::new(false, vec!["foo".to_owned()], "foo", "foo", "foo", input, "foo", SnippetSource::User, &generate_uuid(), None, None, None);
    match expected {
        Rewrite::Unchanged => assert_eq!(actual.code_snippet(), input),
        Rewrite::To(expected) => assert_eq!(actual.code_snippet(), expected),
    }
}

#[test]
fn bogous_variable_rewrite() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();

    assert_rewrite("foo", Rewrite::Unchanged);
    assert_rewrite("hello $1 world$0", Rewrite::Unchanged);

    assert_rewrite("$foo and $foo", Rewrite::To("${1:foo} and ${1:foo}"));
    assert_rewrite("$1 and $SELECTION and $foo", Rewrite::To("$1 and ${SELECTION} and ${2:foo}"));

    assert_rewrite(
        &[
            "for (var ${index} = 0; ${index} < ${array}.length; ${index}++) {",
            "\tvar ${element} = ${array}[${index}];",
            "\t$0",
            "}",
        ]
        .join("\n"),
        Rewrite::To(
            &[
                "for (var ${1:index} = 0; ${1:index} < ${2:array}.length; ${1:index}++) {",
                "\tvar ${3:element} = ${2:array}[${1:index}];",
                "\t$0",
                "\\}",
            ]
            .join("\n"),
        ),
    );
}

#[test]
fn snippet_choices_unable_to_escape_comma_and_pipe_31521() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    assert_rewrite("console.log(${1|not\\, not, five, 5, 1   23|});", Rewrite::Unchanged);
}

#[test]
fn lazy_bogous_variable_rewrite() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let snippet = Snippet::new(
        false,
        vec!["fooLang".to_owned()],
        "foo",
        "prefix",
        "desc",
        "This is ${bogous} because it is a ${var}",
        "source",
        SnippetSource::Extension,
        &generate_uuid(),
        None,
        None,
        None,
    );
    assert_eq!(snippet.body, "This is ${bogous} because it is a ${var}");
    assert_eq!(snippet.code_snippet(), "This is ${1:bogous} because it is a ${2:var}");
    assert!(snippet.is_bogous());
}
