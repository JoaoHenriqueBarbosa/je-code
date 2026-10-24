//! Os dados do `Intl.DisplayNames` sem ICU: nomes de língua, região, escrita, moeda, calendário e campo de
//! data e hora em inglês e em português do Brasil (o que o `ULocaleDisplayNames`, o `ucurr_getName` e o
//! `udatpg_getFieldDisplayName` leem do CLDR).
//!
//! ORIGEM: as tabelas foram escritas de memória do CLDR 4x (as mesmas do ICU 7x do bun); não têm golden.
//! Cobrem as línguas, regiões, escritas e moedas de uso comum, não o CLDR inteiro: um código sem nome
//! aqui responde como o ICU responde a um código sem dados (o `fallback` do chamador decide).
//!
//! O estilo `short` só difere do `long` onde o CLDR tem a variante `alt="short"` (`GB` vira `UK`, `US`
//! vira `US`, `en-US` vira `US English`); `narrow` é tratado como `short`, como o `UDISPCTX_LENGTH_SHORT`.

use crate::runtime::intl_locale_data::Language;

/// Escolhe o texto da língua: (inglês, português).
fn pick(language: Language, english: &'static str, portuguese: &'static str) -> &'static str {
    match language {
        Language::English => english,
        Language::Portuguese => portuguese,
    }
}

/// (código, inglês, português).
type Row = (&'static str, &'static str, &'static str);

fn find(table: &[Row], code: &str, language: Language) -> Option<&'static str> {
    table.iter().find(|row| row.0 == code).map(|row| pick(language, row.1, row.2))
}

// ---------------------------------------------------------------------------------------------
// Línguas
// ---------------------------------------------------------------------------------------------

const LANGUAGES: &[Row] = &[
    ("af", "Afrikaans", "africâner"),
    ("am", "Amharic", "amárico"),
    ("ar", "Arabic", "árabe"),
    ("az", "Azerbaijani", "azerbaijano"),
    ("be", "Belarusian", "bielorrusso"),
    ("bg", "Bulgarian", "búlgaro"),
    ("bn", "Bangla", "bengali"),
    ("bs", "Bosnian", "bósnio"),
    ("ca", "Catalan", "catalão"),
    ("cs", "Czech", "tcheco"),
    ("cy", "Welsh", "galês"),
    ("da", "Danish", "dinamarquês"),
    ("de", "German", "alemão"),
    ("el", "Greek", "grego"),
    ("en", "English", "inglês"),
    ("eo", "Esperanto", "esperanto"),
    ("es", "Spanish", "espanhol"),
    ("et", "Estonian", "estoniano"),
    ("eu", "Basque", "basco"),
    ("fa", "Persian", "persa"),
    ("fi", "Finnish", "finlandês"),
    ("fil", "Filipino", "filipino"),
    ("fr", "French", "francês"),
    ("ga", "Irish", "irlandês"),
    ("gl", "Galician", "galego"),
    ("gu", "Gujarati", "guzerate"),
    ("he", "Hebrew", "hebraico"),
    ("hi", "Hindi", "híndi"),
    ("hr", "Croatian", "croata"),
    ("hu", "Hungarian", "húngaro"),
    ("hy", "Armenian", "armênio"),
    ("id", "Indonesian", "indonésio"),
    ("is", "Icelandic", "islandês"),
    ("it", "Italian", "italiano"),
    ("ja", "Japanese", "japonês"),
    ("ka", "Georgian", "georgiano"),
    ("kk", "Kazakh", "cazaque"),
    ("km", "Khmer", "khmer"),
    ("kn", "Kannada", "canarês"),
    ("ko", "Korean", "coreano"),
    ("la", "Latin", "latim"),
    ("lt", "Lithuanian", "lituano"),
    ("lv", "Latvian", "letão"),
    ("mk", "Macedonian", "macedônio"),
    ("ml", "Malayalam", "malaiala"),
    ("mn", "Mongolian", "mongol"),
    ("mr", "Marathi", "marati"),
    ("ms", "Malay", "malaio"),
    ("mt", "Maltese", "maltês"),
    ("my", "Burmese", "birmanês"),
    ("nb", "Norwegian Bokmål", "bokmål norueguês"),
    ("ne", "Nepali", "nepalês"),
    ("nl", "Dutch", "holandês"),
    ("no", "Norwegian", "norueguês"),
    ("pa", "Punjabi", "panjabi"),
    ("pl", "Polish", "polonês"),
    ("pt", "Portuguese", "português"),
    ("ro", "Romanian", "romeno"),
    ("ru", "Russian", "russo"),
    ("sk", "Slovak", "eslovaco"),
    ("sl", "Slovenian", "esloveno"),
    ("sq", "Albanian", "albanês"),
    ("sr", "Serbian", "sérvio"),
    ("sv", "Swedish", "sueco"),
    ("sw", "Swahili", "suaíli"),
    ("ta", "Tamil", "tâmil"),
    ("te", "Telugu", "télugo"),
    ("th", "Thai", "tailandês"),
    ("tl", "Tagalog", "tagalo"),
    ("tr", "Turkish", "turco"),
    ("uk", "Ukrainian", "ucraniano"),
    ("ur", "Urdu", "urdu"),
    ("uz", "Uzbek", "uzbeque"),
    ("vi", "Vietnamese", "vietnamita"),
    ("yi", "Yiddish", "iídiche"),
    ("yue", "Cantonese", "cantonês"),
    ("zh", "Chinese", "chinês"),
    ("zu", "Zulu", "zulu"),
];

/// O nome da língua sozinha (`en`, `pt`).
pub fn language_name(code: &str, language: Language) -> Option<&'static str> {
    find(LANGUAGES, code, language)
}

/// Os nomes de dialeto (`UDISPCTX_DIALECT_NAMES`): a chave é a tag canônica (`en-GB`, `zh-Hans`); o
/// terceiro campo do inglês é o nome curto (`None`: o curto é o longo). O quarto é o português: medido
/// no bun, o `pt` só tem nome de dialeto para estes (`en-US` sai `Inglês (Estados Unidos)`); a
/// capitalização do início é feita por quem compõe o nome.
const DIALECTS: &[(&str, &str, Option<&str>, Option<&str>)] = &[
    ("de-AT", "Austrian German", None, None),
    ("de-CH", "Swiss High German", None, Some("alto alemão (Suíça)")),
    ("en-AU", "Australian English", None, None),
    ("en-CA", "Canadian English", None, None),
    ("en-GB", "British English", Some("UK English"), None),
    ("en-US", "American English", Some("US English"), None),
    ("es-419", "Latin American Spanish", None, None),
    ("es-ES", "European Spanish", None, None),
    ("es-MX", "Mexican Spanish", None, None),
    ("fr-CA", "Canadian French", None, None),
    ("fr-CH", "Swiss French", None, None),
    ("nl-BE", "Flemish", None, Some("flamengo")),
    ("pt-BR", "Brazilian Portuguese", None, None),
    ("pt-PT", "European Portuguese", None, None),
    ("ro-MD", "Moldavian", None, Some("moldávio")),
    ("sw-CD", "Congo Swahili", None, Some("suaíli do Congo")),
    ("zh-Hans", "Simplified Chinese", None, Some("chinês simplificado")),
    ("zh-Hant", "Traditional Chinese", None, Some("chinês tradicional")),
];

/// O nome do dialeto de `key` (`en-GB`), no estilo curto quando `short`.
pub fn dialect_name(key: &str, language: Language, short: bool) -> Option<&'static str> {
    let row = DIALECTS.iter().find(|row| row.0 == key)?;
    match language {
        Language::English => Some(row.2.filter(|_| short).unwrap_or(row.1)),
        Language::Portuguese => row.3,
    }
}

// ---------------------------------------------------------------------------------------------
// Escritas
// ---------------------------------------------------------------------------------------------

/// (código, inglês, português) da escrita sozinha (`Scripts%stand-alone` quando o CLDR tem as duas).
const SCRIPTS: &[Row] = &[
    ("Arab", "Arabic", "árabe"),
    ("Armn", "Armenian", "armênio"),
    ("Beng", "Bangla", "bengali"),
    ("Bopo", "Bopomofo", "bopomofo"),
    ("Brai", "Braille", "braile"),
    ("Cyrl", "Cyrillic", "cirílico"),
    ("Deva", "Devanagari", "devanágari"),
    ("Ethi", "Ethiopic", "etíope"),
    ("Geor", "Georgian", "georgiano"),
    ("Grek", "Greek", "grego"),
    ("Gujr", "Gujarati", "guzerate"),
    ("Guru", "Gurmukhi", "gurmukhi"),
    ("Hang", "Hangul", "hangul"),
    ("Hani", "Han", "han"),
    ("Hans", "Simplified", "simplificado"),
    ("Hant", "Traditional", "tradicional"),
    ("Hebr", "Hebrew", "hebraico"),
    ("Hira", "Hiragana", "hiragana"),
    ("Jpan", "Japanese", "japonês"),
    ("Kana", "Katakana", "katakana"),
    ("Khmr", "Khmer", "khmer"),
    ("Knda", "Kannada", "canarim"),
    ("Kore", "Korean", "coreano"),
    ("Laoo", "Lao", "laosiano"),
    ("Latn", "Latin", "latim"),
    ("Mlym", "Malayalam", "malaiala"),
    ("Mong", "Mongolian", "mongol"),
    ("Mymr", "Myanmar", "birmanês"),
    ("Sinh", "Sinhala", "cingalês"),
    ("Syrc", "Syriac", "siríaco"),
    ("Taml", "Tamil", "tâmil"),
    ("Telu", "Telugu", "télugo"),
    ("Thai", "Thai", "tailandês"),
    ("Tibt", "Tibetan", "tibetano"),
    ("Zsye", "Emoji", "emoji"),
    ("Zsym", "Symbols", "símbolos"),
    ("Zyyy", "Common", "comum"),
    ("Zzzz", "Unknown Script", "escrita desconhecida"),
];

/// O nome da escrita sozinha (`uldn_scriptDisplayName`).
pub fn script_name(code: &str, language: Language) -> Option<&'static str> {
    find(SCRIPTS, code, language)
}

// ---------------------------------------------------------------------------------------------
// Regiões
// ---------------------------------------------------------------------------------------------

const REGIONS: &[Row] = &[
    ("001", "world", "Mundo"),
    ("019", "Americas", "Américas"),
    ("150", "Europe", "Europa"),
    ("419", "Latin America", "América Latina"),
    ("AD", "Andorra", "Andorra"),
    ("AE", "United Arab Emirates", "Emirados Árabes Unidos"),
    ("AF", "Afghanistan", "Afeganistão"),
    ("AG", "Antigua & Barbuda", "Antígua e Barbuda"),
    ("AL", "Albania", "Albânia"),
    ("AM", "Armenia", "Armênia"),
    ("AO", "Angola", "Angola"),
    ("AR", "Argentina", "Argentina"),
    ("AT", "Austria", "Áustria"),
    ("AU", "Australia", "Austrália"),
    ("AZ", "Azerbaijan", "Azerbaijão"),
    ("BA", "Bosnia & Herzegovina", "Bósnia e Herzegovina"),
    ("BB", "Barbados", "Barbados"),
    ("BD", "Bangladesh", "Bangladesh"),
    ("BE", "Belgium", "Bélgica"),
    ("BF", "Burkina Faso", "Burquina Faso"),
    ("BG", "Bulgaria", "Bulgária"),
    ("BH", "Bahrain", "Bahrein"),
    ("BI", "Burundi", "Burundi"),
    ("BJ", "Benin", "Benin"),
    ("BO", "Bolivia", "Bolívia"),
    ("BR", "Brazil", "Brasil"),
    ("BS", "Bahamas", "Bahamas"),
    ("BT", "Bhutan", "Butão"),
    ("BW", "Botswana", "Botsuana"),
    ("BY", "Belarus", "Belarus"),
    ("BZ", "Belize", "Belize"),
    ("CA", "Canada", "Canadá"),
    ("CD", "Congo - Kinshasa", "Congo - Kinshasa"),
    ("CG", "Congo - Brazzaville", "Congo - Brazzaville"),
    ("CH", "Switzerland", "Suíça"),
    ("CI", "Côte d’Ivoire", "Costa do Marfim"),
    ("CL", "Chile", "Chile"),
    ("CM", "Cameroon", "Camarões"),
    ("CN", "China", "China"),
    ("CO", "Colombia", "Colômbia"),
    ("CR", "Costa Rica", "Costa Rica"),
    ("CU", "Cuba", "Cuba"),
    ("CV", "Cape Verde", "Cabo Verde"),
    ("CY", "Cyprus", "Chipre"),
    ("CZ", "Czechia", "Tchéquia"),
    ("DE", "Germany", "Alemanha"),
    ("DJ", "Djibouti", "Djibuti"),
    ("DK", "Denmark", "Dinamarca"),
    ("DO", "Dominican Republic", "República Dominicana"),
    ("DZ", "Algeria", "Argélia"),
    ("EC", "Ecuador", "Equador"),
    ("EE", "Estonia", "Estônia"),
    ("EG", "Egypt", "Egito"),
    ("ER", "Eritrea", "Eritreia"),
    ("ES", "Spain", "Espanha"),
    ("ET", "Ethiopia", "Etiópia"),
    ("EU", "European Union", "União Europeia"),
    ("FI", "Finland", "Finlândia"),
    ("FJ", "Fiji", "Fiji"),
    ("FR", "France", "França"),
    ("GA", "Gabon", "Gabão"),
    ("GB", "United Kingdom", "Reino Unido"),
    ("GE", "Georgia", "Geórgia"),
    ("GH", "Ghana", "Gana"),
    ("GL", "Greenland", "Groenlândia"),
    ("GM", "Gambia", "Gâmbia"),
    ("GN", "Guinea", "Guiné"),
    ("GQ", "Equatorial Guinea", "Guiné Equatorial"),
    ("GR", "Greece", "Grécia"),
    ("GT", "Guatemala", "Guatemala"),
    ("GW", "Guinea-Bissau", "Guiné-Bissau"),
    ("GY", "Guyana", "Guiana"),
    ("HK", "Hong Kong SAR China", "Hong Kong, RAE da China"),
    ("HN", "Honduras", "Honduras"),
    ("HR", "Croatia", "Croácia"),
    ("HT", "Haiti", "Haiti"),
    ("HU", "Hungary", "Hungria"),
    ("ID", "Indonesia", "Indonésia"),
    ("IE", "Ireland", "Irlanda"),
    ("IL", "Israel", "Israel"),
    ("IN", "India", "Índia"),
    ("IQ", "Iraq", "Iraque"),
    ("IR", "Iran", "Irã"),
    ("IS", "Iceland", "Islândia"),
    ("IT", "Italy", "Itália"),
    ("JM", "Jamaica", "Jamaica"),
    ("JO", "Jordan", "Jordânia"),
    ("JP", "Japan", "Japão"),
    ("KE", "Kenya", "Quênia"),
    ("KG", "Kyrgyzstan", "Quirguistão"),
    ("KH", "Cambodia", "Camboja"),
    ("KP", "North Korea", "Coreia do Norte"),
    ("KR", "South Korea", "Coreia do Sul"),
    ("KW", "Kuwait", "Kuwait"),
    ("KZ", "Kazakhstan", "Cazaquistão"),
    ("LA", "Laos", "Laos"),
    ("LB", "Lebanon", "Líbano"),
    ("LK", "Sri Lanka", "Sri Lanka"),
    ("LR", "Liberia", "Libéria"),
    ("LS", "Lesotho", "Lesoto"),
    ("LT", "Lithuania", "Lituânia"),
    ("LU", "Luxembourg", "Luxemburgo"),
    ("LV", "Latvia", "Letônia"),
    ("LY", "Libya", "Líbia"),
    ("MA", "Morocco", "Marrocos"),
    ("MC", "Monaco", "Mônaco"),
    ("MD", "Moldova", "Moldávia"),
    ("ME", "Montenegro", "Montenegro"),
    ("MG", "Madagascar", "Madagascar"),
    ("MK", "North Macedonia", "Macedônia do Norte"),
    ("ML", "Mali", "Mali"),
    ("MM", "Myanmar (Burma)", "Mianmar (Birmânia)"),
    ("MN", "Mongolia", "Mongólia"),
    ("MO", "Macao SAR China", "Macau, RAE da China"),
    ("MR", "Mauritania", "Mauritânia"),
    ("MT", "Malta", "Malta"),
    ("MU", "Mauritius", "Maurício"),
    ("MV", "Maldives", "Maldivas"),
    ("MW", "Malawi", "Maláui"),
    ("MX", "Mexico", "México"),
    ("MY", "Malaysia", "Malásia"),
    ("MZ", "Mozambique", "Moçambique"),
    ("NA", "Namibia", "Namíbia"),
    ("NE", "Niger", "Níger"),
    ("NG", "Nigeria", "Nigéria"),
    ("NI", "Nicaragua", "Nicarágua"),
    ("NL", "Netherlands", "Países Baixos"),
    ("NO", "Norway", "Noruega"),
    ("NP", "Nepal", "Nepal"),
    ("NZ", "New Zealand", "Nova Zelândia"),
    ("OM", "Oman", "Omã"),
    ("PA", "Panama", "Panamá"),
    ("PE", "Peru", "Peru"),
    ("PG", "Papua New Guinea", "Papua-Nova Guiné"),
    ("PH", "Philippines", "Filipinas"),
    ("PK", "Pakistan", "Paquistão"),
    ("PL", "Poland", "Polônia"),
    ("PR", "Puerto Rico", "Porto Rico"),
    ("PS", "Palestinian Territories", "Territórios palestinos"),
    ("PT", "Portugal", "Portugal"),
    ("PY", "Paraguay", "Paraguai"),
    ("QA", "Qatar", "Catar"),
    ("RO", "Romania", "Romênia"),
    ("RS", "Serbia", "Sérvia"),
    ("RU", "Russia", "Rússia"),
    ("RW", "Rwanda", "Ruanda"),
    ("SA", "Saudi Arabia", "Arábia Saudita"),
    ("SD", "Sudan", "Sudão"),
    ("SE", "Sweden", "Suécia"),
    ("SG", "Singapore", "Singapura"),
    ("SI", "Slovenia", "Eslovênia"),
    ("SK", "Slovakia", "Eslováquia"),
    ("SL", "Sierra Leone", "Serra Leoa"),
    ("SN", "Senegal", "Senegal"),
    ("SO", "Somalia", "Somália"),
    ("SR", "Suriname", "Suriname"),
    ("SS", "South Sudan", "Sudão do Sul"),
    ("ST", "São Tomé & Príncipe", "São Tomé e Príncipe"),
    ("SV", "El Salvador", "El Salvador"),
    ("SY", "Syria", "Síria"),
    ("SZ", "Eswatini", "Essuatíni"),
    ("TD", "Chad", "Chade"),
    ("TG", "Togo", "Togo"),
    ("TH", "Thailand", "Tailândia"),
    ("TJ", "Tajikistan", "Tajiquistão"),
    ("TL", "Timor-Leste", "Timor-Leste"),
    ("TM", "Turkmenistan", "Turcomenistão"),
    ("TN", "Tunisia", "Tunísia"),
    ("TR", "Türkiye", "Turquia"),
    ("TT", "Trinidad & Tobago", "Trinidad e Tobago"),
    ("TW", "Taiwan", "Taiwan"),
    ("TZ", "Tanzania", "Tanzânia"),
    ("UA", "Ukraine", "Ucrânia"),
    ("UK", "United Kingdom", "Reino Unido"),
    ("UG", "Uganda", "Uganda"),
    ("UN", "United Nations", "Nações Unidas"),
    ("US", "United States", "Estados Unidos"),
    ("UY", "Uruguay", "Uruguai"),
    ("UZ", "Uzbekistan", "Uzbequistão"),
    ("VA", "Vatican City", "Cidade do Vaticano"),
    ("VE", "Venezuela", "Venezuela"),
    ("VN", "Vietnam", "Vietnã"),
    ("XK", "Kosovo", "Kosovo"),
    ("YE", "Yemen", "Iêmen"),
    ("ZA", "South Africa", "África do Sul"),
    ("ZM", "Zambia", "Zâmbia"),
    ("ZW", "Zimbabwe", "Zimbábue"),
    ("ZZ", "Unknown Region", "Região desconhecida"),
];

/// As variantes `alt="short"` do CLDR.
const SHORT_REGIONS: &[Row] = &[
    ("GB", "UK", "Reino Unido"),
    ("HK", "Hong Kong", "Hong Kong"),
    ("MO", "Macao", "Macau"),
    ("PS", "Palestine", "Palestina"),
    ("UN", "UN", "ONU"),
    ("US", "US", "EUA"),
];

/// O nome da região (`uldn_regionDisplayName`), no estilo curto quando `short`.
pub fn region_name(code: &str, language: Language, short: bool) -> Option<&'static str> {
    if short {
        if let Some(name) = find(SHORT_REGIONS, code, language) {
            return Some(name);
        }
    }
    find(REGIONS, code, language)
}

// ---------------------------------------------------------------------------------------------
// Moedas
// ---------------------------------------------------------------------------------------------

const CURRENCIES: &[Row] = &[
    ("AED", "United Arab Emirates Dirham", "Dirham dos Emirados Árabes Unidos"),
    ("ARS", "Argentine Peso", "Peso argentino"),
    ("AUD", "Australian Dollar", "Dólar australiano"),
    ("BDT", "Bangladeshi Taka", "Taka bengalesa"),
    ("BGN", "Bulgarian Lev", "Lev búlgaro"),
    ("BRL", "Brazilian Real", "Real brasileiro"),
    ("CAD", "Canadian Dollar", "Dólar canadense"),
    ("CHF", "Swiss Franc", "Franco suíço"),
    ("CLP", "Chilean Peso", "Peso chileno"),
    ("CNY", "Chinese Yuan", "Yuan chinês"),
    ("COP", "Colombian Peso", "Peso colombiano"),
    ("CZK", "Czech Koruna", "Coroa tcheca"),
    ("DKK", "Danish Krone", "Coroa dinamarquesa"),
    ("EGP", "Egyptian Pound", "Libra egípcia"),
    ("EUR", "Euro", "Euro"),
    ("GBP", "British Pound", "Libra esterlina"),
    ("HKD", "Hong Kong Dollar", "Dólar de Hong Kong"),
    ("HUF", "Hungarian Forint", "Florim húngaro"),
    ("IDR", "Indonesian Rupiah", "Rupia indonésia"),
    ("ILS", "Israeli New Shekel", "Novo shekel israelense"),
    ("INR", "Indian Rupee", "Rupia indiana"),
    ("JPY", "Japanese Yen", "Iene japonês"),
    ("KRW", "South Korean Won", "Won sul-coreano"),
    ("MXN", "Mexican Peso", "Peso mexicano"),
    ("MYR", "Malaysian Ringgit", "Ringgit malaio"),
    ("NGN", "Nigerian Naira", "Naira nigeriano"),
    ("NOK", "Norwegian Krone", "Coroa norueguesa"),
    ("NZD", "New Zealand Dollar", "Dólar neozelandês"),
    ("PEN", "Peruvian Sol", "Sol peruano"),
    ("PHP", "Philippine Peso", "Peso filipino"),
    ("PKR", "Pakistani Rupee", "Rupia paquistanesa"),
    ("PLN", "Polish Zloty", "Zloty polonês"),
    ("RON", "Romanian Leu", "Leu romeno"),
    ("RUB", "Russian Ruble", "Rublo russo"),
    ("SAR", "Saudi Riyal", "Rial saudita"),
    ("SEK", "Swedish Krona", "Coroa sueca"),
    ("SGD", "Singapore Dollar", "Dólar singapuriano"),
    ("THB", "Thai Baht", "Baht tailandês"),
    ("TRY", "Turkish Lira", "Lira turca"),
    ("TWD", "New Taiwan Dollar", "Novo dólar taiwanês"),
    ("UAH", "Ukrainian Hryvnia", "Hryvnia ucraniana"),
    ("USD", "US Dollar", "Dólar americano"),
    ("UYU", "Uruguayan Peso", "Peso uruguaio"),
    ("VND", "Vietnamese Dong", "Dong vietnamita"),
    ("XAF", "Central African CFA Franc", "Franco CFA de BEAC"),
    ("XOF", "West African CFA Franc", "Franco CFA de BCEAO"),
    ("XPF", "CFP Franc", "Franco CFP"),
    ("XTS", "Testing Currency Code", "Código de Moeda de Teste"),
    ("XXX", "Unknown Currency", "Moeda desconhecida"),
    ("ZAR", "South African Rand", "Rand sul-africano"),
];

/// Os símbolos estreitos que o `currency_narrow_symbol` do `NumberFormat` não cobre (o CLDR `en`).
const NARROW_SYMBOLS: &[(&str, &str)] = &[
    ("ARS", "$"),
    ("CLP", "$"),
    ("COP", "$"),
    ("CZK", "Kč"),
    ("DKK", "kr"),
    ("EGP", "E£"),
    ("NGN", "₦"),
    ("NOK", "kr"),
    ("PLN", "zł"),
    ("RUB", "₽"),
    ("SEK", "kr"),
    ("SGD", "$"),
    ("THB", "฿"),
    ("TRY", "₺"),
    ("UAH", "₴"),
    ("ZAR", "R"),
];

/// O nome da moeda por extenso (`UCURR_LONG_NAME`); `None` se o código não tem dados.
pub fn currency_long_name(code: &str, language: Language) -> Option<&'static str> {
    find(CURRENCIES, code, language)
}

/// Se o código tem dados de moeda.
pub fn has_currency(code: &str) -> bool {
    CURRENCIES.iter().any(|row| row.0 == code)
}

/// O símbolo estreito de uma moeda que o `NumberFormat` não conhece.
pub fn extra_narrow_symbol(code: &str) -> Option<&'static str> {
    NARROW_SYMBOLS.iter().find(|row| row.0 == code).map(|row| row.1)
}

// ---------------------------------------------------------------------------------------------
// Calendários
// ---------------------------------------------------------------------------------------------

/// Medido no bun; o português vazio é "sem dado no CLDR `pt`" (o ICU responde como a um código desconhecido).
const CALENDARS: &[Row] = &[
    ("buddhist", "Buddhist Calendar", "Calendário Budista"),
    ("chinese", "Chinese Calendar", "Calendário Chinês"),
    ("coptic", "Coptic Calendar", "Calendário Copta"),
    ("dangi", "Dangi Calendar", "Calendário Dangi"),
    ("ethiopic", "Ethiopic Calendar", "Calendário Etíope"),
    ("ethiopic-amete-alem", "Ethiopic Amete Alem Calendar", "Calendário Amete Alem Etíope"),
    ("gregorian", "Gregorian Calendar", "Calendário Gregoriano"),
    ("hebrew", "Hebrew Calendar", "Calendário Hebraico"),
    ("indian", "Indian National Calendar", "Calendário Nacional Indiano"),
    ("islamic", "Hijri Calendar", "Calendário Hegírico"),
    ("islamic-civil", "Hijri Calendar (tabular, civil epoch)", "Calendário Hegírico (tabular, época civil)"),
    ("islamic-rgsa", "Hijri Calendar (Saudi Arabia, sighting)", ""),
    ("islamic-tbla", "Hijri Calendar (tabular, astronomical epoch)", ""),
    ("islamic-umalqura", "Hijri Calendar (Umm al-Qura)", "Calendário Hegírico (Umm al\u{2011}Qura)"),
    ("iso8601", "Gregorian Calendar (ISO 8601 Weeks)", "Calendário ISO-8601"),
    ("japanese", "Japanese Calendar", "Calendário Japonês"),
    ("persian", "Persian Calendar", "Calendário Persa"),
    ("roc", "Minguo Calendar", "Calendário da República da China"),
];

/// O nome do calendário a partir da chave ICU (`gregorian`, não `gregory`).
pub fn calendar_name(icu_key: &str, language: Language) -> Option<&'static str> {
    find(CALENDARS, icu_key, language).filter(|name| !name.is_empty())
}

/// `mapBCP47ToICUCalendarKeyword`: as duas chaves BCP 47 que o ICU escreve de outro jeito.
pub fn bcp47_calendar_to_icu(code: &str) -> &str {
    match code {
        "gregory" => "gregorian",
        "ethioaa" => "ethiopic-amete-alem",
        other => other,
    }
}

// ---------------------------------------------------------------------------------------------
// Campos de data e hora
// ---------------------------------------------------------------------------------------------

/// O estilo de um nome de campo (`UDATPG_WIDE`, `UDATPG_ABBREVIATED`, `UDATPG_NARROW`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FieldWidth {
    Wide,
    Abbreviated,
    Narrow,
}

/// (código, inglês largo, abreviado, estreito, português largo, abreviado, estreito).
type FieldRow = (&'static str, [&'static str; 3], [&'static str; 3]);

const FIELDS: &[FieldRow] = &[
    ("era", ["era", "era", "era"], ["era", "era", "era"]),
    ("year", ["year", "yr.", "yr"], ["ano", "ano", "ano"]),
    ("quarter", ["quarter", "qtr.", "qtr"], ["trimestre", "trim.", "trim."]),
    ("month", ["month", "mo.", "mo"], ["mês", "mês", "mês"]),
    ("weekOfYear", ["week", "wk.", "wk"], ["semana", "sem.", "sem."]),
    ("weekday", ["day of the week", "day of wk.", "day of wk."], ["dia da semana", "dia da sem.", "dia da sem."]),
    ("day", ["day", "day", "day"], ["dia", "dia", "dia"]),
    ("dayPeriod", ["AM/PM", "AM/PM", "AM/PM"], ["AM/PM", "AM/PM", "AM/PM"]),
    ("hour", ["hour", "hr.", "hr"], ["hora", "h", "h"]),
    ("minute", ["minute", "min.", "min"], ["minuto", "min.", "min"]),
    ("second", ["second", "sec.", "sec"], ["segundo", "seg.", "seg."]),
    ("timeZoneName", ["time zone", "zone", "zone"], ["fuso horário", "fuso", "fuso"]),
];

/// O nome do campo (`udatpg_getFieldDisplayName`); `None` se `code` não é um `dateTimeField` válido.
pub fn date_field_name(code: &str, language: Language, width: FieldWidth) -> Option<&'static str> {
    let row = FIELDS.iter().find(|row| row.0 == code)?;
    let names = pick_fields(language, row);
    Some(names[match width {
        FieldWidth::Wide => 0,
        FieldWidth::Abbreviated => 1,
        FieldWidth::Narrow => 2,
    }])
}

fn pick_fields(language: Language, row: &FieldRow) -> [&'static str; 3] {
    match language {
        Language::English => row.1,
        Language::Portuguese => row.2,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tables_have_unique_codes() {
        for table in [LANGUAGES, SCRIPTS, REGIONS, CURRENCIES, CALENDARS] {
            let mut seen: Vec<&str> = table.iter().map(|row| row.0).collect();
            let length = seen.len();
            seen.sort();
            seen.dedup();
            assert_eq!(seen.len(), length);
        }
    }

    #[test]
    fn short_region_falls_back_to_long() {
        assert_eq!(region_name("GB", Language::English, true), Some("UK"));
        assert_eq!(region_name("FR", Language::English, true), Some("France"));
        assert_eq!(region_name("US", Language::Portuguese, true), Some("EUA"));
    }
}
