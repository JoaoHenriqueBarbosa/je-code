//! Porte de `src/vs/base/test/common/naturalLanguage/korean.test.ts` (suíte `Korean`), caso a caso e na mesma ordem.
//!
//! Os casos ficam na subsuíte `getKoreanAltChars` do original. As jamo conjuntivas (U+1100 a U+11FF), que
//! não se leem bem no editor, saem como escapes `\u{...}`.

mod common;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_base::natural_language::korean::get_korean_alt_chars;

fn get_korean_alt_chars_for_string(text: &str) -> String {
    let mut result: Vec<u16> = Vec::new();
    for unit in text.encode_utf16() {
        match get_korean_alt_chars(u32::from(unit)) {
            Some(chars) => result.extend(chars),
            None => result.push(unit),
        }
    }
    String::from_utf16_lossy(&result)
}

#[test]
fn modern_initial_consonants() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();

    let cases: [(&str, &str); 19] = [
        ("\u{1100}", "r"),
        ("\u{1101}", "R"),
        ("\u{1102}", "s"),
        ("\u{1103}", "e"),
        ("\u{1104}", "E"),
        ("\u{1105}", "f"),
        ("\u{1106}", "a"),
        ("\u{1107}", "q"),
        ("\u{1108}", "Q"),
        ("\u{1109}", "t"),
        ("\u{110a}", "T"),
        ("\u{110b}", "d"),
        ("\u{110c}", "w"),
        ("\u{110d}", "W"),
        ("\u{110e}", "c"),
        ("\u{110f}", "z"),
        ("\u{1110}", "x"),
        ("\u{1111}", "v"),
        ("\u{1112}", "g"),
    ];
    for (hangul, alt) in cases {
        assert_eq!(
            get_korean_alt_chars_for_string(hangul),
            alt,
            "\"{hangul}\" (0x{:x}) should result in \"{alt}\"",
            hangul.encode_utf16().next().unwrap()
        );
    }
}

#[test]
fn modern_latter_consonants() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();

    let cases: [(&str, &str); 27] = [
        ("\u{11a8}", "r"),
        ("\u{11a9}", "R"),
        ("\u{11aa}", "rt"),
        ("\u{11ab}", "s"),
        ("\u{11ac}", "sw"),
        ("\u{11ad}", "sg"),
        ("\u{11ae}", "e"),
        ("\u{11af}", "f"),
        ("\u{11b0}", "fr"),
        ("\u{11b1}", "fa"),
        ("\u{11b2}", "fq"),
        ("\u{11b3}", "ft"),
        ("\u{11b4}", "fx"),
        ("\u{11b5}", "fv"),
        ("\u{11b6}", "fg"),
        ("\u{11b7}", "a"),
        ("\u{11b8}", "q"),
        ("\u{11b9}", "qt"),
        ("\u{11ba}", "t"),
        ("\u{11bb}", "T"),
        ("\u{11bc}", "d"),
        ("\u{11bd}", "w"),
        ("\u{11be}", "c"),
        ("\u{11bf}", "z"),
        ("\u{11c0}", "x"),
        ("\u{11c1}", "v"),
        ("\u{11c2}", "g"),
    ];
    for (hangul, alt) in cases {
        assert_eq!(
            get_korean_alt_chars_for_string(hangul),
            alt,
            "\"{hangul}\" (0x{:x}) should result in \"{alt}\"",
            hangul.encode_utf16().next().unwrap()
        );
    }
}

#[test]
fn modern_vowels() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();

    let cases: [(&str, &str); 21] = [
        ("\u{1161}", "k"),
        ("\u{1162}", "o"),
        ("\u{1163}", "i"),
        ("\u{1164}", "O"),
        ("\u{1165}", "j"),
        ("\u{1166}", "p"),
        ("\u{1167}", "u"),
        ("\u{1168}", "P"),
        ("\u{1169}", "h"),
        ("\u{116a}", "hk"),
        ("\u{116b}", "ho"),
        ("\u{116c}", "hl"),
        ("\u{116d}", "y"),
        ("\u{116e}", "n"),
        ("\u{116f}", "nj"),
        ("\u{1170}", "np"),
        ("\u{1171}", "nl"),
        ("\u{1172}", "b"),
        ("\u{1173}", "m"),
        ("\u{1174}", "ml"),
        ("\u{1175}", "l"),
    ];
    for (hangul, alt) in cases {
        assert_eq!(
            get_korean_alt_chars_for_string(hangul),
            alt,
            "\"{hangul}\" (0x{:x}) should result in \"{alt}\"",
            hangul.encode_utf16().next().unwrap()
        );
    }
}

#[test]
fn compatibility_jamo() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();

    let cases: [(&str, &str); 51] = [
        ("ㄱ", "r"),
        ("ㄲ", "R"),
        ("ㄳ", "rt"),
        ("ㄴ", "s"),
        ("ㄵ", "sw"),
        ("ㄶ", "sg"),
        ("ㄷ", "e"),
        ("ㄸ", "E"),
        ("ㄹ", "f"),
        ("ㄺ", "fr"),
        ("ㄻ", "fa"),
        ("ㄼ", "fq"),
        ("ㄽ", "ft"),
        ("ㄾ", "fx"),
        ("ㄿ", "fv"),
        ("ㅀ", "fg"),
        ("ㅁ", "a"),
        ("ㅂ", "q"),
        ("ㅃ", "Q"),
        ("ㅄ", "qt"),
        ("ㅅ", "t"),
        ("ㅆ", "T"),
        ("ㅇ", "d"),
        ("ㅈ", "w"),
        ("ㅉ", "W"),
        ("ㅊ", "c"),
        ("ㅋ", "z"),
        ("ㅌ", "x"),
        ("ㅍ", "v"),
        ("ㅎ", "g"),
        ("ㅏ", "k"),
        ("ㅐ", "o"),
        ("ㅑ", "i"),
        ("ㅒ", "O"),
        ("ㅓ", "j"),
        ("ㅔ", "p"),
        ("ㅕ", "u"),
        ("ㅖ", "P"),
        ("ㅗ", "h"),
        ("ㅘ", "hk"),
        ("ㅙ", "ho"),
        ("ㅚ", "hl"),
        ("ㅛ", "y"),
        ("ㅜ", "n"),
        ("ㅝ", "nj"),
        ("ㅞ", "np"),
        ("ㅟ", "nl"),
        ("ㅠ", "b"),
        ("ㅡ", "m"),
        ("ㅢ", "ml"),
        ("ㅣ", "l"),
    ];
    for (hangul, alt) in cases {
        assert_eq!(
            get_korean_alt_chars_for_string(hangul),
            alt,
            "\"{hangul}\" (0x{:x}) should result in \"{alt}\"",
            hangul.encode_utf16().next().unwrap()
        );
    }
}

#[test]
fn composed_samples() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();

    let cases: [(&str, &str); 82] = [
        ("ㅁㅊㅊㄷㄴ냐ㅠㅑㅣㅑ쇼", "accessibility"),
        ("ㅁㅊ채ㅕㅜㅅ뚜샤시드둣ㄴ", "accountEntitlements"),
        ("며야ㅐ쳗ㄴ", "audioCues"),
        ("ㅠㄱㅁ찯셰먁채ㅣㅐ걐ㄷㄱ2ㅆ디듣ㅅ교", "bracketPairColorizer2Telemetry"),
        ("ㅠㅕㅣㅏㄸ얏", "bulkEdit"),
        ("ㅊ미ㅣㅗㅑㄷㄱㅁㄱ초ㅛ", "callHierarchy"),
        ("촘ㅅ", "chat"),
        ("챙ㄷㅁㅊ샤ㅐㅜㄴ", "codeActions"),
        ("챙ㄷㄸ야색", "codeEditor"),
        ("채ㅡㅡ뭉ㄴ", "commands"),
        ("채ㅡㅡ둣ㄴ", "comments"),
        ("채ㅜ럏ㄸ테ㅐㄳㄷㄱ", "configExporter"),
        ("채ㅜㅅㄷㅌ스두ㅕ", "contextmenu"),
        ("쳔새ㅡㄸ야색", "customEditor"),
        ("ㅇ듀ㅕㅎ", "debug"),
        ("ㅇ덱ㄷㅊㅁㅅㄷㅇㄸㅌㅅ두냐ㅐㅜㅡㅑㅎㄱㅁ색", "deprecatedExtensionMigrator"),
        ("ㄷ얏ㄴㄷㄴ냐ㅐㅜㄴ", "editSessions"),
        ("드ㅡㄷㅅ", "emmet"),
        ("ㄷㅌㅅ두냐ㅐㅜㄴ", "extensions"),
        ("ㄷㅌㅅㄷ구밌ㄷ그ㅑㅜ미", "externalTerminal"),
        ("ㄷㅌㅅㄷ구미ㅕ갸ㅒㅔ둗ㄱ", "externalUriOpener"),
        ("랴ㅣㄷㄴ", "files"),
        ("래ㅣ야ㅜㅎ", "folding"),
        ("래금ㅅ", "format"),
        ("ㅑㅟ묘ㅗㅑㅜㅅㄴ", "inlayHints"),
        ("ㅑㅟㅑㅜㄷ촘ㅅ", "inlineChat"),
        ("ㅑㅜㅅㄷㄱㅁㅊ샾ㄷ", "interactive"),
        ("ㅑㄴ녇", "issue"),
        ("ㅏ됴ㅠㅑㅜ야ㅜㅎㄴ", "keybindings"),
        ("ㅣ무혐ㅎㄷㅇㄷㅅㄷㅊ샤ㅐㅜ", "languageDetection"),
        ("ㅣ무혐ㅎㄷㄴㅅㅁ션", "languageStatus"),
        ("ㅣㅑㅡㅑ샤ㅜ얓ㅁ색", "limitIndicator"),
        ("ㅣㅑㄴㅅ", "list"),
        ("ㅣㅐㅊ미ㅗㅑㄴ새교", "localHistory"),
        ("ㅣㅐㅊ미ㅑㅋㅁ샤ㅐㅜ", "localization"),
        ("ㅣㅐㅎㄴ", "logs"),
        ("ㅡ메ㅔㄷㅇㄸ얏ㄴ", "mappedEdits"),
        ("ㅡㅁ가애주", "markdown"),
        ("ㅡㅁ갇ㄱㄴ", "markers"),
        ("ㅡㄷㄱㅎㄷㄸ야색", "mergeEditor"),
        ("ㅡㅕㅣ샤얄ㄹㄸ야색", "multiDiffEditor"),
        ("ㅜㅐㅅ듀ㅐㅐㅏ", "notebook"),
        ("ㅐㅕ시ㅑㅜㄷ", "outline"),
        ("ㅐㅕ세ㅕㅅ", "output"),
        ("ㅔㄷㄱ래그뭋ㄷ", "performance"),
        ("ㅔㄱㄷㄹㄷㄱ둧ㄷㄴ", "preferences"),
        ("벼ㅑ참ㅊㅊㄷㄴㄴ", "quickaccess"),
        ("ㄱ디며ㅜ촏ㄱ", "relauncher"),
        ("ㄱ드ㅐㅅㄷ", "remote"),
        ("ㄱ드ㅐㅅㄷ쎠ㅜㅜ디", "remoteTunnel"),
        ("ㄴㅁ노", "sash"),
        ("ㄴ츠", "scm"),
        ("ㄴㄷㅁㄱ초", "search"),
        ("ㄴㄷㅁㄱ초ㄸ야색", "searchEditor"),
        ("놈ㄱㄷ", "share"),
        ("누ㅑㅔㅔㄷㅅㄴ", "snippets"),
        ("넫ㄷ초", "speech"),
        ("네ㅣㅁ노", "splash"),
        ("녁ㅍ됸", "surveys"),
        ("ㅅㅁㅎㄴ", "tags"),
        ("ㅅㅁ난", "tasks"),
        ("ㅅ디듣ㅅ교", "telemetry"),
        ("ㅅㄷ그ㅑㅜ미", "terminal"),
        ("ㅅㄷ그ㅑㅜ미채ㅜㅅ갸ㅠ", "terminalContrib"),
        ("ㅅㄷㄴ샤ㅜㅎ", "testing"),
        ("소듣ㄴ", "themes"),
        ("샤ㅡ디ㅑㅜㄷ", "timeline"),
        ("쇼ㅔ도ㅑㄷㄱㅁㄱ초ㅛ", "typeHierarchy"),
        ("ㅕㅔㅇㅁㅅㄷ", "update"),
        ("ㅕ기", "url"),
        ("ㅕㄴㄷㄱㅇㅁㅅ몌개랴ㅣㄷ", "userDataProfile"),
        ("ㅕㄴㄷㄱㅇㅁㅅㅁ뇨ㅜㅊ", "userDataSync"),
        ("ㅈ듀퍋ㅈ", "webview"),
        ("ㅈ듀퍋졔무디", "webviewPanel"),
        ("ㅈ듀퍋ㅈ퍋ㅈ", "webviewView"),
        ("ㅈ디채ㅡ듀무ㅜㄷㄱ", "welcomeBanner"),
        ("ㅈ디채ㅡㄷ야미ㅐㅎ", "welcomeDialog"),
        ("ㅈ디채ㅡㄷㅎㄷㅅ샤ㅜㅎㄴㅅㅁㄳㄷㅇ", "welcomeGettingStarted"),
        ("ㅈ디채ㅡㄷ퍋ㅈㄴ", "welcomeViews"),
        ("ㅈ디채ㅡㄷㅉ미ㅏ소개ㅕ호", "welcomeWalkthrough"),
        ("재가넴ㅊㄷ", "workspace"),
        ("재가넴ㅊㄷㄴ", "workspaces"),
    ];
    for (hangul, alt) in cases {
        // Compara em minúsculas, pois alguns casos não têm entrada em hangul correspondente
        assert_eq!(
            get_korean_alt_chars_for_string(hangul).to_lowercase(),
            alt.to_lowercase(),
            "\"{hangul}\" (0x{:x}) should result in \"{alt}\"",
            hangul.encode_utf16().next().unwrap()
        );
    }
}
