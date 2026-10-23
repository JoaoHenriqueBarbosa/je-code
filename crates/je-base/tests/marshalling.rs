//! Porte de `src/vs/base/test/common/marshalling.test.ts` (suíte `Marshalling`), caso a caso e na mesma ordem.
//!
//! O `ensureNoDisposablesAreLeakedInTestSuite` não entra, porque `marshalling` não cria `IDisposable`.
//! O `/foo/img` do original é `RegExpValue::new("foo", "gim")`, e o `URI.file` usa a plataforma POSIX (a que
//! o original executa no Linux).

use je_base::js_string::JsString;
use je_base::marshalling::{Marshalled, RegExpValue, parse, stringify};
use je_base::path::Platform;
use je_base::uri::{UriComponents, URI};

const POSIX: Platform = Platform::Posix;

fn clone_of(value: &Marshalled) -> Marshalled {
    let raw = stringify(value);
    parse(raw.units(), POSIX)
}

#[test]
fn reg_exp() {
    let regexp = RegExpValue::new(&JsString::from("foo"), &JsString::from("img"));
    let value = Marshalled::RegExp(regexp.into());
    let Marshalled::RegExp(clone) = clone_of(&value) else { panic!("esperava um RegExp") };
    let Marshalled::RegExp(original) = &value else { unreachable!() };

    assert_eq!(original.source, clone.source);
    assert_eq!(original.regexp.flags().global, clone.regexp.flags().global);
    assert_eq!(original.regexp.flags().ignore_case, clone.regexp.flags().ignore_case);
    assert_eq!(original.regexp.flags().multiline, clone.regexp.flags().multiline);
    assert!(clone.regexp.flags().global && clone.regexp.flags().ignore_case && clone.regexp.flags().multiline);
}

#[test]
fn uri() {
    let component = |text: &str| Some(JsString::from(text));
    let value = URI::from(
        &UriComponents {
            scheme: component("file"),
            authority: component("server"),
            path: component("/shares/c#files"),
            query: component("q"),
            fragment: component("f"),
        },
        false,
    );
    let Marshalled::Uri(clone) = clone_of(&Marshalled::Uri(value.clone())) else { panic!("esperava um URI") };

    assert_eq!(value.scheme(), clone.scheme());
    assert_eq!(value.authority(), clone.authority());
    assert_eq!(value.path(), clone.path());
    assert_eq!(value.query(), clone.query());
    assert_eq!(value.fragment(), clone.fragment());
}

#[test]
fn bug_16793_hash_in_folder_name_mirror_models_get_out_of_sync() {
    let uri1 = URI::file("C:\\C#\\file.txt", POSIX);
    let Marshalled::Uri(clone) = clone_of(&Marshalled::Uri(uri1.clone())) else { panic!("esperava um URI") };
    assert_eq!(clone.to_string(false), uri1.to_string(false));
}
