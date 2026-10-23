//! `Intl.Collator`: a colação CLDR (UCA) do ICU, no `icu_collator` do ICU4X.
//!
//! É a mesma crate, na mesma versão (a 2.3.1 vendorizada em `vendor/icu_collator`, ver o `JE-CODE.md`
//! de lá), que o `Intl.Collator` do `zjsc` do pseudo-linus usa, e as opções são mapeadas como o V8 as
//! mapeia para o ICU4C (`JSCollator::New`):
//!
//! | `Intl.Collator`                   | ICU                                     | aqui                           |
//! | --------------------------------- | --------------------------------------- | ------------------------------ |
//! | `sensitivity: 'base'`             | `UCOL_STRENGTH = PRIMARY`               | [`Strength::Primary`]          |
//! | `sensitivity: 'accent'`           | `UCOL_STRENGTH = SECONDARY`             | [`Strength::Secondary`]        |
//! | `sensitivity: 'case'`             | `PRIMARY` mais `UCOL_CASE_LEVEL = ON`   | `Primary` e [`CaseLevel::On`]  |
//! | `sensitivity: 'variant'` (padrão) | `UCOL_STRENGTH = TERTIARY`              | [`Strength::Tertiary`]         |
//! | `numeric: true`                   | `kn` (`UCOL_NUMERIC_COLLATION = ON`)    | [`CollationNumericOrdering`]   |
//! | `caseFirst`                       | `kf` (`UCOL_CASE_FIRST`)                | [`CollationCaseFirst`]         |
//! | `ignorePunctuation: true`         | `UCOL_ALTERNATE_HANDLING = SHIFTED`     | [`AlternateHandling::Shifted`] |
//!
//! ## De onde vem a locale
//!
//! O `new Intl.Collator(undefined, ...)` do `comparers.ts` usa a locale padrão do hospedeiro, que no VS
//! Electron é a do idioma de exibição. Aqui a locale padrão é [`default_locale`], que começa em `en` (a do
//! VS Code em inglês) e é trocada por [`set_default_locale`], que a configuração e o `nls` chamam ao ler o
//! idioma (por exemplo com `Platform::locale`). Quem passa a tag explicitamente em [`Collator::new`]
//! ignora a padrão. Como no `safeIntl.Collator` do VS Code, uma tag que não é um identificador de locale
//! válido cai na padrão. Uma locale bem formada, mas sem dados (`xx`), cai na colação raiz do CLDR,
//! que para o `en` é a mesma ordem.
//!
//! O resultado de [`Collator::compare`] é `-1`, `0` ou `1`, como o de `Intl.Collator.prototype.compare`,
//! e texto é `&[u16]` porque o ICU compara unidades UTF-16 (inclusive surrogates isolados, que têm peso
//! implícito do próprio code unit, como no ICU4C).

use std::cmp::Ordering;
use std::sync::RwLock;

use icu_collator::options::{AlternateHandling, CaseLevel, CollatorOptions as IcuOptions, Strength};
use icu_collator::preferences::{CollationCaseFirst, CollationNumericOrdering};
use icu_collator::{Collator as IcuCollator, CollatorBorrowed, CollatorPreferences};
use icu_locale_core::Locale;

/// `sensitivity` do `Intl.Collator`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Sensitivity {
    /// Só as letras base: `a` igual a `á` e a `A`.
    Base,
    /// Letras e acentos: `a` diferente de `á`, igual a `A`.
    Accent,
    /// Letras e caixa: `a` igual a `á`, diferente de `A`.
    Case,
    /// Tudo (o padrão).
    #[default]
    Variant,
}

/// `caseFirst` do `Intl.Collator`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CaseFirst {
    Upper,
    Lower,
    False,
}

/// As opções do `Intl.Collator` que o VS Code e o porte usam (`usage`, `collation` e `localeMatcher` não
/// mudam a ordem de `sort` em `en`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CollatorOptions {
    pub numeric: bool,
    pub sensitivity: Sensitivity,
    pub ignore_punctuation: bool,
    /// `None` deixa o padrão da locale (`false` na maioria, `upper` em `da`, por exemplo), que é o que o V8
    /// faz quando a opção não é passada.
    pub case_first: Option<CaseFirst>,
}

static DEFAULT_LOCALE: RwLock<String> = RwLock::new(String::new());

/// A locale padrão, `en` até alguém chamar [`set_default_locale`].
pub fn default_locale() -> String {
    let current = DEFAULT_LOCALE.read().unwrap_or_else(|poisoned| poisoned.into_inner());
    if current.is_empty() { "en".to_owned() } else { current.clone() }
}

/// Troca a locale padrão dos colatores criados depois (os já criados não mudam, como o `Intl.Collator`
/// guarda a locale que resolveu na construção). Uma tag inválida é ignorada e devolve `false`.
pub fn set_default_locale(tag: &str) -> bool {
    let Ok(locale) = tag.parse::<Locale>() else { return false };
    *DEFAULT_LOCALE.write().unwrap_or_else(|poisoned| poisoned.into_inner()) = locale.to_string();
    true
}

/// Um `Intl.Collator`.
pub struct Collator {
    icu: CollatorBorrowed<'static>,
    options: CollatorOptions,
    locale: String,
}

impl Collator {
    /// `new Intl.Collator(locale, options)` pelo caminho do `safeIntl.Collator`: sem locale, ou com uma
    /// inválida, vale [`default_locale`].
    pub fn new(locale: Option<&str>, options: CollatorOptions) -> Collator {
        let requested = locale.and_then(|tag| tag.parse::<Locale>().ok()).unwrap_or_else(|| {
            default_locale().parse().expect("a locale padrão foi validada em set_default_locale")
        });
        let mut preferences = CollatorPreferences::from(&requested);
        preferences.case_first = options.case_first.map(|case_first| match case_first {
            CaseFirst::Upper => CollationCaseFirst::Upper,
            CaseFirst::Lower => CollationCaseFirst::Lower,
            CaseFirst::False => CollationCaseFirst::False,
        });
        preferences.numeric_ordering = Some(if options.numeric {
            CollationNumericOrdering::True
        } else {
            CollationNumericOrdering::False
        });
        let (strength, case_level) = match options.sensitivity {
            Sensitivity::Base => (Strength::Primary, CaseLevel::Off),
            Sensitivity::Accent => (Strength::Secondary, CaseLevel::Off),
            Sensitivity::Case => (Strength::Primary, CaseLevel::On),
            Sensitivity::Variant => (Strength::Tertiary, CaseLevel::Off),
        };
        let mut icu_options = IcuOptions::default();
        icu_options.strength = Some(strength);
        icu_options.case_level = Some(case_level);
        icu_options.alternate_handling = options.ignore_punctuation.then_some(AlternateHandling::Shifted);
        let icu = IcuCollator::try_new(preferences, icu_options)
            .or_else(|_| IcuCollator::try_new(CollatorPreferences::default(), icu_options))
            .expect("a colação raiz está nos dados compilados");
        Collator { icu, options, locale: requested.to_string() }
    }

    /// `collator.compare(a, b)`: `-1`, `0` ou `1`.
    pub fn compare(&self, a: &[u16], b: &[u16]) -> i32 {
        match self.icu.compare_utf16(a, b) {
            Ordering::Less => -1,
            Ordering::Equal => 0,
            Ordering::Greater => 1,
        }
    }

    /// `resolvedOptions().numeric`.
    pub fn is_numeric(&self) -> bool {
        self.options.numeric
    }

    /// A locale com que o colador foi criado.
    pub fn locale(&self) -> &str {
        &self.locale
    }
}

thread_local! {
    static DEFAULT_COLLATOR: Collator = Collator::new(None, CollatorOptions::default());
}

/// `a.localeCompare(b)`: a comparação do `String.prototype.localeCompare` sem locale nem opções.
pub fn locale_compare(a: &str, b: &str) -> Ordering {
    let (a, b): (Vec<u16>, Vec<u16>) = (a.encode_utf16().collect(), b.encode_utf16().collect());
    DEFAULT_COLLATOR.with(|collator| collator.compare(&a, &b).cmp(&0))
}
