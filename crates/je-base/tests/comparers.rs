//! Porte de `src/vs/base/test/browser/comparers.test.ts` (suíte `Comparers`), caso a caso e na mesma ordem.
//!
//! A locale é a padrão (`en`). `compareLocale` e `compareLocaleNumeric` do original são o
//! `a.localeCompare(b)` e o `a.localeCompare(b, undefined, { numeric: true })`, aqui um `Collator` sem opções
//! e um `numeric: true`. `string | null` é `Option<&str>`, e as listas ordenadas são `Vec<String>`.

use je_base::collator::{Collator, CollatorOptions};
use je_base::comparers;

type Comparer = fn(Option<&[u16]>, Option<&[u16]>) -> i32;

fn u(text: &str) -> Vec<u16> {
    text.encode_utf16().collect()
}

fn c(comparer: Comparer, one: Option<&str>, other: Option<&str>) -> i32 {
    comparer(one.map(u).as_deref(), other.map(u).as_deref())
}

fn locale(one: &str, other: &str) -> i32 {
    Collator::new(None, CollatorOptions::default()).compare(&u(one), &u(other))
}

fn locale_numeric(one: &str, other: &str) -> i32 {
    Collator::new(None, CollatorOptions { numeric: true, ..CollatorOptions::default() }).compare(&u(one), &u(other))
}

fn locale_fn(one: Option<&[u16]>, other: Option<&[u16]>) -> i32 {
    Collator::new(None, CollatorOptions::default()).compare(one.unwrap_or(&[]), other.unwrap_or(&[]))
}

/// `[...].sort(comparer)`: ordenação estável como a do JavaScript.
fn sorted(comparer: Comparer, items: &[&str]) -> Vec<String> {
    let mut items: Vec<String> = items.iter().map(|s| s.to_string()).collect();
    items.sort_by(|a, b| comparer(Some(&u(a)), Some(&u(b))).cmp(&0));
    items
}

#[test]
fn compare_file_names() {
    //
    // Comparisons with the same results as compareFileNamesDefault
    //

    // name-only comparisons
    assert!(c(comparers::compare_file_names, None, None) == 0, "null should be equal");
    assert!(c(comparers::compare_file_names, None, Some("abc")) < 0, "null should be come before real values");
    assert!(c(comparers::compare_file_names, Some(""), Some("")) == 0, "empty should be equal");
    assert!(c(comparers::compare_file_names, Some("abc"), Some("abc")) == 0, "equal names should be equal");
    assert!(c(comparers::compare_file_names, Some("z"), Some("A")) > 0, "z comes after A");
    assert!(c(comparers::compare_file_names, Some("Z"), Some("a")) > 0, "Z comes after a");
    // name plus extension comparisons
    assert!(c(comparers::compare_file_names, Some("bbb.aaa"), Some("aaa.bbb")) > 0, "compares the whole name all at once by locale");
    assert!(c(comparers::compare_file_names, Some("aggregate.go"), Some("aggregate_repo.go")) > 0, "compares the whole name all at once by locale");
    // dotfile comparisons
    assert!(c(comparers::compare_file_names, Some(".abc"), Some(".abc")) == 0, "equal dotfile names should be equal");
    assert!(c(comparers::compare_file_names, Some(".env."), Some(".gitattributes")) < 0, "filenames starting with dots and with extensions should still sort properly");
    assert!(c(comparers::compare_file_names, Some(".env"), Some(".aaa.env")) > 0, "dotfiles sort alphabetically when they contain multiple dots");
    assert!(c(comparers::compare_file_names, Some(".env"), Some(".env.aaa")) < 0, "dotfiles with the same root sort shortest first");
    assert!(c(comparers::compare_file_names, Some(".aaa_env"), Some(".aaa.env")) < 0, "an underscore in a dotfile name will sort before a dot");
    // dotfile vs non-dotfile comparisons
    assert!(c(comparers::compare_file_names, None, Some(".abc")) < 0, "null should come before dotfiles");
    assert!(c(comparers::compare_file_names, Some(".env"), Some("aaa")) < 0, "dotfiles come before filenames without extensions");
    assert!(c(comparers::compare_file_names, Some(".env"), Some("aaa.env")) < 0, "dotfiles come before filenames with extensions");
    assert!(c(comparers::compare_file_names, Some(".md"), Some("A.MD")) < 0, "dotfiles sort before uppercase files");
    assert!(c(comparers::compare_file_names, Some(".MD"), Some("a.md")) < 0, "dotfiles sort before lowercase files");
    // numeric comparisons
    assert!(c(comparers::compare_file_names, Some("1"), Some("1")) == 0, "numerically equal full names should be equal");
    assert!(c(comparers::compare_file_names, Some("abc1.txt"), Some("abc1.txt")) == 0, "equal filenames with numbers should be equal");
    assert!(c(comparers::compare_file_names, Some("abc1.txt"), Some("abc2.txt")) < 0, "filenames with numbers should be in numerical order, not alphabetical order");
    assert!(c(comparers::compare_file_names, Some("abc2.txt"), Some("abc10.txt")) < 0, "filenames with numbers should be in numerical order even when they are multiple digits long");
    assert!(c(comparers::compare_file_names, Some("abc02.txt"), Some("abc010.txt")) < 0, "filenames with numbers that have leading zeros sort numerically");
    assert!(c(comparers::compare_file_names, Some("abc1.10.txt"), Some("abc1.2.txt")) > 0, "numbers with dots between them are treated as two separate numbers, not one decimal number");
    assert!(c(comparers::compare_file_names, Some("a.ext1"), Some("b.Ext1")) < 0, "if names are different and extensions with numbers are equal except for case, filenames are sorted in name order");
    assert_eq!(sorted(comparers::compare_file_names, &["a10.txt", "A2.txt", "A100.txt", "a20.txt"]), vec!["A2.txt", "a10.txt", "a20.txt", "A100.txt"], "filenames with number and case differences compare numerically");
    //
    // Comparisons with different results than compareFileNamesDefault
    //

    // name-only comparisons
    assert!(c(comparers::compare_file_names, Some("a"), Some("A")) != locale("a", "A"), "the same letter sorts in unicode order, not by locale");
    assert!(c(comparers::compare_file_names, Some("â"), Some("Â")) != locale("â", "Â"), "the same accented letter sorts in unicode order, not by locale");
    assert_ne!(sorted(comparers::compare_file_names, &["artichoke", "Artichoke", "art", "Art"]), sorted(locale_fn, &["artichoke", "Artichoke", "art", "Art"]), "words with the same root and different cases do not sort in locale order");
    assert_ne!(sorted(comparers::compare_file_names, &["email", "Email", "émail", "Émail"]), sorted(locale_fn, &["email", "Email", "émail", "Émail"]), "the same base characters with different case or accents do not sort in locale order");
    // numeric comparisons
    assert!(c(comparers::compare_file_names, Some("abc02.txt"), Some("abc002.txt")) > 0, "filenames with equivalent numbers and leading zeros sort in unicode order");
    assert!(c(comparers::compare_file_names, Some("abc.txt1"), Some("abc.txt01")) > 0, "same name plus extensions with equal numbers sort in unicode order");
    assert!(c(comparers::compare_file_names, Some("art01"), Some("Art01")) != locale_numeric("art01", "Art01"), "a numerically equivalent word of a different case does not compare numerically based on locale");
    assert!(c(comparers::compare_file_names, Some("a.ext1"), Some("a.Ext1")) > 0, "if names are equal and extensions with numbers are equal except for case, filenames are sorted in full filename unicode order");
}

#[test]
fn compare_file_extensions() {
    //
    // Comparisons with the same results as compareFileExtensionsDefault
    //

    // name-only comparisons
    assert!(c(comparers::compare_file_extensions, None, None) == 0, "null should be equal");
    assert!(c(comparers::compare_file_extensions, None, Some("abc")) < 0, "null should come before real files without extension");
    assert!(c(comparers::compare_file_extensions, Some(""), Some("")) == 0, "empty should be equal");
    assert!(c(comparers::compare_file_extensions, Some("abc"), Some("abc")) == 0, "equal names should be equal");
    assert!(c(comparers::compare_file_extensions, Some("z"), Some("A")) > 0, "z comes after A");
    assert!(c(comparers::compare_file_extensions, Some("Z"), Some("a")) > 0, "Z comes after a");
    // name plus extension comparisons
    assert!(c(comparers::compare_file_extensions, Some("file.ext"), Some("file.ext")) == 0, "equal full names should be equal");
    assert!(c(comparers::compare_file_extensions, Some("a.ext"), Some("b.ext")) < 0, "if equal extensions, filenames should be compared");
    assert!(c(comparers::compare_file_extensions, Some("file.aaa"), Some("file.bbb")) < 0, "files with equal names should be compared by extensions");
    assert!(c(comparers::compare_file_extensions, Some("bbb.aaa"), Some("aaa.bbb")) < 0, "files should be compared by extensions even if filenames compare differently");
    // dotfile comparisons
    assert!(c(comparers::compare_file_extensions, Some(".abc"), Some(".abc")) == 0, "equal dotfiles should be equal");
    assert!(c(comparers::compare_file_extensions, Some(".md"), Some(".Gitattributes")) > 0, "dotfiles sort alphabetically regardless of case");
    // dotfile vs non-dotfile comparisons
    assert!(c(comparers::compare_file_extensions, None, Some(".abc")) < 0, "null should come before dotfiles");
    assert!(c(comparers::compare_file_extensions, Some(".env"), Some("aaa.env")) < 0, "if equal extensions, filenames should be compared, empty filename should come before others");
    assert!(c(comparers::compare_file_extensions, Some(".MD"), Some("a.md")) < 0, "if extensions differ in case, files sort by extension in unicode order");
    // numeric comparisons
    assert!(c(comparers::compare_file_extensions, Some("1"), Some("1")) == 0, "numerically equal full names should be equal");
    assert!(c(comparers::compare_file_extensions, Some("abc1.txt"), Some("abc1.txt")) == 0, "equal filenames with numbers should be equal");
    assert!(c(comparers::compare_file_extensions, Some("abc1.txt"), Some("abc2.txt")) < 0, "filenames with numbers should be in numerical order, not alphabetical order");
    assert!(c(comparers::compare_file_extensions, Some("abc2.txt"), Some("abc10.txt")) < 0, "filenames with numbers should be in numerical order even when they are multiple digits long");
    assert!(c(comparers::compare_file_extensions, Some("abc02.txt"), Some("abc010.txt")) < 0, "filenames with numbers that have leading zeros sort numerically");
    assert!(c(comparers::compare_file_extensions, Some("abc1.10.txt"), Some("abc1.2.txt")) > 0, "numbers with dots between them are treated as two separate numbers, not one decimal number");
    assert!(c(comparers::compare_file_extensions, Some("abc2.txt2"), Some("abc1.txt10")) < 0, "extensions with numbers should be in numerical order, not alphabetical order");
    assert!(c(comparers::compare_file_extensions, Some("txt.abc1"), Some("txt.abc1")) == 0, "equal extensions with numbers should be equal");
    assert!(c(comparers::compare_file_extensions, Some("txt.abc1"), Some("txt.abc2")) < 0, "extensions with numbers should be in numerical order, not alphabetical order");
    assert!(c(comparers::compare_file_extensions, Some("txt.abc2"), Some("txt.abc10")) < 0, "extensions with numbers should be in numerical order even when they are multiple digits long");
    assert!(c(comparers::compare_file_extensions, Some("a.ext1"), Some("b.ext1")) < 0, "if equal extensions with numbers, names should be compared");
    assert_eq!(sorted(comparers::compare_file_extensions, &["a10.txt", "A2.txt", "A100.txt", "a20.txt"]), vec!["A2.txt", "a10.txt", "a20.txt", "A100.txt"], "filenames with number and case differences compare numerically");
    //
    // Comparisons with different results from compareFileExtensionsDefault
    //

    // name-only comparisions
    assert!(c(comparers::compare_file_extensions, Some("a"), Some("A")) != locale("a", "A"), "the same letter of different case does not sort by locale");
    assert!(c(comparers::compare_file_extensions, Some("â"), Some("Â")) != locale("â", "Â"), "the same accented letter of different case does not sort by locale");
    assert_ne!(sorted(comparers::compare_file_extensions, &["artichoke", "Artichoke", "art", "Art"]), sorted(locale_fn, &["artichoke", "Artichoke", "art", "Art"]), "words with the same root and different cases do not sort in locale order");
    assert_ne!(sorted(comparers::compare_file_extensions, &["email", "Email", "émail", "Émail"]), sorted(locale_fn, &["email", "Email", "émail", "Émail"]), "the same base characters with different case or accents do not sort in locale order");
    // name plus extension comparisons
    assert!(c(comparers::compare_file_extensions, Some("a.MD"), Some("a.md")) < 0, "case differences in extensions sort in unicode order");
    assert!(c(comparers::compare_file_extensions, Some("a.md"), Some("A.md")) > 0, "case differences in names sort in unicode order");
    assert!(c(comparers::compare_file_extensions, Some("a.md"), Some("b.MD")) > 0, "when extensions are the same except for case, the files sort by extension");
    assert!(c(comparers::compare_file_extensions, Some("aggregate.go"), Some("aggregate_repo.go")) < 0, "when extensions are equal, names sort in dictionary order");
    // dotfile comparisons
    assert!(c(comparers::compare_file_extensions, Some(".env"), Some(".aaa.env")) < 0, "a dotfile with an extension is treated as a name plus an extension - equal extensions");
    assert!(c(comparers::compare_file_extensions, Some(".env"), Some(".env.aaa")) > 0, "a dotfile with an extension is treated as a name plus an extension - unequal extensions");
    // dotfile vs non-dotfile comparisons
    assert!(c(comparers::compare_file_extensions, Some(".env"), Some("aaa")) > 0, "filenames without extensions come before dotfiles");
    assert!(c(comparers::compare_file_extensions, Some(".md"), Some("A.MD")) > 0, "a file with an uppercase extension sorts before a dotfile of the same lowercase extension");
    // numeric comparisons
    assert!(c(comparers::compare_file_extensions, Some("abc.txt01"), Some("abc.txt1")) < 0, "extensions with equal numbers sort in unicode order");
    assert!(c(comparers::compare_file_extensions, Some("art01"), Some("Art01")) != locale_numeric("art01", "Art01"), "a numerically equivalent word of a different case does not compare by locale");
    assert!(c(comparers::compare_file_extensions, Some("abc02.txt"), Some("abc002.txt")) > 0, "filenames with equivalent numbers and leading zeros sort in unicode order");
    assert!(c(comparers::compare_file_extensions, Some("txt.abc01"), Some("txt.abc1")) < 0, "extensions with equivalent numbers sort in unicode order");
    assert!(c(comparers::compare_file_extensions, Some("a.ext1"), Some("b.Ext1")) > 0, "if names are different and extensions with numbers are equal except for case, filenames are sorted in extension unicode order");
    assert!(c(comparers::compare_file_extensions, Some("a.ext1"), Some("a.Ext1")) > 0, "if names are equal and extensions with numbers are equal except for case, filenames are sorted in extension unicode order");
}

#[test]
fn compare_file_names_default() {
    //
    // Comparisons with the same results as compareFileNames
    //

    // name-only comparisons
    assert!(c(comparers::compare_file_names_default, None, None) == 0, "null should be equal");
    assert!(c(comparers::compare_file_names_default, None, Some("abc")) < 0, "null should be come before real values");
    assert!(c(comparers::compare_file_names_default, Some(""), Some("")) == 0, "empty should be equal");
    assert!(c(comparers::compare_file_names_default, Some("abc"), Some("abc")) == 0, "equal names should be equal");
    assert!(c(comparers::compare_file_names_default, Some("z"), Some("A")) > 0, "z comes after A");
    assert!(c(comparers::compare_file_names_default, Some("Z"), Some("a")) > 0, "Z comes after a");
    // name plus extension comparisons
    assert!(c(comparers::compare_file_names_default, Some("file.ext"), Some("file.ext")) == 0, "equal full names should be equal");
    assert!(c(comparers::compare_file_names_default, Some("a.ext"), Some("b.ext")) < 0, "if equal extensions, filenames should be compared");
    assert!(c(comparers::compare_file_names_default, Some("file.aaa"), Some("file.bbb")) < 0, "files with equal names should be compared by extensions");
    assert!(c(comparers::compare_file_names_default, Some("bbb.aaa"), Some("aaa.bbb")) > 0, "files should be compared by names even if extensions compare differently");
    assert!(c(comparers::compare_file_names_default, Some("aggregate.go"), Some("aggregate_repo.go")) > 0, "compares the whole filename in locale order");
    // dotfile comparisons
    assert!(c(comparers::compare_file_names_default, Some(".abc"), Some(".abc")) == 0, "equal dotfile names should be equal");
    assert!(c(comparers::compare_file_names_default, Some(".env."), Some(".gitattributes")) < 0, "filenames starting with dots and with extensions should still sort properly");
    assert!(c(comparers::compare_file_names_default, Some(".env"), Some(".aaa.env")) > 0, "dotfiles sort alphabetically when they contain multiple dots");
    assert!(c(comparers::compare_file_names_default, Some(".env"), Some(".env.aaa")) < 0, "dotfiles with the same root sort shortest first");
    assert!(c(comparers::compare_file_names_default, Some(".aaa_env"), Some(".aaa.env")) < 0, "an underscore in a dotfile name will sort before a dot");
    // dotfile vs non-dotfile comparisons
    assert!(c(comparers::compare_file_names_default, None, Some(".abc")) < 0, "null should come before dotfiles");
    assert!(c(comparers::compare_file_names_default, Some(".env"), Some("aaa")) < 0, "dotfiles come before filenames without extensions");
    assert!(c(comparers::compare_file_names_default, Some(".env"), Some("aaa.env")) < 0, "dotfiles come before filenames with extensions");
    assert!(c(comparers::compare_file_names_default, Some(".md"), Some("A.MD")) < 0, "dotfiles sort before uppercase files");
    assert!(c(comparers::compare_file_names_default, Some(".MD"), Some("a.md")) < 0, "dotfiles sort before lowercase files");
    // numeric comparisons
    assert!(c(comparers::compare_file_names_default, Some("1"), Some("1")) == 0, "numerically equal full names should be equal");
    assert!(c(comparers::compare_file_names_default, Some("abc1.txt"), Some("abc1.txt")) == 0, "equal filenames with numbers should be equal");
    assert!(c(comparers::compare_file_names_default, Some("abc1.txt"), Some("abc2.txt")) < 0, "filenames with numbers should be in numerical order, not alphabetical order");
    assert!(c(comparers::compare_file_names_default, Some("abc2.txt"), Some("abc10.txt")) < 0, "filenames with numbers should be in numerical order even when they are multiple digits long");
    assert!(c(comparers::compare_file_names_default, Some("abc02.txt"), Some("abc010.txt")) < 0, "filenames with numbers that have leading zeros sort numerically");
    assert!(c(comparers::compare_file_names_default, Some("abc1.10.txt"), Some("abc1.2.txt")) > 0, "numbers with dots between them are treated as two separate numbers, not one decimal number");
    assert!(c(comparers::compare_file_names_default, Some("a.ext1"), Some("b.Ext1")) < 0, "if names are different and extensions with numbers are equal except for case, filenames are compared by full filename");
    assert_eq!(sorted(comparers::compare_file_names_default, &["a10.txt", "A2.txt", "A100.txt", "a20.txt"]), vec!["A2.txt", "a10.txt", "a20.txt", "A100.txt"], "filenames with number and case differences compare numerically");
    //
    // Comparisons with different results than compareFileNames
    //

    // name-only comparisons
    assert!(c(comparers::compare_file_names_default, Some("a"), Some("A")) == locale("a", "A"), "the same letter sorts by locale");
    assert!(c(comparers::compare_file_names_default, Some("â"), Some("Â")) == locale("â", "Â"), "the same accented letter sorts by locale");
    assert_eq!(sorted(comparers::compare_file_names_default, &["email", "Email", "émail", "Émail"]), sorted(locale_fn, &["email", "Email", "émail", "Émail"]), "the same base characters with different case or accents sort in locale order");
    // numeric comparisons
    assert!(c(comparers::compare_file_names_default, Some("abc02.txt"), Some("abc002.txt")) < 0, "filenames with equivalent numbers and leading zeros sort shortest number first");
    assert!(c(comparers::compare_file_names_default, Some("abc.txt1"), Some("abc.txt01")) < 0, "same name plus extensions with equal numbers sort shortest number first");
    assert!(c(comparers::compare_file_names_default, Some("art01"), Some("Art01")) == locale_numeric("art01", "Art01"), "a numerically equivalent word of a different case compares numerically based on locale");
    assert!(c(comparers::compare_file_names_default, Some("a.ext1"), Some("a.Ext1")) == locale("ext1", "Ext1"), "if names are equal and extensions with numbers are equal except for case, filenames are sorted in extension locale order");
}

#[test]
fn compare_file_extensions_default() {
    //
    // Comparisons with the same result as compareFileExtensions
    //

    // name-only comparisons
    assert!(c(comparers::compare_file_extensions_default, None, None) == 0, "null should be equal");
    assert!(c(comparers::compare_file_extensions_default, None, Some("abc")) < 0, "null should come before real files without extensions");
    assert!(c(comparers::compare_file_extensions_default, Some(""), Some("")) == 0, "empty should be equal");
    assert!(c(comparers::compare_file_extensions_default, Some("abc"), Some("abc")) == 0, "equal names should be equal");
    assert!(c(comparers::compare_file_extensions_default, Some("z"), Some("A")) > 0, "z comes after A");
    assert!(c(comparers::compare_file_extensions_default, Some("Z"), Some("a")) > 0, "Z comes after a");
    // name plus extension comparisons
    assert!(c(comparers::compare_file_extensions_default, Some("file.ext"), Some("file.ext")) == 0, "equal full filenames should be equal");
    assert!(c(comparers::compare_file_extensions_default, Some("a.ext"), Some("b.ext")) < 0, "if equal extensions, filenames should be compared");
    assert!(c(comparers::compare_file_extensions_default, Some("file.aaa"), Some("file.bbb")) < 0, "files with equal names should be compared by extensions");
    assert!(c(comparers::compare_file_extensions_default, Some("bbb.aaa"), Some("aaa.bbb")) < 0, "files should be compared by extension first");
    // dotfile comparisons
    assert!(c(comparers::compare_file_extensions_default, Some(".abc"), Some(".abc")) == 0, "equal dotfiles should be equal");
    assert!(c(comparers::compare_file_extensions_default, Some(".md"), Some(".Gitattributes")) > 0, "dotfiles sort alphabetically regardless of case");
    // dotfile vs non-dotfile comparisons
    assert!(c(comparers::compare_file_extensions_default, None, Some(".abc")) < 0, "null should come before dotfiles");
    assert!(c(comparers::compare_file_extensions_default, Some(".env"), Some("aaa.env")) < 0, "dotfiles come before filenames with extensions");
    assert!(c(comparers::compare_file_extensions_default, Some(".MD"), Some("a.md")) < 0, "dotfiles sort before lowercase files");
    // numeric comparisons
    assert!(c(comparers::compare_file_extensions_default, Some("1"), Some("1")) == 0, "numerically equal full names should be equal");
    assert!(c(comparers::compare_file_extensions_default, Some("abc1.txt"), Some("abc1.txt")) == 0, "equal filenames with numbers should be equal");
    assert!(c(comparers::compare_file_extensions_default, Some("abc1.txt"), Some("abc2.txt")) < 0, "filenames with numbers should be in numerical order, not alphabetical order");
    assert!(c(comparers::compare_file_extensions_default, Some("abc2.txt"), Some("abc10.txt")) < 0, "filenames with numbers should be in numerical order");
    assert!(c(comparers::compare_file_extensions_default, Some("abc02.txt"), Some("abc010.txt")) < 0, "filenames with numbers that have leading zeros sort numerically");
    assert!(c(comparers::compare_file_extensions_default, Some("abc1.10.txt"), Some("abc1.2.txt")) > 0, "numbers with dots between them are treated as two separate numbers, not one decimal number");
    assert!(c(comparers::compare_file_extensions_default, Some("abc2.txt2"), Some("abc1.txt10")) < 0, "extensions with numbers should be in numerical order, not alphabetical order");
    assert!(c(comparers::compare_file_extensions_default, Some("txt.abc1"), Some("txt.abc1")) == 0, "equal extensions with numbers should be equal");
    assert!(c(comparers::compare_file_extensions_default, Some("txt.abc1"), Some("txt.abc2")) < 0, "extensions with numbers should be in numerical order, not alphabetical order");
    assert!(c(comparers::compare_file_extensions_default, Some("txt.abc2"), Some("txt.abc10")) < 0, "extensions with numbers should be in numerical order even when they are multiple digits long");
    assert!(c(comparers::compare_file_extensions_default, Some("a.ext1"), Some("b.ext1")) < 0, "if equal extensions with numbers, full filenames should be compared");
    assert_eq!(sorted(comparers::compare_file_extensions_default, &["a10.txt", "A2.txt", "A100.txt", "a20.txt"]), vec!["A2.txt", "a10.txt", "a20.txt", "A100.txt"], "filenames with number and case differences compare numerically");
    //
    // Comparisons with different results than compareFileExtensions
    //

    // name-only comparisons
    assert!(c(comparers::compare_file_extensions_default, Some("a"), Some("A")) == locale("a", "A"), "the same letter of different case sorts by locale");
    assert!(c(comparers::compare_file_extensions_default, Some("â"), Some("Â")) == locale("â", "Â"), "the same accented letter of different case sorts by locale");
    assert_eq!(sorted(comparers::compare_file_extensions_default, &["email", "Email", "émail", "Émail"]), sorted(locale_fn, &["email", "Email", "émail", "Émail"]), "the same base characters with different case or accents sort in locale order");
    // name plus extension comparisons
    assert!(c(comparers::compare_file_extensions_default, Some("a.MD"), Some("a.md")) == locale("MD", "md"), "case differences in extensions sort by locale");
    assert!(c(comparers::compare_file_extensions_default, Some("a.md"), Some("A.md")) == locale("a", "A"), "case differences in names sort by locale");
    assert!(c(comparers::compare_file_extensions_default, Some("a.md"), Some("b.MD")) < 0, "when extensions are the same except for case, the files sort by name");
    assert!(c(comparers::compare_file_extensions_default, Some("aggregate.go"), Some("aggregate_repo.go")) > 0, "names with the same extension sort in full filename locale order");
    // dotfile comparisons
    assert!(c(comparers::compare_file_extensions_default, Some(".env"), Some(".aaa.env")) > 0, "dotfiles sort alphabetically when they contain multiple dots");
    assert!(c(comparers::compare_file_extensions_default, Some(".env"), Some(".env.aaa")) < 0, "dotfiles with the same root sort shortest first");
    // dotfile vs non-dotfile comparisons
    assert!(c(comparers::compare_file_extensions_default, Some(".env"), Some("aaa")) < 0, "dotfiles come before filenames without extensions");
    assert!(c(comparers::compare_file_extensions_default, Some(".md"), Some("A.MD")) < 0, "dotfiles sort before uppercase files");
    // numeric comparisons
    assert!(c(comparers::compare_file_extensions_default, Some("abc.txt01"), Some("abc.txt1")) > 0, "extensions with equal numbers should be in shortest-first order");
    assert!(c(comparers::compare_file_extensions_default, Some("art01"), Some("Art01")) == locale_numeric("art01", "Art01"), "a numerically equivalent word of a different case compares numerically based on locale");
    assert!(c(comparers::compare_file_extensions_default, Some("abc02.txt"), Some("abc002.txt")) < 0, "filenames with equivalent numbers and leading zeros sort shortest string first");
    assert!(c(comparers::compare_file_extensions_default, Some("txt.abc01"), Some("txt.abc1")) > 0, "extensions with equivalent numbers sort shortest extension first");
    assert!(c(comparers::compare_file_extensions_default, Some("a.ext1"), Some("b.Ext1")) < 0, "if extensions with numbers are equal except for case, full filenames should be compared");
    assert!(c(comparers::compare_file_extensions_default, Some("a.ext1"), Some("a.Ext1")) == locale("a.ext1", "a.Ext1"), "if extensions with numbers are equal except for case, full filenames are compared in locale order");
}

#[test]
fn compare_file_names_upper() {
    //
    // Comparisons with the same results as compareFileNamesDefault
    //

    // name-only comparisons
    assert!(c(comparers::compare_file_names_upper, None, None) == 0, "null should be equal");
    assert!(c(comparers::compare_file_names_upper, None, Some("abc")) < 0, "null should be come before real values");
    assert!(c(comparers::compare_file_names_upper, Some(""), Some("")) == 0, "empty should be equal");
    assert!(c(comparers::compare_file_names_upper, Some("abc"), Some("abc")) == 0, "equal names should be equal");
    assert!(c(comparers::compare_file_names_upper, Some("z"), Some("A")) > 0, "z comes after A");
    // name plus extension comparisons
    assert!(c(comparers::compare_file_names_upper, Some("file.ext"), Some("file.ext")) == 0, "equal full names should be equal");
    assert!(c(comparers::compare_file_names_upper, Some("a.ext"), Some("b.ext")) < 0, "if equal extensions, filenames should be compared");
    assert!(c(comparers::compare_file_names_upper, Some("file.aaa"), Some("file.bbb")) < 0, "files with equal names should be compared by extensions");
    assert!(c(comparers::compare_file_names_upper, Some("bbb.aaa"), Some("aaa.bbb")) > 0, "files should be compared by names even if extensions compare differently");
    assert!(c(comparers::compare_file_names_upper, Some("aggregate.go"), Some("aggregate_repo.go")) > 0, "compares the full filename in locale order");
    // dotfile comparisons
    assert!(c(comparers::compare_file_names_upper, Some(".abc"), Some(".abc")) == 0, "equal dotfile names should be equal");
    assert!(c(comparers::compare_file_names_upper, Some(".env."), Some(".gitattributes")) < 0, "filenames starting with dots and with extensions should still sort properly");
    assert!(c(comparers::compare_file_names_upper, Some(".env"), Some(".aaa.env")) > 0, "dotfiles sort alphabetically when they contain multiple dots");
    assert!(c(comparers::compare_file_names_upper, Some(".env"), Some(".env.aaa")) < 0, "dotfiles with the same root sort shortest first");
    assert!(c(comparers::compare_file_names_upper, Some(".aaa_env"), Some(".aaa.env")) < 0, "an underscore in a dotfile name will sort before a dot");
    // dotfile vs non-dotfile comparisons
    assert!(c(comparers::compare_file_names_upper, None, Some(".abc")) < 0, "null should come before dotfiles");
    assert!(c(comparers::compare_file_names_upper, Some(".env"), Some("aaa")) < 0, "dotfiles come before filenames without extensions");
    assert!(c(comparers::compare_file_names_upper, Some(".env"), Some("aaa.env")) < 0, "dotfiles come before filenames with extensions");
    assert!(c(comparers::compare_file_names_upper, Some(".md"), Some("A.MD")) < 0, "dotfiles sort before uppercase files");
    assert!(c(comparers::compare_file_names_upper, Some(".MD"), Some("a.md")) < 0, "dotfiles sort before lowercase files");
    // numeric comparisons
    assert!(c(comparers::compare_file_names_upper, Some("1"), Some("1")) == 0, "numerically equal full names should be equal");
    assert!(c(comparers::compare_file_names_upper, Some("abc1.txt"), Some("abc1.txt")) == 0, "equal filenames with numbers should be equal");
    assert!(c(comparers::compare_file_names_upper, Some("abc1.txt"), Some("abc2.txt")) < 0, "filenames with numbers should be in numerical order, not alphabetical order");
    assert!(c(comparers::compare_file_names_upper, Some("abc2.txt"), Some("abc10.txt")) < 0, "filenames with numbers should be in numerical order even when they are multiple digits long");
    assert!(c(comparers::compare_file_names_upper, Some("abc02.txt"), Some("abc010.txt")) < 0, "filenames with numbers that have leading zeros sort numerically");
    assert!(c(comparers::compare_file_names_upper, Some("abc1.10.txt"), Some("abc1.2.txt")) > 0, "numbers with dots between them are treated as two separate numbers, not one decimal number");
    assert!(c(comparers::compare_file_names_upper, Some("abc02.txt"), Some("abc002.txt")) < 0, "filenames with equivalent numbers and leading zeros sort shortest number first");
    assert!(c(comparers::compare_file_names_upper, Some("abc.txt1"), Some("abc.txt01")) < 0, "same name plus extensions with equal numbers sort shortest number first");
    assert!(c(comparers::compare_file_names_upper, Some("a.ext1"), Some("b.Ext1")) < 0, "different names with the equal extensions except for case are sorted by full filename");
    assert!(c(comparers::compare_file_names_upper, Some("a.ext1"), Some("a.Ext1")) == locale("a.ext1", "a.Ext1"), "same names with equal and extensions except for case are sorted in full filename locale order");
    //
    // Comparisons with different results than compareFileNamesDefault
    //

    // name-only comparisons
    assert!(c(comparers::compare_file_names_upper, Some("Z"), Some("a")) < 0, "Z comes before a");
    assert!(c(comparers::compare_file_names_upper, Some("a"), Some("A")) > 0, "the same letter sorts uppercase first");
    assert!(c(comparers::compare_file_names_upper, Some("â"), Some("Â")) > 0, "the same accented letter sorts uppercase first");
    assert_eq!(sorted(comparers::compare_file_names_upper, &["artichoke", "Artichoke", "art", "Art"]), vec!["Art", "Artichoke", "art", "artichoke"], "names with the same root and different cases sort uppercase first");
    assert_eq!(sorted(comparers::compare_file_names_upper, &["email", "Email", "émail", "Émail"]), vec!["Email", "Émail", "email", "émail"], "the same base characters with different case or accents sort uppercase first");
    // numeric comparisons
    assert!(c(comparers::compare_file_names_upper, Some("art01"), Some("Art01")) > 0, "a numerically equivalent name of a different case compares uppercase first");
    assert_eq!(sorted(comparers::compare_file_names_upper, &["a10.txt", "A2.txt", "A100.txt", "a20.txt"]), vec!["A2.txt", "A100.txt", "a10.txt", "a20.txt"], "filenames with number and case differences group by case then compare by number");
}

#[test]
fn compare_file_extensions_upper() {
    //
    // Comparisons with the same result as compareFileExtensionsDefault
    //

    // name-only comparisons
    assert!(c(comparers::compare_file_extensions_upper, None, None) == 0, "null should be equal");
    assert!(c(comparers::compare_file_extensions_upper, None, Some("abc")) < 0, "null should come before real files without extensions");
    assert!(c(comparers::compare_file_extensions_upper, Some(""), Some("")) == 0, "empty should be equal");
    assert!(c(comparers::compare_file_extensions_upper, Some("abc"), Some("abc")) == 0, "equal names should be equal");
    assert!(c(comparers::compare_file_extensions_upper, Some("z"), Some("A")) > 0, "z comes after A");
    // name plus extension comparisons
    assert!(c(comparers::compare_file_extensions_upper, Some("file.ext"), Some("file.ext")) == 0, "equal full filenames should be equal");
    assert!(c(comparers::compare_file_extensions_upper, Some("a.ext"), Some("b.ext")) < 0, "if equal extensions, filenames should be compared");
    assert!(c(comparers::compare_file_extensions_upper, Some("file.aaa"), Some("file.bbb")) < 0, "files with equal names should be compared by extensions");
    assert!(c(comparers::compare_file_extensions_upper, Some("bbb.aaa"), Some("aaa.bbb")) < 0, "files should be compared by extension first");
    assert!(c(comparers::compare_file_extensions_upper, Some("a.md"), Some("b.MD")) < 0, "when extensions are the same except for case, the files sort by name");
    assert!(c(comparers::compare_file_extensions_upper, Some("a.MD"), Some("a.md")) == locale("MD", "md"), "case differences in extensions sort by locale");
    assert!(c(comparers::compare_file_extensions_upper, Some("aggregate.go"), Some("aggregate_repo.go")) > 0, "when extensions are equal, compares the full filename");
    // dotfile comparisons
    assert!(c(comparers::compare_file_extensions_upper, Some(".abc"), Some(".abc")) == 0, "equal dotfiles should be equal");
    assert!(c(comparers::compare_file_extensions_upper, Some(".md"), Some(".Gitattributes")) > 0, "dotfiles sort alphabetically regardless of case");
    assert!(c(comparers::compare_file_extensions_upper, Some(".env"), Some(".aaa.env")) > 0, "dotfiles sort alphabetically when they contain multiple dots");
    assert!(c(comparers::compare_file_extensions_upper, Some(".env"), Some(".env.aaa")) < 0, "dotfiles with the same root sort shortest first");
    // dotfile vs non-dotfile comparisons
    assert!(c(comparers::compare_file_extensions_upper, None, Some(".abc")) < 0, "null should come before dotfiles");
    assert!(c(comparers::compare_file_extensions_upper, Some(".env"), Some("aaa.env")) < 0, "dotfiles come before filenames with extensions");
    assert!(c(comparers::compare_file_extensions_upper, Some(".MD"), Some("a.md")) < 0, "dotfiles sort before lowercase files");
    assert!(c(comparers::compare_file_extensions_upper, Some(".env"), Some("aaa")) < 0, "dotfiles come before filenames without extensions");
    assert!(c(comparers::compare_file_extensions_upper, Some(".md"), Some("A.MD")) < 0, "dotfiles sort before uppercase files");
    // numeric comparisons
    assert!(c(comparers::compare_file_extensions_upper, Some("1"), Some("1")) == 0, "numerically equal full names should be equal");
    assert!(c(comparers::compare_file_extensions_upper, Some("abc1.txt"), Some("abc1.txt")) == 0, "equal filenames with numbers should be equal");
    assert!(c(comparers::compare_file_extensions_upper, Some("abc1.txt"), Some("abc2.txt")) < 0, "filenames with numbers should be in numerical order, not alphabetical order");
    assert!(c(comparers::compare_file_extensions_upper, Some("abc2.txt"), Some("abc10.txt")) < 0, "filenames with numbers should be in numerical order");
    assert!(c(comparers::compare_file_extensions_upper, Some("abc02.txt"), Some("abc010.txt")) < 0, "filenames with numbers that have leading zeros sort numerically");
    assert!(c(comparers::compare_file_extensions_upper, Some("abc1.10.txt"), Some("abc1.2.txt")) > 0, "numbers with dots between them are treated as two separate numbers, not one decimal number");
    assert!(c(comparers::compare_file_extensions_upper, Some("abc2.txt2"), Some("abc1.txt10")) < 0, "extensions with numbers should be in numerical order, not alphabetical order");
    assert!(c(comparers::compare_file_extensions_upper, Some("txt.abc1"), Some("txt.abc1")) == 0, "equal extensions with numbers should be equal");
    assert!(c(comparers::compare_file_extensions_upper, Some("txt.abc1"), Some("txt.abc2")) < 0, "extensions with numbers should be in numerical order, not alphabetical order");
    assert!(c(comparers::compare_file_extensions_upper, Some("txt.abc2"), Some("txt.abc10")) < 0, "extensions with numbers should be in numerical order even when they are multiple digits long");
    assert!(c(comparers::compare_file_extensions_upper, Some("a.ext1"), Some("b.ext1")) < 0, "if equal extensions with numbers, full filenames should be compared");
    assert!(c(comparers::compare_file_extensions_upper, Some("abc.txt01"), Some("abc.txt1")) > 0, "extensions with equal numbers should be in shortest-first order");
    assert!(c(comparers::compare_file_extensions_upper, Some("abc02.txt"), Some("abc002.txt")) < 0, "filenames with equivalent numbers and leading zeros sort shortest string first");
    assert!(c(comparers::compare_file_extensions_upper, Some("txt.abc01"), Some("txt.abc1")) > 0, "extensions with equivalent numbers sort shortest extension first");
    assert!(c(comparers::compare_file_extensions_upper, Some("a.ext1"), Some("b.Ext1")) < 0, "different names and extensions that are equal except for case are sorted in full filename order");
    assert!(c(comparers::compare_file_extensions_upper, Some("a.ext1"), Some("a.Ext1")) == locale("a.ext1", "b.Ext1"), "same names and extensions that are equal except for case are sorted in full filename locale order");
    //
    // Comparisons with different results than compareFileExtensionsDefault
    //

    // name-only comparisons
    assert!(c(comparers::compare_file_extensions_upper, Some("Z"), Some("a")) < 0, "Z comes before a");
    assert!(c(comparers::compare_file_extensions_upper, Some("a"), Some("A")) > 0, "the same letter sorts uppercase first");
    assert!(c(comparers::compare_file_extensions_upper, Some("â"), Some("Â")) > 0, "the same accented letter sorts uppercase first");
    assert_eq!(sorted(comparers::compare_file_extensions_upper, &["artichoke", "Artichoke", "art", "Art"]), vec!["Art", "Artichoke", "art", "artichoke"], "names with the same root and different cases sort uppercase names first");
    assert_eq!(sorted(comparers::compare_file_extensions_upper, &["email", "Email", "émail", "Émail"]), vec!["Email", "Émail", "email", "émail"], "the same base characters with different case or accents sort uppercase names first");
    // name plus extension comparisons
    assert!(c(comparers::compare_file_extensions_upper, Some("a.md"), Some("A.md")) > 0, "case differences in names sort uppercase first");
    assert!(c(comparers::compare_file_extensions_upper, Some("art01"), Some("Art01")) > 0, "a numerically equivalent word of a different case sorts uppercase first");
    assert_eq!(sorted(comparers::compare_file_extensions_upper, &["a10.txt", "A2.txt", "A100.txt", "a20.txt"]), vec!["A2.txt", "A100.txt", "a10.txt", "a20.txt"], "filenames with number and case differences group by case then sort by number");
}

#[test]
fn compare_file_names_lower() {
    //
    // Comparisons with the same results as compareFileNamesDefault
    //

    // name-only comparisons
    assert!(c(comparers::compare_file_names_lower, None, None) == 0, "null should be equal");
    assert!(c(comparers::compare_file_names_lower, None, Some("abc")) < 0, "null should be come before real values");
    assert!(c(comparers::compare_file_names_lower, Some(""), Some("")) == 0, "empty should be equal");
    assert!(c(comparers::compare_file_names_lower, Some("abc"), Some("abc")) == 0, "equal names should be equal");
    assert!(c(comparers::compare_file_names_lower, Some("Z"), Some("a")) > 0, "Z comes after a");
    // name plus extension comparisons
    assert!(c(comparers::compare_file_names_lower, Some("file.ext"), Some("file.ext")) == 0, "equal full names should be equal");
    assert!(c(comparers::compare_file_names_lower, Some("a.ext"), Some("b.ext")) < 0, "if equal extensions, filenames should be compared");
    assert!(c(comparers::compare_file_names_lower, Some("file.aaa"), Some("file.bbb")) < 0, "files with equal names should be compared by extensions");
    assert!(c(comparers::compare_file_names_lower, Some("bbb.aaa"), Some("aaa.bbb")) > 0, "files should be compared by names even if extensions compare differently");
    assert!(c(comparers::compare_file_names_lower, Some("aggregate.go"), Some("aggregate_repo.go")) > 0, "compares full filenames");
    // dotfile comparisons
    assert!(c(comparers::compare_file_names_lower, Some(".abc"), Some(".abc")) == 0, "equal dotfile names should be equal");
    assert!(c(comparers::compare_file_names_lower, Some(".env."), Some(".gitattributes")) < 0, "filenames starting with dots and with extensions should still sort properly");
    assert!(c(comparers::compare_file_names_lower, Some(".env"), Some(".aaa.env")) > 0, "dotfiles sort alphabetically when they contain multiple dots");
    assert!(c(comparers::compare_file_names_lower, Some(".env"), Some(".env.aaa")) < 0, "dotfiles with the same root sort shortest first");
    assert!(c(comparers::compare_file_names_lower, Some(".aaa_env"), Some(".aaa.env")) < 0, "an underscore in a dotfile name will sort before a dot");
    // dotfile vs non-dotfile comparisons
    assert!(c(comparers::compare_file_names_lower, None, Some(".abc")) < 0, "null should come before dotfiles");
    assert!(c(comparers::compare_file_names_lower, Some(".env"), Some("aaa")) < 0, "dotfiles come before filenames without extensions");
    assert!(c(comparers::compare_file_names_lower, Some(".env"), Some("aaa.env")) < 0, "dotfiles come before filenames with extensions");
    assert!(c(comparers::compare_file_names_lower, Some(".md"), Some("A.MD")) < 0, "dotfiles sort before uppercase files");
    assert!(c(comparers::compare_file_names_lower, Some(".MD"), Some("a.md")) < 0, "dotfiles sort before lowercase files");
    // numeric comparisons
    assert!(c(comparers::compare_file_names_lower, Some("1"), Some("1")) == 0, "numerically equal full names should be equal");
    assert!(c(comparers::compare_file_names_lower, Some("abc1.txt"), Some("abc1.txt")) == 0, "equal filenames with numbers should be equal");
    assert!(c(comparers::compare_file_names_lower, Some("abc1.txt"), Some("abc2.txt")) < 0, "filenames with numbers should be in numerical order, not alphabetical order");
    assert!(c(comparers::compare_file_names_lower, Some("abc2.txt"), Some("abc10.txt")) < 0, "filenames with numbers should be in numerical order even when they are multiple digits long");
    assert!(c(comparers::compare_file_names_lower, Some("abc02.txt"), Some("abc010.txt")) < 0, "filenames with numbers that have leading zeros sort numerically");
    assert!(c(comparers::compare_file_names_lower, Some("abc1.10.txt"), Some("abc1.2.txt")) > 0, "numbers with dots between them are treated as two separate numbers, not one decimal number");
    assert!(c(comparers::compare_file_names_lower, Some("abc02.txt"), Some("abc002.txt")) < 0, "filenames with equivalent numbers and leading zeros sort shortest number first");
    assert!(c(comparers::compare_file_names_lower, Some("abc.txt1"), Some("abc.txt01")) < 0, "same name plus extensions with equal numbers sort shortest number first");
    assert!(c(comparers::compare_file_names_lower, Some("a.ext1"), Some("b.Ext1")) < 0, "different names and extensions that are equal except for case are sorted in full filename order");
    assert!(c(comparers::compare_file_names_lower, Some("a.ext1"), Some("a.Ext1")) == locale("a.ext1", "b.Ext1"), "same names and extensions that are equal except for case are sorted in full filename locale order");
    //
    // Comparisons with different results than compareFileNamesDefault
    //

    // name-only comparisons
    assert!(c(comparers::compare_file_names_lower, Some("z"), Some("A")) < 0, "z comes before A");
    assert!(c(comparers::compare_file_names_lower, Some("a"), Some("A")) < 0, "the same letter sorts lowercase first");
    assert!(c(comparers::compare_file_names_lower, Some("â"), Some("Â")) < 0, "the same accented letter sorts lowercase first");
    assert_eq!(sorted(comparers::compare_file_names_lower, &["artichoke", "Artichoke", "art", "Art"]), vec!["art", "artichoke", "Art", "Artichoke"], "names with the same root and different cases sort lowercase first");
    assert_eq!(sorted(comparers::compare_file_names_lower, &["email", "Email", "émail", "Émail"]), vec!["email", "émail", "Email", "Émail"], "the same base characters with different case or accents sort lowercase first");
    // numeric comparisons
    assert!(c(comparers::compare_file_names_lower, Some("art01"), Some("Art01")) < 0, "a numerically equivalent name of a different case compares lowercase first");
    assert_eq!(sorted(comparers::compare_file_names_lower, &["a10.txt", "A2.txt", "A100.txt", "a20.txt"]), vec!["a10.txt", "a20.txt", "A2.txt", "A100.txt"], "filenames with number and case differences group by case then compare by number");
}

#[test]
fn compare_file_extensions_lower() {
    //
    // Comparisons with the same result as compareFileExtensionsDefault
    //

    // name-only comparisons
    assert!(c(comparers::compare_file_extensions_lower, None, None) == 0, "null should be equal");
    assert!(c(comparers::compare_file_extensions_lower, None, Some("abc")) < 0, "null should come before real files without extensions");
    assert!(c(comparers::compare_file_extensions_lower, Some(""), Some("")) == 0, "empty should be equal");
    assert!(c(comparers::compare_file_extensions_lower, Some("abc"), Some("abc")) == 0, "equal names should be equal");
    assert!(c(comparers::compare_file_extensions_lower, Some("Z"), Some("a")) > 0, "Z comes after a");
    // name plus extension comparisons
    assert!(c(comparers::compare_file_extensions_lower, Some("file.ext"), Some("file.ext")) == 0, "equal full filenames should be equal");
    assert!(c(comparers::compare_file_extensions_lower, Some("a.ext"), Some("b.ext")) < 0, "if equal extensions, filenames should be compared");
    assert!(c(comparers::compare_file_extensions_lower, Some("file.aaa"), Some("file.bbb")) < 0, "files with equal names should be compared by extensions");
    assert!(c(comparers::compare_file_extensions_lower, Some("bbb.aaa"), Some("aaa.bbb")) < 0, "files should be compared by extension first");
    assert!(c(comparers::compare_file_extensions_lower, Some("a.md"), Some("b.MD")) < 0, "when extensions are the same except for case, the files sort by name");
    assert!(c(comparers::compare_file_extensions_lower, Some("a.MD"), Some("a.md")) == locale("MD", "md"), "case differences in extensions sort by locale");
    // dotfile comparisons
    assert!(c(comparers::compare_file_extensions_lower, Some(".abc"), Some(".abc")) == 0, "equal dotfiles should be equal");
    assert!(c(comparers::compare_file_extensions_lower, Some(".md"), Some(".Gitattributes")) > 0, "dotfiles sort alphabetically regardless of case");
    assert!(c(comparers::compare_file_extensions_lower, Some(".env"), Some(".aaa.env")) > 0, "dotfiles sort alphabetically when they contain multiple dots");
    assert!(c(comparers::compare_file_extensions_lower, Some(".env"), Some(".env.aaa")) < 0, "dotfiles with the same root sort shortest first");
    // dotfile vs non-dotfile comparisons
    assert!(c(comparers::compare_file_extensions_lower, None, Some(".abc")) < 0, "null should come before dotfiles");
    assert!(c(comparers::compare_file_extensions_lower, Some(".env"), Some("aaa.env")) < 0, "dotfiles come before filenames with extensions");
    assert!(c(comparers::compare_file_extensions_lower, Some(".MD"), Some("a.md")) < 0, "dotfiles sort before lowercase files");
    assert!(c(comparers::compare_file_extensions_lower, Some(".env"), Some("aaa")) < 0, "dotfiles come before filenames without extensions");
    assert!(c(comparers::compare_file_extensions_lower, Some(".md"), Some("A.MD")) < 0, "dotfiles sort before uppercase files");
    // numeric comparisons
    assert!(c(comparers::compare_file_extensions_lower, Some("1"), Some("1")) == 0, "numerically equal full names should be equal");
    assert!(c(comparers::compare_file_extensions_lower, Some("abc1.txt"), Some("abc1.txt")) == 0, "equal filenames with numbers should be equal");
    assert!(c(comparers::compare_file_extensions_lower, Some("abc1.txt"), Some("abc2.txt")) < 0, "filenames with numbers should be in numerical order, not alphabetical order");
    assert!(c(comparers::compare_file_extensions_lower, Some("abc2.txt"), Some("abc10.txt")) < 0, "filenames with numbers should be in numerical order");
    assert!(c(comparers::compare_file_extensions_lower, Some("abc02.txt"), Some("abc010.txt")) < 0, "filenames with numbers that have leading zeros sort numerically");
    assert!(c(comparers::compare_file_extensions_lower, Some("abc1.10.txt"), Some("abc1.2.txt")) > 0, "numbers with dots between them are treated as two separate numbers, not one decimal number");
    assert!(c(comparers::compare_file_extensions_lower, Some("abc2.txt2"), Some("abc1.txt10")) < 0, "extensions with numbers should be in numerical order, not alphabetical order");
    assert!(c(comparers::compare_file_extensions_lower, Some("txt.abc1"), Some("txt.abc1")) == 0, "equal extensions with numbers should be equal");
    assert!(c(comparers::compare_file_extensions_lower, Some("txt.abc1"), Some("txt.abc2")) < 0, "extensions with numbers should be in numerical order, not alphabetical order");
    assert!(c(comparers::compare_file_extensions_lower, Some("txt.abc2"), Some("txt.abc10")) < 0, "extensions with numbers should be in numerical order even when they are multiple digits long");
    assert!(c(comparers::compare_file_extensions_lower, Some("a.ext1"), Some("b.ext1")) < 0, "if equal extensions with numbers, full filenames should be compared");
    assert!(c(comparers::compare_file_extensions_lower, Some("abc.txt01"), Some("abc.txt1")) > 0, "extensions with equal numbers should be in shortest-first order");
    assert!(c(comparers::compare_file_extensions_lower, Some("abc02.txt"), Some("abc002.txt")) < 0, "filenames with equivalent numbers and leading zeros sort shortest string first");
    assert!(c(comparers::compare_file_extensions_lower, Some("txt.abc01"), Some("txt.abc1")) > 0, "extensions with equivalent numbers sort shortest extension first");
    assert!(c(comparers::compare_file_extensions_lower, Some("a.ext1"), Some("b.Ext1")) < 0, "if extensions with numbers are equal except for case, full filenames should be compared");
    assert!(c(comparers::compare_file_extensions_lower, Some("a.ext1"), Some("a.Ext1")) == locale("a.ext1", "a.Ext1"), "if extensions with numbers are equal except for case, filenames are sorted in locale order");
    //
    // Comparisons with different results than compareFileExtensionsDefault
    //

    // name-only comparisons
    assert!(c(comparers::compare_file_extensions_lower, Some("z"), Some("A")) < 0, "z comes before A");
    assert!(c(comparers::compare_file_extensions_lower, Some("a"), Some("A")) < 0, "the same letter sorts lowercase first");
    assert!(c(comparers::compare_file_extensions_lower, Some("â"), Some("Â")) < 0, "the same accented letter sorts lowercase first");
    assert_eq!(sorted(comparers::compare_file_extensions_lower, &["artichoke", "Artichoke", "art", "Art"]), vec!["art", "artichoke", "Art", "Artichoke"], "names with the same root and different cases sort lowercase names first");
    assert_eq!(sorted(comparers::compare_file_extensions_lower, &["email", "Email", "émail", "Émail"]), vec!["email", "émail", "Email", "Émail"], "the same base characters with different case or accents sort lowercase names first");
    // name plus extension comparisons
    assert!(c(comparers::compare_file_extensions_lower, Some("a.md"), Some("A.md")) < 0, "case differences in names sort lowercase first");
    assert!(c(comparers::compare_file_extensions_lower, Some("art01"), Some("Art01")) < 0, "a numerically equivalent word of a different case sorts lowercase first");
    assert_eq!(sorted(comparers::compare_file_extensions_lower, &["a10.txt", "A2.txt", "A100.txt", "a20.txt"]), vec!["a10.txt", "a20.txt", "A2.txt", "A100.txt"], "filenames with number and case differences group by case then sort by number");
    assert!(c(comparers::compare_file_extensions_lower, Some("aggregate.go"), Some("aggregate_repo.go")) > 0, "when extensions are equal, compares full filenames");
}

#[test]
fn compare_file_names_unicode() {
    //
    // Comparisons with the same results as compareFileNamesDefault
    //

    // name-only comparisons
    assert!(c(comparers::compare_file_names_unicode, None, None) == 0, "null should be equal");
    assert!(c(comparers::compare_file_names_unicode, None, Some("abc")) < 0, "null should be come before real values");
    assert!(c(comparers::compare_file_names_unicode, Some(""), Some("")) == 0, "empty should be equal");
    assert!(c(comparers::compare_file_names_unicode, Some("abc"), Some("abc")) == 0, "equal names should be equal");
    assert!(c(comparers::compare_file_names_unicode, Some("z"), Some("A")) > 0, "z comes after A");
    // name plus extension comparisons
    assert!(c(comparers::compare_file_names_unicode, Some("file.ext"), Some("file.ext")) == 0, "equal full names should be equal");
    assert!(c(comparers::compare_file_names_unicode, Some("a.ext"), Some("b.ext")) < 0, "if equal extensions, filenames should be compared");
    assert!(c(comparers::compare_file_names_unicode, Some("file.aaa"), Some("file.bbb")) < 0, "files with equal names should be compared by extensions");
    assert!(c(comparers::compare_file_names_unicode, Some("bbb.aaa"), Some("aaa.bbb")) > 0, "files should be compared by names even if extensions compare differently");
    // dotfile comparisons
    assert!(c(comparers::compare_file_names_unicode, Some(".abc"), Some(".abc")) == 0, "equal dotfile names should be equal");
    assert!(c(comparers::compare_file_names_unicode, Some(".env."), Some(".gitattributes")) < 0, "filenames starting with dots and with extensions should still sort properly");
    assert!(c(comparers::compare_file_names_unicode, Some(".env"), Some(".aaa.env")) > 0, "dotfiles sort alphabetically when they contain multiple dots");
    assert!(c(comparers::compare_file_names_unicode, Some(".env"), Some(".env.aaa")) < 0, "dotfiles with the same root sort shortest first");
    // dotfile vs non-dotfile comparisons
    assert!(c(comparers::compare_file_names_unicode, None, Some(".abc")) < 0, "null should come before dotfiles");
    assert!(c(comparers::compare_file_names_unicode, Some(".env"), Some("aaa")) < 0, "dotfiles come before filenames without extensions");
    assert!(c(comparers::compare_file_names_unicode, Some(".env"), Some("aaa.env")) < 0, "dotfiles come before filenames with extensions");
    assert!(c(comparers::compare_file_names_unicode, Some(".md"), Some("A.MD")) < 0, "dotfiles sort before uppercase files");
    assert!(c(comparers::compare_file_names_unicode, Some(".MD"), Some("a.md")) < 0, "dotfiles sort before lowercase files");
    // numeric comparisons
    assert!(c(comparers::compare_file_names_unicode, Some("1"), Some("1")) == 0, "numerically equal full names should be equal");
    assert!(c(comparers::compare_file_names_unicode, Some("abc1.txt"), Some("abc1.txt")) == 0, "equal filenames with numbers should be equal");
    assert!(c(comparers::compare_file_names_unicode, Some("abc1.txt"), Some("abc2.txt")) < 0, "filenames with numbers should be in numerical order, not alphabetical order");
    assert!(c(comparers::compare_file_names_unicode, Some("a.ext1"), Some("b.Ext1")) < 0, "if names are different and extensions with numbers are equal except for case, filenames are sorted by unicode full filename");
    assert!(c(comparers::compare_file_names_unicode, Some("a.ext1"), Some("a.Ext1")) > 0, "if names are equal and extensions with numbers are equal except for case, filenames are sorted by unicode full filename");
    //
    // Comparisons with different results than compareFileNamesDefault
    //

    // name-only comparisons
    assert!(c(comparers::compare_file_names_unicode, Some("Z"), Some("a")) < 0, "Z comes before a");
    assert!(c(comparers::compare_file_names_unicode, Some("a"), Some("A")) > 0, "the same letter sorts uppercase first");
    assert!(c(comparers::compare_file_names_unicode, Some("â"), Some("Â")) > 0, "the same accented letter sorts uppercase first");
    assert_eq!(sorted(comparers::compare_file_names_unicode, &["artichoke", "Artichoke", "art", "Art"]), vec!["Art", "Artichoke", "art", "artichoke"], "names with the same root and different cases sort uppercase first");
    assert_eq!(sorted(comparers::compare_file_names_unicode, &["email", "Email", "émail", "Émail"]), vec!["Email", "email", "Émail", "émail"], "the same base characters with different case or accents sort in unicode order");
    // name plus extension comparisons
    assert!(c(comparers::compare_file_names_unicode, Some("aggregate.go"), Some("aggregate_repo.go")) < 0, "compares the whole name in unicode order, but dot comes before underscore");
    // dotfile comparisons
    assert!(c(comparers::compare_file_names_unicode, Some(".aaa_env"), Some(".aaa.env")) > 0, "an underscore in a dotfile name will sort after a dot");
    // numeric comparisons
    assert!(c(comparers::compare_file_names_unicode, Some("abc2.txt"), Some("abc10.txt")) > 0, "filenames with numbers should be in unicode order even when they are multiple digits long");
    assert!(c(comparers::compare_file_names_unicode, Some("abc02.txt"), Some("abc010.txt")) > 0, "filenames with numbers that have leading zeros sort in unicode order");
    assert!(c(comparers::compare_file_names_unicode, Some("abc1.10.txt"), Some("abc1.2.txt")) < 0, "numbers with dots between them are sorted in unicode order");
    assert!(c(comparers::compare_file_names_unicode, Some("abc02.txt"), Some("abc002.txt")) > 0, "filenames with equivalent numbers and leading zeros sort in unicode order");
    assert!(c(comparers::compare_file_names_unicode, Some("abc.txt1"), Some("abc.txt01")) > 0, "same name plus extensions with equal numbers sort in unicode order");
    assert!(c(comparers::compare_file_names_unicode, Some("art01"), Some("Art01")) > 0, "a numerically equivalent name of a different case compares uppercase first");
    assert_eq!(sorted(comparers::compare_file_names_unicode, &["a10.txt", "A2.txt", "A100.txt", "a20.txt"]), vec!["A100.txt", "A2.txt", "a10.txt", "a20.txt"], "filenames with number and case differences sort in unicode order");
}

#[test]
fn compare_file_extensions_unicode() {
    //
    // Comparisons with the same result as compareFileExtensionsDefault
    //

    // name-only comparisons
    assert!(c(comparers::compare_file_extensions_unicode, None, None) == 0, "null should be equal");
    assert!(c(comparers::compare_file_extensions_unicode, None, Some("abc")) < 0, "null should come before real files without extensions");
    assert!(c(comparers::compare_file_extensions_unicode, Some(""), Some("")) == 0, "empty should be equal");
    assert!(c(comparers::compare_file_extensions_unicode, Some("abc"), Some("abc")) == 0, "equal names should be equal");
    assert!(c(comparers::compare_file_extensions_unicode, Some("z"), Some("A")) > 0, "z comes after A");
    // name plus extension comparisons
    assert!(c(comparers::compare_file_extensions_unicode, Some("file.ext"), Some("file.ext")) == 0, "equal full filenames should be equal");
    assert!(c(comparers::compare_file_extensions_unicode, Some("a.ext"), Some("b.ext")) < 0, "if equal extensions, filenames should be compared");
    assert!(c(comparers::compare_file_extensions_unicode, Some("file.aaa"), Some("file.bbb")) < 0, "files with equal names should be compared by extensions");
    assert!(c(comparers::compare_file_extensions_unicode, Some("bbb.aaa"), Some("aaa.bbb")) < 0, "files should be compared by extension first");
    assert!(c(comparers::compare_file_extensions_unicode, Some("a.md"), Some("b.MD")) < 0, "when extensions are the same except for case, the files sort by name");
    assert!(c(comparers::compare_file_extensions_unicode, Some("a.MD"), Some("a.md")) < 0, "case differences in extensions sort in unicode order");
    // dotfile comparisons
    assert!(c(comparers::compare_file_extensions_unicode, Some(".abc"), Some(".abc")) == 0, "equal dotfiles should be equal");
    assert!(c(comparers::compare_file_extensions_unicode, Some(".md"), Some(".Gitattributes")) > 0, "dotfiles sort alphabetically regardless of case");
    assert!(c(comparers::compare_file_extensions_unicode, Some(".env"), Some(".aaa.env")) > 0, "dotfiles sort alphabetically when they contain multiple dots");
    assert!(c(comparers::compare_file_extensions_unicode, Some(".env"), Some(".env.aaa")) < 0, "dotfiles with the same root sort shortest first");
    // dotfile vs non-dotfile comparisons
    assert!(c(comparers::compare_file_extensions_unicode, None, Some(".abc")) < 0, "null should come before dotfiles");
    assert!(c(comparers::compare_file_extensions_unicode, Some(".env"), Some("aaa.env")) < 0, "dotfiles come before filenames with extensions");
    assert!(c(comparers::compare_file_extensions_unicode, Some(".MD"), Some("a.md")) < 0, "dotfiles sort before lowercase files");
    assert!(c(comparers::compare_file_extensions_unicode, Some(".env"), Some("aaa")) < 0, "dotfiles come before filenames without extensions");
    assert!(c(comparers::compare_file_extensions_unicode, Some(".md"), Some("A.MD")) < 0, "dotfiles sort before uppercase files");
    // numeric comparisons
    assert!(c(comparers::compare_file_extensions_unicode, Some("1"), Some("1")) == 0, "numerically equal full names should be equal");
    assert!(c(comparers::compare_file_extensions_unicode, Some("abc1.txt"), Some("abc1.txt")) == 0, "equal filenames with numbers should be equal");
    assert!(c(comparers::compare_file_extensions_unicode, Some("abc1.txt"), Some("abc2.txt")) < 0, "filenames with numbers should be in numerical order, not alphabetical order");
    assert!(c(comparers::compare_file_extensions_unicode, Some("txt.abc1"), Some("txt.abc1")) == 0, "equal extensions with numbers should be equal");
    assert!(c(comparers::compare_file_extensions_unicode, Some("txt.abc1"), Some("txt.abc2")) < 0, "extensions with numbers should be in numerical order, not alphabetical order");
    assert!(c(comparers::compare_file_extensions_unicode, Some("a.ext1"), Some("b.ext1")) < 0, "if equal extensions with numbers, full filenames should be compared");
    //
    // Comparisons with different results than compareFileExtensionsDefault
    //

    // name-only comparisons
    assert!(c(comparers::compare_file_extensions_unicode, Some("Z"), Some("a")) < 0, "Z comes before a");
    assert!(c(comparers::compare_file_extensions_unicode, Some("a"), Some("A")) > 0, "the same letter sorts uppercase first");
    assert!(c(comparers::compare_file_extensions_unicode, Some("â"), Some("Â")) > 0, "the same accented letter sorts uppercase first");
    assert_eq!(sorted(comparers::compare_file_extensions_unicode, &["artichoke", "Artichoke", "art", "Art"]), vec!["Art", "Artichoke", "art", "artichoke"], "names with the same root and different cases sort uppercase names first");
    assert_eq!(sorted(comparers::compare_file_extensions_unicode, &["email", "Email", "émail", "Émail"]), vec!["Email", "email", "Émail", "émail"], "the same base characters with different case or accents sort in unicode order");
    // name plus extension comparisons
    assert!(c(comparers::compare_file_extensions_unicode, Some("a.MD"), Some("a.md")) < 0, "case differences in extensions sort by uppercase extension first");
    assert!(c(comparers::compare_file_extensions_unicode, Some("a.md"), Some("A.md")) > 0, "case differences in names sort uppercase first");
    assert!(c(comparers::compare_file_extensions_unicode, Some("art01"), Some("Art01")) > 0, "a numerically equivalent name of a different case sorts uppercase first");
    assert_eq!(sorted(comparers::compare_file_extensions_unicode, &["a10.txt", "A2.txt", "A100.txt", "a20.txt"]), vec!["A100.txt", "A2.txt", "a10.txt", "a20.txt"], "filenames with number and case differences sort in unicode order");
    assert!(c(comparers::compare_file_extensions_unicode, Some("aggregate.go"), Some("aggregate_repo.go")) < 0, "when extensions are equal, compares full filenames in unicode order");
    // numeric comparisons
    assert!(c(comparers::compare_file_extensions_unicode, Some("abc2.txt"), Some("abc10.txt")) > 0, "filenames with numbers should be in unicode order");
    assert!(c(comparers::compare_file_extensions_unicode, Some("abc02.txt"), Some("abc010.txt")) > 0, "filenames with numbers that have leading zeros sort in unicode order");
    assert!(c(comparers::compare_file_extensions_unicode, Some("abc1.10.txt"), Some("abc1.2.txt")) < 0, "numbers with dots between them sort in unicode order");
    assert!(c(comparers::compare_file_extensions_unicode, Some("abc2.txt2"), Some("abc1.txt10")) > 0, "extensions with numbers should be in unicode order");
    assert!(c(comparers::compare_file_extensions_unicode, Some("txt.abc2"), Some("txt.abc10")) > 0, "extensions with numbers should be in unicode order even when they are multiple digits long");
    assert!(c(comparers::compare_file_extensions_unicode, Some("abc.txt01"), Some("abc.txt1")) < 0, "extensions with equal numbers should be in unicode order");
    assert!(c(comparers::compare_file_extensions_unicode, Some("abc02.txt"), Some("abc002.txt")) > 0, "filenames with equivalent numbers and leading zeros sort in unicode order");
    assert!(c(comparers::compare_file_extensions_unicode, Some("txt.abc01"), Some("txt.abc1")) < 0, "extensions with equivalent numbers sort in unicode order");
    assert!(c(comparers::compare_file_extensions_unicode, Some("a.ext1"), Some("b.Ext1")) < 0, "if extensions with numbers are equal except for case, unicode full filenames should be compared");
    assert!(c(comparers::compare_file_extensions_unicode, Some("a.ext1"), Some("a.Ext1")) > 0, "if extensions with numbers are equal except for case, unicode full filenames should be compared");
}
