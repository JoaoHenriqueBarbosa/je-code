//! Porte de `src/vs/base/test/common/normalization.test.ts` (suíte `Normalization`), caso a caso e na mesma ordem.

mod common;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_base::normalization;

fn normalize_to_base(text: &str) -> String {
    let units: Vec<u16> = text.encode_utf16().collect();
    String::from_utf16_lossy(normalization::try_normalize_to_base(&units).units())
}

#[test]
fn try_normalize_to_base() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();

    assert_eq!(normalize_to_base("joào"), "joao");
    assert_eq!(normalize_to_base("joáo"), "joao");
    assert_eq!(normalize_to_base("joâo"), "joao");
    assert_eq!(normalize_to_base("joäo"), "joao");
    // assert.strictEqual(strings.tryNormalizeToBase('joæo'), 'joao'); // not an accent
    assert_eq!(normalize_to_base("joão"), "joao");
    assert_eq!(normalize_to_base("joåo"), "joao");
    assert_eq!(normalize_to_base("joåo"), "joao");
    assert_eq!(normalize_to_base("joāo"), "joao");

    assert_eq!(normalize_to_base("fôo"), "foo");
    assert_eq!(normalize_to_base("föo"), "foo");
    assert_eq!(normalize_to_base("fòo"), "foo");
    assert_eq!(normalize_to_base("fóo"), "foo");
    // assert.strictEqual(strings.tryNormalizeToBase('fœo'), 'foo');
    // assert.strictEqual(strings.tryNormalizeToBase('føo'), 'foo');
    assert_eq!(normalize_to_base("fōo"), "foo");
    assert_eq!(normalize_to_base("fõo"), "foo");

    assert_eq!(normalize_to_base("andrè"), "andre");
    assert_eq!(normalize_to_base("andré"), "andre");
    assert_eq!(normalize_to_base("andrê"), "andre");
    assert_eq!(normalize_to_base("andrë"), "andre");
    assert_eq!(normalize_to_base("andrē"), "andre");
    assert_eq!(normalize_to_base("andrė"), "andre");
    assert_eq!(normalize_to_base("andrę"), "andre");

    assert_eq!(normalize_to_base("hvîc"), "hvic");
    assert_eq!(normalize_to_base("hvïc"), "hvic");
    assert_eq!(normalize_to_base("hvíc"), "hvic");
    assert_eq!(normalize_to_base("hvīc"), "hvic");
    assert_eq!(normalize_to_base("hvįc"), "hvic");
    assert_eq!(normalize_to_base("hvìc"), "hvic");

    assert_eq!(normalize_to_base("ûdo"), "udo");
    assert_eq!(normalize_to_base("üdo"), "udo");
    assert_eq!(normalize_to_base("ùdo"), "udo");
    assert_eq!(normalize_to_base("údo"), "udo");
    assert_eq!(normalize_to_base("ūdo"), "udo");

    assert_eq!(normalize_to_base("heÿ"), "hey");

    // assert.strictEqual(strings.tryNormalizeToBase('gruß'), 'grus');
    assert_eq!(normalize_to_base("gruś"), "grus");
    assert_eq!(normalize_to_base("gruš"), "grus");

    assert_eq!(normalize_to_base("çool"), "cool");
    assert_eq!(normalize_to_base("ćool"), "cool");
    assert_eq!(normalize_to_base("čool"), "cool");

    assert_eq!(normalize_to_base("ñice"), "nice");
    assert_eq!(normalize_to_base("ńice"), "nice");

    // Different cases
    assert_eq!(normalize_to_base("CAFÉ"), "cafe");
    assert_eq!(normalize_to_base("Café"), "cafe");
    assert_eq!(normalize_to_base("café"), "cafe");
    assert_eq!(normalize_to_base("JOÃO"), "joao");
    assert_eq!(normalize_to_base("João"), "joao");

    // Mixed cases with accents
    assert_eq!(normalize_to_base("CaFé"), "cafe");
    assert_eq!(normalize_to_base("JoÃo"), "joao");
    assert_eq!(normalize_to_base("AnDrÉ"), "andre");

    // Precomposed accents
    assert_eq!(normalize_to_base("é"), "e");
    assert_eq!(normalize_to_base("à"), "a");
    assert_eq!(normalize_to_base("café"), "cafe");

    // Base + combining accents - lower only
    assert_eq!(normalize_to_base("é"), "é");
    assert_eq!(normalize_to_base("Ãà"), "ãà");
    assert_eq!(normalize_to_base("CaFé"), "café");

}
