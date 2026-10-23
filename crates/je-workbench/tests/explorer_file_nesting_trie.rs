//! Porte de `src/vs/workbench/contrib/files/test/browser/explorerFileNestingTrie.test.ts` (suítes `SufTrie`,
//! `PreTrie` e `StarTrie`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` vira o `LeakGuard` criado no começo de cada caso.
//! - O `test.skip('is fast')` do original continua pulado, como `#[ignore]`.
//! - O `deepStrictEqual` sobre `string[]` vira `assert_eq!` sobre `Vec`, que também compara a ordem.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_workbench::contrib::files::explorer_file_nesting_trie::{
    ExplorerFileNestingTrie, FilenameAttributes, NestingResult, PreTrie, SufTrie,
};

fn fake_filename_attributes() -> FilenameAttributes {
    FilenameAttributes { dirname: "mydir".to_string(), basename: String::new(), extname: String::new() }
}

fn no_matches() -> Vec<&'static str> {
    Vec::new()
}

mod suf_trie {
    use super::*;

    #[test]
    fn exact_matches() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let attrs = fake_filename_attributes();
        let mut t = SufTrie::new();
        t.add(".npmrc", "MyKey");
        assert_eq!(t.get(".npmrc", &attrs), vec!["MyKey"]);
        assert_eq!(t.get(".npmrcs", &attrs), no_matches());
        assert_eq!(t.get("a.npmrc", &attrs), no_matches());
    }

    #[test]
    fn star_matches() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let attrs = fake_filename_attributes();
        let mut t = SufTrie::new();
        t.add("*.npmrc", "MyKey");
        assert_eq!(t.get(".npmrc", &attrs), vec!["MyKey"]);
        assert_eq!(t.get("npmrc", &attrs), no_matches());
        assert_eq!(t.get(".npmrcs", &attrs), no_matches());
        assert_eq!(t.get("a.npmrc", &attrs), vec!["MyKey"]);
        assert_eq!(t.get("a.b.c.d.npmrc", &attrs), vec!["MyKey"]);
    }

    #[test]
    fn star_substitutes() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let attrs = fake_filename_attributes();
        let mut t = SufTrie::new();
        t.add("*.npmrc", "${capture}.json");
        assert_eq!(t.get(".npmrc", &attrs), vec![".json"]);
        assert_eq!(t.get("npmrc", &attrs), no_matches());
        assert_eq!(t.get(".npmrcs", &attrs), no_matches());
        assert_eq!(t.get("a.npmrc", &attrs), vec!["a.json"]);
        assert_eq!(t.get("a.b.c.d.npmrc", &attrs), vec!["a.b.c.d.json"]);
    }

    #[test]
    fn multi_matches() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let attrs = fake_filename_attributes();
        let mut t = SufTrie::new();
        t.add("*.npmrc", "Key1");
        t.add("*.json", "Key2");
        t.add("*d.npmrc", "Key3");
        assert_eq!(t.get(".npmrc", &attrs), vec!["Key1"]);
        assert_eq!(t.get("npmrc", &attrs), no_matches());
        assert_eq!(t.get(".npmrcs", &attrs), no_matches());
        assert_eq!(t.get(".json", &attrs), vec!["Key2"]);
        assert_eq!(t.get("a.json", &attrs), vec!["Key2"]);
        assert_eq!(t.get("a.npmrc", &attrs), vec!["Key1"]);
        assert_eq!(t.get("a.b.c.d.npmrc", &attrs), vec!["Key1", "Key3"]);
    }

    #[test]
    fn multi_substitutes() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let attrs = fake_filename_attributes();
        let mut t = SufTrie::new();
        t.add("*.npmrc", "Key1.${capture}.js");
        t.add("*.json", "Key2.${capture}.js");
        t.add("*d.npmrc", "Key3.${capture}.js");
        assert_eq!(t.get(".npmrc", &attrs), vec!["Key1..js"]);
        assert_eq!(t.get("npmrc", &attrs), no_matches());
        assert_eq!(t.get(".npmrcs", &attrs), no_matches());
        assert_eq!(t.get(".json", &attrs), vec!["Key2..js"]);
        assert_eq!(t.get("a.json", &attrs), vec!["Key2.a.js"]);
        assert_eq!(t.get("a.npmrc", &attrs), vec!["Key1.a.js"]);
        assert_eq!(t.get("a.b.cd.npmrc", &attrs), vec!["Key1.a.b.cd.js", "Key3.a.b.c.js"]);
        assert_eq!(t.get("a.b.c.d.npmrc", &attrs), vec!["Key1.a.b.c.d.js", "Key3.a.b.c..js"]);
    }
}

mod pre_trie {
    use super::*;

    #[test]
    fn exact_matches() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let attrs = fake_filename_attributes();
        let mut t = PreTrie::new();
        t.add(".npmrc", "MyKey");
        assert_eq!(t.get(".npmrc", &attrs), vec!["MyKey"]);
        assert_eq!(t.get(".npmrcs", &attrs), no_matches());
        assert_eq!(t.get("a.npmrc", &attrs), no_matches());
    }

    #[test]
    fn star_matches() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let attrs = fake_filename_attributes();
        let mut t = PreTrie::new();
        t.add("*.npmrc", "MyKey");
        assert_eq!(t.get(".npmrc", &attrs), vec!["MyKey"]);
        assert_eq!(t.get("npmrc", &attrs), no_matches());
        assert_eq!(t.get(".npmrcs", &attrs), no_matches());
        assert_eq!(t.get("a.npmrc", &attrs), vec!["MyKey"]);
        assert_eq!(t.get("a.b.c.d.npmrc", &attrs), vec!["MyKey"]);
    }

    #[test]
    fn star_substitutes() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let attrs = fake_filename_attributes();
        let mut t = PreTrie::new();
        t.add("*.npmrc", "${capture}.json");
        assert_eq!(t.get(".npmrc", &attrs), vec![".json"]);
        assert_eq!(t.get("npmrc", &attrs), no_matches());
        assert_eq!(t.get(".npmrcs", &attrs), no_matches());
        assert_eq!(t.get("a.npmrc", &attrs), vec!["a.json"]);
        assert_eq!(t.get("a.b.c.d.npmrc", &attrs), vec!["a.b.c.d.json"]);
    }

    #[test]
    fn multi_matches() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let attrs = fake_filename_attributes();
        let mut t = PreTrie::new();
        t.add("*.npmrc", "Key1");
        t.add("*.json", "Key2");
        t.add("*d.npmrc", "Key3");
        assert_eq!(t.get(".npmrc", &attrs), vec!["Key1"]);
        assert_eq!(t.get("npmrc", &attrs), no_matches());
        assert_eq!(t.get(".npmrcs", &attrs), no_matches());
        assert_eq!(t.get(".json", &attrs), vec!["Key2"]);
        assert_eq!(t.get("a.json", &attrs), vec!["Key2"]);
        assert_eq!(t.get("a.npmrc", &attrs), vec!["Key1"]);
        assert_eq!(t.get("a.b.c.d.npmrc", &attrs), vec!["Key1", "Key3"]);
    }

    #[test]
    fn multi_substitutes() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let attrs = fake_filename_attributes();
        let mut t = PreTrie::new();
        t.add("*.npmrc", "Key1.${capture}.js");
        t.add("*.json", "Key2.${capture}.js");
        t.add("*d.npmrc", "Key3.${capture}.js");
        assert_eq!(t.get(".npmrc", &attrs), vec!["Key1..js"]);
        assert_eq!(t.get("npmrc", &attrs), no_matches());
        assert_eq!(t.get(".npmrcs", &attrs), no_matches());
        assert_eq!(t.get(".json", &attrs), vec!["Key2..js"]);
        assert_eq!(t.get("a.json", &attrs), vec!["Key2.a.js"]);
        assert_eq!(t.get("a.npmrc", &attrs), vec!["Key1.a.js"]);
        assert_eq!(t.get("a.b.cd.npmrc", &attrs), vec!["Key1.a.b.cd.js", "Key3.a.b.c.js"]);
        assert_eq!(t.get("a.b.c.d.npmrc", &attrs), vec!["Key1.a.b.c.d.js", "Key3.a.b.c..js"]);
    }

    #[test]
    fn empty_matches() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let attrs = fake_filename_attributes();
        let mut t = PreTrie::new();
        t.add("package*json", "package");
        assert_eq!(t.get("package.json", &attrs), vec!["package"]);
        assert_eq!(t.get("packagejson", &attrs), vec!["package"]);
        assert_eq!(t.get("package-lock.json", &attrs), vec!["package"]);
    }
}

mod star_trie {
    use super::*;

    fn assert_map_equals(actual: &NestingResult, expected: &[(&str, &[&str])]) {
        let actual_str: Vec<String> = actual
            .iter()
            .map(|(parent, children)| format!("{parent} => [{}]", children.keys().cloned().collect::<Vec<_>>().join(",")))
            .collect();
        let expected_str: Vec<String> =
            expected.iter().map(|(parent, children)| format!("{parent}: [{}]", children.join(","))).collect();
        let big_msg = format!("{}==={}", actual_str.join(","), expected_str.join(","));
        assert_eq!(actual.len(), expected.len(), "{big_msg}");
        for (parent, act) in actual.iter() {
            let exp = expected
                .iter()
                .find(|(name, _)| name == parent)
                .map(|(_, children)| *children)
                .unwrap_or_else(|| panic!("pai inesperado {parent}: {big_msg}"));
            let msg = format!("{big_msg}\n{}==={}", act.keys().cloned().collect::<Vec<_>>().join(","), exp.join(","));
            assert!(act.len() == exp.len(), "{msg}");
            for child in exp {
                assert!(act.contains_key(&child.to_string()), "{msg}");
            }
        }
    }

    #[test]
    fn does_added_extension_nesting() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let t = ExplorerFileNestingTrie::new(&[("*", &["${capture}.*"])]);
        let nesting = t.nest(
            &[
                "file",
                "file.json",
                "boop.test",
                "boop.test1",
                "boop.test.1",
                "beep",
                "beep.test1",
                "beep.boop.test1",
                "beep.boop.test2",
                "beep.boop.a",
            ],
            "mydir",
        );
        assert_map_equals(
            &nesting,
            &[
                ("file", &["file.json"]),
                ("boop.test", &["boop.test.1"]),
                ("boop.test1", &[]),
                ("beep", &["beep.test1", "beep.boop.test1", "beep.boop.test2", "beep.boop.a"]),
            ],
        );
    }

    #[test]
    fn does_ext_specific_nesting() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let t = ExplorerFileNestingTrie::new(&[("*.ts", &["${capture}.js"]), ("*.js", &["${capture}.map"])]);
        let nesting = t.nest(
            &["a.ts", "a.js", "a.jss", "ab.js", "b.js", "b.map", "c.ts", "c.js", "c.map", "d.ts", "d.map"],
            "mydir",
        );
        assert_map_equals(
            &nesting,
            &[
                ("a.ts", &["a.js"]),
                ("ab.js", &[]),
                ("a.jss", &[]),
                ("b.js", &["b.map"]),
                ("c.ts", &["c.js", "c.map"]),
                ("d.ts", &[]),
                ("d.map", &[]),
            ],
        );
    }

    #[test]
    fn handles_loops() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let t = ExplorerFileNestingTrie::new(&[
            ("*.a", &["${capture}.b", "${capture}.c"]),
            ("*.b", &["${capture}.a"]),
            ("*.c", &["${capture}.d"]),
            ("*.aa", &["${capture}.bb"]),
            ("*.bb", &["${capture}.cc", "${capture}.dd"]),
            ("*.cc", &["${capture}.aa"]),
            ("*.dd", &["${capture}.ee"]),
        ]);
        let nesting = t.nest(
            &[
                ".a", ".b", ".c", ".d", "a.a", "a.b", "a.d", "a.aa", "a.bb", "a.cc", "b.aa", "b.bb", "c.bb", "c.cc",
                "d.aa", "d.cc", "e.aa", "e.bb", "e.dd", "e.ee", "f.aa", "f.bb", "f.cc", "f.dd", "f.ee",
            ],
            "mydir",
        );

        assert_map_equals(
            &nesting,
            &[
                (".a", &[]),
                (".b", &[]),
                (".c", &[]),
                (".d", &[]),
                ("a.a", &[]),
                ("a.b", &[]),
                ("a.d", &[]),
                ("a.aa", &[]),
                ("a.bb", &[]),
                ("a.cc", &[]),
                ("b.aa", &["b.bb"]),
                ("c.bb", &["c.cc"]),
                ("d.cc", &["d.aa"]),
                ("e.aa", &["e.bb", "e.dd", "e.ee"]),
                ("f.aa", &[]),
                ("f.bb", &[]),
                ("f.cc", &[]),
                ("f.dd", &[]),
                ("f.ee", &[]),
            ],
        );
    }

    #[test]
    fn does_general_bidirectional_suffix_matching() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let t = ExplorerFileNestingTrie::new(&[("*-vsdoc.js", &["${capture}.js"]), ("*.js", &["${capture}-vscdoc.js"])]);

        let nesting = t.nest(&["a-vsdoc.js", "a.js", "b.js", "b-vscdoc.js"], "mydir");

        assert_map_equals(&nesting, &[("a-vsdoc.js", &["a.js"]), ("b.js", &["b-vscdoc.js"])]);
    }

    #[test]
    fn does_general_bidirectional_prefix_matching() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let t = ExplorerFileNestingTrie::new(&[("vsdoc-*.js", &["${capture}.js"]), ("*.js", &["vscdoc-${capture}.js"])]);

        let nesting = t.nest(&["vsdoc-a.js", "a.js", "b.js", "vscdoc-b.js"], "mydir");

        assert_map_equals(&nesting, &[("vsdoc-a.js", &["a.js"]), ("b.js", &["vscdoc-b.js"])]);
    }

    #[test]
    fn does_general_bidirectional_general_matching() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let t = ExplorerFileNestingTrie::new(&[("foo-*-bar.js", &["${capture}.js"]), ("*.js", &["bib-${capture}-bap.js"])]);

        let nesting = t.nest(&["foo-a-bar.js", "a.js", "b.js", "bib-b-bap.js"], "mydir");

        assert_map_equals(&nesting, &[("foo-a-bar.js", &["a.js"]), ("b.js", &["bib-b-bap.js"])]);
    }

    #[test]
    fn does_extension_specific_path_segment_matching() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let t = ExplorerFileNestingTrie::new(&[("*.js", &["${capture}.*.js"])]);

        let nesting = t.nest(&["foo.js", "foo.test.js", "fooTest.js", "bar.js.js"], "mydir");

        assert_map_equals(&nesting, &[("foo.js", &["foo.test.js"]), ("fooTest.js", &[]), ("bar.js.js", &[])]);
    }

    #[test]
    fn does_exact_match_nesting() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let t = ExplorerFileNestingTrie::new(&[
            (
                "package.json",
                &[".npmrc", "npm-shrinkwrap.json", "yarn.lock", ".yarnclean", ".yarnignore", ".yarn-integrity", ".yarnrc"],
            ),
            ("bower.json", &[".bowerrc"]),
        ]);

        let nesting = t.nest(&["package.json", ".npmrc", "npm-shrinkwrap.json", "yarn.lock", ".bowerrc"], "mydir");

        assert_map_equals(
            &nesting,
            &[("package.json", &[".npmrc", "npm-shrinkwrap.json", "yarn.lock"]), (".bowerrc", &[])],
        );
    }

    #[test]
    fn eslint_test() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let t = ExplorerFileNestingTrie::new(&[(".eslintrc*", &[".eslint*"])]);

        let nesting1 = t.nest(&[".eslintrc.json", ".eslintignore"], "mydir");

        assert_map_equals(&nesting1, &[(".eslintrc.json", &[".eslintignore"])]);

        let nesting2 = t.nest(&[".eslintrc", ".eslintignore"], "mydir");

        assert_map_equals(&nesting2, &[(".eslintrc", &[".eslintignore"])]);
    }

    #[test]
    fn basename_expansion() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let t = ExplorerFileNestingTrie::new(&[("*-vsdoc.js", &["${basename}.doc"])]);

        let nesting1 = t.nest(&["boop-vsdoc.js", "boop-vsdoc.doc", "boop.doc"], "mydir");

        assert_map_equals(&nesting1, &[("boop-vsdoc.js", &["boop-vsdoc.doc"]), ("boop.doc", &[])]);
    }

    #[test]
    fn extname_expansion() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let t = ExplorerFileNestingTrie::new(&[("*-vsdoc.js", &["${extname}.doc"])]);

        let nesting1 = t.nest(&["boop-vsdoc.js", "js.doc", "boop.doc"], "mydir");

        assert_map_equals(&nesting1, &[("boop-vsdoc.js", &["js.doc"]), ("boop.doc", &[])]);
    }

    #[test]
    fn added_segment_matcher() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let t = ExplorerFileNestingTrie::new(&[("*", &["${basename}.*.${extname}"])]);

        let nesting1 = t.nest(
            &["some.file", "some.html.file", "some.html.nested.file", "other.file", "some.thing", "some.thing.else"],
            "mydir",
        );

        assert_map_equals(
            &nesting1,
            &[
                ("some.file", &["some.html.file", "some.html.nested.file"]),
                ("other.file", &[]),
                ("some.thing", &[]),
                ("some.thing.else", &[]),
            ],
        );
    }

    #[test]
    fn added_segment_matcher_old_format() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let t = ExplorerFileNestingTrie::new(&[("*", &["$(basename).*.$(extname)"])]);

        let nesting1 = t.nest(
            &["some.file", "some.html.file", "some.html.nested.file", "other.file", "some.thing", "some.thing.else"],
            "mydir",
        );

        assert_map_equals(
            &nesting1,
            &[
                ("some.file", &["some.html.file", "some.html.nested.file"]),
                ("other.file", &[]),
                ("some.thing", &[]),
                ("some.thing.else", &[]),
            ],
        );
    }

    #[test]
    fn multiple_child_patterns_matching_the_parent_itself() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let t = ExplorerFileNestingTrie::new(&[("*", &["${basename}.*", "${basename}*.${extname}"])]);

        let nesting1 =
            t.nest(&["Button.tsx", "Button.test.tsx", "Button.module.css", "ButtonGroup.tsx", "other.ts"], "mydir");

        assert_map_equals(
            &nesting1,
            &[("Button.tsx", &["Button.test.tsx", "Button.module.css", "ButtonGroup.tsx"]), ("other.ts", &[])],
        );
    }

    #[test]
    fn dirname_matching() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let t = ExplorerFileNestingTrie::new(&[("index.ts", &["${dirname}.ts"])]);

        let nesting1 = t.nest(&["otherFile.ts", "MyComponent.ts", "index.ts"], "MyComponent");

        assert_map_equals(&nesting1, &[("index.ts", &["MyComponent.ts"]), ("otherFile.ts", &[])]);
    }

    #[test]
    #[ignore = "test.skip no original"]
    fn is_fast() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let big_nester = ExplorerFileNestingTrie::new(&[
            ("*", &["${capture}.*"]),
            ("*.js", &["${capture}.*.js", "${capture}.map"]),
            ("*.jsx", &["${capture}.js"]),
            ("*.ts", &["${capture}.js", "${capture}.*.ts"]),
            ("*.tsx", &["${capture}.js"]),
            ("*.css", &["${capture}.*.css", "${capture}.map"]),
            ("*.html", &["${capture}.*.html"]),
            ("*.htm", &["${capture}.*.htm"]),
            ("*.less", &["${capture}.*.less", "${capture}.css"]),
            ("*.scss", &["${capture}.*.scss", "${capture}.css"]),
            ("*.sass", &["${capture}.css"]),
            ("*.styl", &["${capture}.css"]),
            ("*.coffee", &["${capture}.*.coffee", "${capture}.js"]),
            ("*.iced", &["${capture}.*.iced", "${capture}.js"]),
            ("*.config", &["${capture}.*.config"]),
            ("*.cs", &["${capture}.*.cs", "${capture}.cs.d.ts"]),
            ("*.vb", &["${capture}.*.vb"]),
            ("*.json", &["${capture}.*.json"]),
            ("*.md", &["${capture}.html"]),
            ("*.mdown", &["${capture}.html"]),
            ("*.markdown", &["${capture}.html"]),
            ("*.mdwn", &["${capture}.html"]),
            ("*.svg", &["${capture}.svgz"]),
            ("*.a", &["${capture}.b"]),
            ("*.b", &["${capture}.a"]),
            ("*.resx", &["${capture}.designer.cs"]),
            (
                "package.json",
                &[".npmrc", "npm-shrinkwrap.json", "yarn.lock", ".yarnclean", ".yarnignore", ".yarn-integrity", ".yarnrc"],
            ),
            ("bower.json", &[".bowerrc"]),
            ("*-vsdoc.js", &["${capture}.js"]),
            ("*.tt", &["${capture}.*"]),
        ]);

        let mut big_files: Vec<String> = Vec::new();
        for i in 0..(50000 / 6) {
            for ext in ["js", "map", "css", "ts", "d.ts", "jsx"] {
                big_files.push(format!("file{i}.{ext}"));
            }
        }
        let big_refs: Vec<&str> = big_files.iter().map(String::as_str).collect();

        let start = std::time::Instant::now();
        big_nester.nest(&big_refs, "mydir");
        let elapsed = start.elapsed();
        assert!(elapsed.as_millis() < 1000, "too slow...{}", elapsed.as_millis());
    }
}
