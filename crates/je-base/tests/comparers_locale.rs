//! A locale entra no `comparers` por `collator::set_default_locale`, e muda a ordem como no `Intl.Collator`
//! do Node (valores conferidos com `new Intl.Collator(locale).compare`). Fica num binário de teste só
//! porque a locale padrão é estado do processo.

use je_base::collator::{Collator, CollatorOptions, default_locale, set_default_locale};
use je_base::comparers;

fn u(text: &str) -> Vec<u16> {
    text.encode_utf16().collect()
}

#[test]
fn default_locale_starts_in_english_and_changes_the_order() {
    assert_eq!(default_locale(), "en");
    // Em `en` o `ä` é um `a` acentuado e vem antes do `z`.
    assert!(comparers::compare_file_names(Some(&u("ä")), Some(&u("z"))) < 0);

    // Em `sv` o `ä` é uma letra depois do `z` (Node: `new Intl.Collator('sv').compare('ä', 'z') === 1`).
    assert!(set_default_locale("sv"));
    assert_eq!(default_locale(), "sv");
    assert!(comparers::compare_file_names(Some(&u("ä")), Some(&u("z"))) > 0);
    assert!(comparers::compare_file_names_default(Some(&u("ä")), Some(&u("z"))) > 0);

    // Uma tag inválida é recusada e a locale não muda.
    assert!(!set_default_locale("x_y"));
    assert_eq!(default_locale(), "sv");

    // Um colador com a tag explícita ignora a padrão, e a tag inválida cai na padrão (`safeIntl.Collator`).
    let en = Collator::new(Some("en"), CollatorOptions::default());
    assert_eq!(en.compare(&u("ä"), &u("z")), -1);
    let invalid = Collator::new(Some("x_y"), CollatorOptions::default());
    assert_eq!(invalid.locale(), "sv");
    assert_eq!(invalid.compare(&u("ä"), &u("z")), 1);

    // `da` põe a maiúscula primeiro por padrão (`resolvedOptions().caseFirst === 'upper'` no V8).
    let da = Collator::new(Some("da"), CollatorOptions::default());
    assert_eq!(da.compare(&u("a"), &u("A")), 1);
}
