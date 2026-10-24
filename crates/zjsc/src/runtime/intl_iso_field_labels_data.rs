//! Rótulos de `month` e `day` do calendário `iso8601` com era, por locale, e os meses abreviados que diferem do gregoriano (medidos no bun).
//!
//! GERADO por `scripts/gen-iso-field-labels.js`: não edite à mão. Para regenerar, da raiz da crate:
//! `bun scripts/gen-iso-field-labels.js > src/runtime/intl_iso_field_labels_data.rs`.

/// (locale, rótulo do mês, rótulo do dia), na ordem de `LOCALES` do gerador de `intl_date_time_data`.
static LABELS: [(&str, &str, &str); 65] = [
    ("en", "month", "day"),
    ("pt", "m\u{ea}s", "dia"),
    ("es", "mes", "d\u{ed}a"),
    ("fr", "mois", "jour"),
    ("de", "Monat", "Tag"),
    ("it", "mese", "giorno"),
    ("ja", "\u{6708}", "\u{65e5}"),
    ("ru", "\u{43c}\u{435}\u{441}\u{44f}\u{446}", "\u{434}\u{435}\u{43d}\u{44c}"),
    ("ar", "\u{627}\u{644}\u{634}\u{647}\u{631}", "\u{64a}\u{648}\u{645}"),
    ("zh", "\u{6708}", "\u{65e5}"),
    ("ko", "\u{c6d4}", "\u{c77c}"),
    ("nl", "maand", "dag"),
    ("hi", "\u{92e}\u{93e}\u{939}", "\u{926}\u{93f}\u{928}"),
    ("th", "\u{e40}\u{e14}\u{e37}\u{e2d}\u{e19}", "\u{e27}\u{e31}\u{e19}"),
    ("tr", "ay", "g\u{fc}n"),
    ("pl", "miesi\u{105}c", "dzie\u{144}"),
    ("sv", "m\u{e5}nad", "dag"),
    ("da", "m\u{e5}ned", "dag"),
    ("nb", "m\u{e5}ned", "dag"),
    ("fi", "kuukausi", "p\u{e4}iv\u{e4}"),
    ("cs", "m\u{11b}s\u{ed}c", "den"),
    ("el", "\u{3bc}\u{3ae}\u{3bd}\u{3b1}\u{3c2}", "\u{3b7}\u{3bc}\u{3ad}\u{3c1}\u{3b1}"),
    ("he", "\u{5d7}\u{5d5}\u{5d3}\u{5e9}", "\u{5d9}\u{5d5}\u{5dd}"),
    ("id", "bulan", "hari"),
    ("vi", "Th\u{e1}ng", "Ng\u{e0}y"),
    ("uk", "\u{43c}\u{456}\u{441}\u{44f}\u{446}\u{44c}", "\u{434}\u{435}\u{43d}\u{44c}"),
    ("en-GB", "month", "day"),
    ("en-AU", "month", "day"),
    ("en-CA", "month", "day"),
    ("en-IN", "month", "day"),
    ("pt-PT", "m\u{ea}s", "dia"),
    ("es-MX", "mes", "d\u{ed}a"),
    ("es-AR", "mes", "d\u{ed}a"),
    ("fr-CA", "mois", "jour"),
    ("de-AT", "Monat", "Tag"),
    ("de-CH", "Monat", "Tag"),
    ("zh-TW", "\u{6708}", "\u{65e5}"),
    ("zh-HK", "\u{6708}", "\u{65e5}"),
    ("hu", "h\u{f3}nap", "nap"),
    ("fa", "\u{645}\u{627}\u{647}", "\u{631}\u{648}\u{632}"),
    ("am", "\u{12c8}\u{122d}", "\u{1240}\u{1295}"),
    ("my", "\u{101c}", "\u{101b}\u{1000}\u{103a}"),
    ("km", "\u{1781}\u{17c2}", "\u{1790}\u{17d2}\u{1784}\u{17c3}"),
    ("lo", "\u{ec0}\u{e94}\u{eb7}\u{ead}\u{e99}", "\u{ea1}\u{eb7}\u{ec9}"),
    ("mn", "\u{441}\u{430}\u{440}", "\u{4e9}\u{434}\u{4e9}\u{440}"),
    ("ps", "\u{645}\u{64a}\u{627}\u{634}\u{62a}", "\u{648}\u{631}\u{681}"),
    ("sd", "\u{645}\u{647}\u{64a}\u{646}\u{648}", "\u{68f}\u{64a}\u{646}\u{647}\u{646}"),
    ("so", "Bil", "maalin"),
    ("fil", "buwan", "araw"),
    ("ha", "wata", "kwana"),
    ("yo", "Os\u{f9}", "\u{1ecc}j\u{1ecd}\u{301}"),
    ("zu", "Inyanga", "Usuku"),
    ("xh", "inyanga", "usuku"),
    ("cy", "mis", "diwrnod"),
    ("gd", "m\u{ec}os", "latha"),
    ("lb", "Mount", "Dag"),
    ("mt", "xahar", "jum"),
    ("fo", "m\u{e1}na\u{f0}ur", "dagur"),
    ("ky", "\u{430}\u{439}", "\u{43a}\u{4af}\u{43d}"),
    ("tg", "\u{43c}\u{43e}\u{4b3}", "\u{440}\u{4ef}\u{437}"),
    ("tk", "a\u{fd}", "g\u{fc}n"),
    ("tt", "\u{430}\u{439}", "\u{43a}\u{4e9}\u{43d}"),
    ("ku", "meh", "roj"),
    ("or", "\u{b2e}\u{b3e}\u{b38}", "\u{b26}\u{b3f}\u{b28}"),
    ("as", "\u{9ae}\u{9be}\u{9b9}", "\u{9a6}\u{9bf}\u{9a8}"),
];

fn find(key: &str) -> Option<(&'static str, &'static str)> {
    LABELS.iter().find(|(name, _, _)| *name == key).map(|(_, month, day)| (*month, *day))
}

/// Os meses abreviados do `iso8601` dos locales em que diferem do gregoriano isolado.
static SHORT_MONTHS: [(&str, [&str; 12]); 1] = [
    ("el", ["\u{399}\u{3b1}\u{3bd}", "\u{3a6}\u{3b5}\u{3b2}", "\u{39c}\u{3ac}\u{3c1}", "\u{391}\u{3c0}\u{3c1}", "\u{39c}\u{3ac}\u{3b9}", "\u{399}\u{3bf}\u{3cd}\u{3bd}", "\u{399}\u{3bf}\u{3cd}\u{3bb}", "\u{391}\u{3cd}\u{3b3}", "\u{3a3}\u{3b5}\u{3c0}", "\u{39f}\u{3ba}\u{3c4}", "\u{39d}\u{3bf}\u{3ad}", "\u{394}\u{3b5}\u{3ba}"]),
];

/// O mês abreviado (`month0` de 0 a 11) do `iso8601` quando o locale difere do gregoriano isolado.
pub fn short_month_override(tag: &str, month0: usize) -> Option<&'static str> {
    let language = tag.split('-').next().unwrap_or("");
    SHORT_MONTHS.iter().find(|(name, _)| *name == language).map(|(_, months)| months[month0])
}

/// O rótulo do mês e o do dia do tag BCP 47: primeiro `língua-REGIÃO`, depois a língua sozinha, por fim o inglês.
pub fn labels(tag: &str) -> (&'static str, &'static str) {
    let mut subtags = tag.split('-');
    let language = subtags.next().unwrap_or("");
    let region = subtags.take_while(|subtag| subtag.len() > 1).find(|subtag| subtag.len() == 2 && subtag.bytes().all(|byte| byte.is_ascii_alphabetic()));
    region
        .and_then(|region| find(&format!("{language}-{}", region.to_ascii_uppercase())))
        .or_else(|| find(language))
        .unwrap_or(("month", "day"))
}
