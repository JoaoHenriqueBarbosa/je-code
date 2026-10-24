//! Casos de unidade do `jschardet` portado. Os valores esperados saem dos exemplos do `README.md` do pacote
//! e do próprio `jschardet` 3.1.4 (npm) rodado sobre as mesmas entradas.

use jschardet::codingstatemachine::SmModel;
use jschardet::{DetectOptions, Detected, Error, detect, detect_all, try_detect, try_detect_all};

const ISO_8859_5: &str = "ISO-8859-5";

fn options(detect_encodings: &[&str]) -> DetectOptions {
    DetectOptions { detect_encodings: Some(detect_encodings.iter().map(|encoding| (*encoding).to_string()).collect()), min_confidence: None }
}

fn assert_detected(actual: Option<Detected>, encoding: &str, confidence: f64) {
    let actual = actual.unwrap_or_else(|| panic!("esperava {encoding}, veio nada"));
    assert_eq!(actual.encoding, encoding);
    assert!((actual.confidence - confidence).abs() < 1e-12, "confiança de {encoding}: {} no lugar de {confidence}", actual.confidence);
}

fn assert_all(actual: &[Detected], expected: &[(&str, f64)]) {
    let summary: Vec<(String, f64)> = actual.iter().map(|detected| (detected.encoding.clone(), detected.confidence)).collect();
    assert_eq!(summary.len(), expected.len(), "{summary:?}");
    for ((encoding, confidence), (expected_encoding, expected_confidence)) in summary.iter().zip(expected) {
        assert_eq!(encoding, expected_encoding, "{summary:?}");
        assert!((confidence - expected_confidence).abs() < 1e-12, "{summary:?}");
    }
}

// README: "àíàçã" in UTF-8
#[test]
fn readme_utf8_example() {
    assert_detected(detect(b"\xc3\xa0\xc3\xad\xc3\xa0\xc3\xa7\xc3\xa3", None), "UTF-8", 0.99);
}

// README: "次常用國字標準字體表" in Big5
#[test]
fn readme_big5_example() {
    let bytes = b"\xa6\xb8\xb1\x60\xa5\xce\xb0\xea\xa6\x72\xbc\xd0\xb7\xc7\xa6\x72\xc5\xe9\xaa\xed";
    assert_detected(detect(bytes, None), "Big5", 0.99);
}

// README: Martin Kühl, detectAll (o README é de uma versão anterior e não tem o windows-1250 do ajuste do 3.x)
#[test]
fn readme_detect_all_example() {
    let bytes = b"<string>Martin K\xfchl</string>";
    assert_all(
        &detect_all(bytes, None),
        &[("windows-1252", 0.95), ("ISO-8859-2", 0.8796300205763055), ("windows-1250", 0.825), ("SHIFT_JIS", 0.01)],
    );
}

// O caso do `read_stream_autoguess_encoding_candidate_guess_encodings` do VS Code: bytes Shift_JIS com os
// candidatos utf-8, shiftjis e euc-jp (já em nomes do `jschardet`, como o `toJschardetEncoding` os devolve).
#[test]
fn shift_jis_with_candidates() {
    let bytes = [82, 177, 82, 241, 82, 201, 82, 191, 82, 205];
    assert_detected(detect(&bytes, Some(&options(&["UTF-8", "SHIFT_JIS", "EUC-JP"]))), "SHIFT_JIS", 0.01);
    assert_eq!(detect(&bytes, None), None);
    assert_all(&detect_all(&bytes, None), &[("SHIFT_JIS", 0.01), ("windows-1251", 0.01), ("windows-1252", 0.0)]);
}

#[test]
fn euc_jp_with_and_without_candidates() {
    let bytes = [0xa4, 0xb3, 0xa4, 0xf3, 0xa4, 0xcb, 0xa4, 0xc1, 0xa4, 0xcf];
    assert_detected(detect(&bytes, None), "EUC-JP", 0.99);
    assert_detected(detect(&bytes, Some(&options(&["UTF-8", "SHIFT_JIS", "EUC-JP"]))), "EUC-JP", 0.99);
}

#[test]
fn ascii_and_empty() {
    assert_detected(detect(b"hello world", None), "ascii", 1.0);
    assert_all(&detect_all(b"hello world", None), &[("ascii", 1.0)]);
    assert_eq!(detect(b"", None), None);
    assert_eq!(detect_all(b"", None), vec![]);
    assert_eq!(detect(&[0x1b], None), None);
    assert_eq!(detect(b"~{ plain }", None), None);
}

#[test]
fn byte_order_marks() {
    assert_detected(detect(&[0xef, 0xbb, 0xbf, 0x41], None), "UTF-8", 1.0);
    assert_detected(detect(&[0xff, 0xfe, 0x41, 0x00], None), "UTF-16LE", 1.0);
    assert_detected(detect(&[0xfe, 0xff, 0x00, 0x41], None), "UTF-16BE", 1.0);
    assert_detected(detect(&[0xff, 0xfe], None), "UTF-16LE", 1.0);
    assert_detected(detect(&[0xff, 0xfe, 0, 0, 0x41, 0, 0, 0], None), "UTF-32LE", 1.0);
    assert_detected(detect(&[0, 0, 0xfe, 0xff, 0, 0, 0, 0x41], None), "UTF-32BE", 1.0);
    assert_detected(detect(&[0xfe, 0xff, 0, 0, 1, 2], None), "X-ISO-10646-UCS-4-3412", 1.0);
    assert_detected(detect(&[0, 0, 0xff, 0xfe, 1, 2], None), "X-ISO-10646-UCS-4-2143", 1.0);
}

#[test]
fn byte_order_mark_outside_the_candidates_falls_back_to_the_probers() {
    assert_detected(detect(&[0xef, 0xbb, 0xbf, 0x41], Some(&options(&["windows-1252"]))), "windows-1252", 0.95);
}

#[test]
fn escape_sequences() {
    assert_detected(detect(&[0x1b, 0x24, 0x42, 0x24, 0x33, 0x24, 0x73, 0x1b, 0x28, 0x42], None), "ISO-2022-JP", 0.99);
    assert_detected(detect(&[0x1b, 0x24, 0x29, 0x43, 0x0e, 0x30, 0x21, 0x0f], None), "ISO-2022-KR", 0.99);
    assert_detected(detect(b"~{<:Ky2;S{#,NpJ)l6HK!#~}", None), "HZ-GB-2312", 0.99);
    assert_eq!(detect(&[0x1b, 0x24, 0x42, 0x24, 0x33, 0x24, 0x73, 0x1b, 0x28, 0x42], Some(&options(&["UTF-8"]))), None);
}

#[test]
fn multibyte_and_single_byte_samples() {
    assert_detected(detect(b"caf\xc3\xa9 au lait \xc3\xa0 la cr\xc3\xa8me", None), "UTF-8", 0.99);
    assert_eq!(detect(b"caf\xc3", None), None);
    assert_detected(detect(&[0xbe, 0xc8, 0xb3, 0xe7, 0xc7, 0xcf, 0xbc, 0xbc, 0xbf, 0xe4], None), "EUC-KR", 0.6666666666666666);
    assert_eq!(detect(&[0xc4, 0xe3, 0xba, 0xc3, 0xca, 0xc0, 0xbd, 0xe7], None), None);
    assert_detected(detect(&[0xf0, 0xd2, 0xc9, 0xd7, 0xc5, 0xd4, 0x20, 0xcd, 0xc9, 0xd2], None), "KOI8-R", 0.99);
    let win1251 = [0xcf, 0xf0, 0xe8, 0xe2, 0xe5, 0xf2, 0x20, 0xec, 0xe8, 0xf0, 0x20, 0xef, 0xf0, 0xe8, 0xe2, 0xe5, 0xf2, 0x20, 0xe4, 0xf0, 0xf3, 0xe3];
    assert_detected(detect(&win1251, None), "windows-1251", 0.99);
    assert_detected(detect(&[0xa1, 0xd2, 0xc3, 0xa1, 0xd5, 0xb5, 0xcb, 0xd5, 0xe8, 0xb4, 0xd5, 0xa1, 0xd5, 0xb4], None), "TIS-620", 0.6642853145283018);
    assert_detected(detect(&[0xc3, 0xe5, 0xe9, 0xdc, 0x20, 0xf3, 0xef, 0xf5, 0x20, 0xea, 0xfc, 0xf3, 0xec, 0xe5], None), "ISO-8859-7", 0.9043984173479895);
}

#[test]
fn hebrew_logical_and_visual() {
    let word = [0xf9, 0xec, 0xe5, 0xed, 0x20, 0xf2, 0xe5, 0xec, 0xed, 0x20];
    let logical: Vec<u8> = word.repeat(8);
    let mut reversed = word[..9].to_vec();
    reversed.reverse();
    reversed.push(0x20);
    let visual: Vec<u8> = reversed.repeat(8);
    assert_detected(detect(&logical, None), "windows-1255", 0.99);
    assert_detected(detect(&visual, None), "ISO-8859-8", 0.99);
    assert_all(&detect_all(&visual, None), &[("ISO-8859-8", 0.99), ("EUC-JP", 0.01), ("windows-1252", 0.0)]);
}

#[test]
fn minimum_threshold() {
    let bytes = b"caf\xe9 au lait \xe0 la cr\xe8me";
    assert_detected(detect(bytes, None), "windows-1252", 0.95);
    let strict = DetectOptions { detect_encodings: None, min_confidence: Some(0.99) };
    assert_eq!(detect(bytes, Some(&strict)), None);
    // `detectAll` devolve a lista ordenada mesmo quando nenhuma passa do limite.
    assert_all(&detect_all(bytes, Some(&strict)), &[("windows-1252", 0.95), ("windows-1250", 0.825), ("ISO-8859-2", 0.6567904153636414)]);
    // Com `detectEncodings` o limite padrão cai para zero.
    assert_detected(detect(b"caf\xe9 au lait \xe0 la cr\xe8me", Some(&options(&["windows-1252"]))), "windows-1252", 0.95);
    assert_detected(detect(&[0xe9, 0xe8, 0xe0], Some(&DetectOptions { detect_encodings: None, min_confidence: Some(0.0) })), "windows-1255", 0.99);
}

#[test]
fn detect_encodings_filter() {
    assert_detected(detect(b"caf\xe9 au lait", Some(&options(&["UTF-8", "windows-1252"]))), "windows-1252", 0.95);
    // A validação tira só os hífens e passa tudo para minúsculas; a comparação de `canDetectEncoding` não tira
    // os hífens, então "WINDOWS1252" é aceito pela validação e nunca casa.
    assert_eq!(detect(b"caf\xe9 au lait", Some(&options(&["utf8", "WINDOWS1252"]))), None);
    assert_eq!(detect(b"caf\xe9 au lait", Some(&options(&[]))), None);
    assert_eq!(detect(b"abc", Some(&options(&["windows-1252"]))), None);
}

#[test]
fn unsupported_encodings_are_errors() {
    let expected_message = "Encoding foo is not supported. Supported encodings: UTF-8,UTF-32LE,UTF-32BE,UTF-32BE,UTF-16LE,UTF-16BE,X-ISO-10646-UCS-4-3412,X-ISO-10646-UCS-4-2143,HZ-GB-2312,ISO-2022-CN,ISO-2022-JP,ISO-2022-KR,UTF-8,SHIFT_JIS,EUC-JP,GB2312,EUC-KR,Big5,EUC-TW,windows-1251,KOI8-R,ISO-8859-5,x-mac-cyrillic,IBM866,IBM855,ISO-8859-7,windows-1253,ISO-8859-5,windows-1251,ISO-8859-2,windows-1250,TIS-620,windows-1255,windows-1255,windows-1255,windows-1252.";
    let error = try_detect(&[0xe9], Some(&options(&["foo"]))).unwrap_err();
    assert!(matches!(&error, Error::UnsupportedEncoding { encoding, .. } if encoding == "foo"));
    assert_eq!(error.to_string(), expected_message);
    assert_eq!(detect(&[0xe9], Some(&options(&["foo"]))), None);
    assert_eq!(detect_all(&[0xe9], Some(&options(&["foo"]))), vec![]);
    // "ascii" e "shiftjis" não estão na lista do original: só "SHIFT_JIS" e companhia.
    assert!(try_detect(b"abc", Some(&options(&["ascii"]))).is_err());
    assert!(try_detect_all(&[82, 177], Some(&options(&["shiftjis"]))).is_err());
}

// O `TypeError` do original quando a ordem de um hiragana Shift_JIS passa da tabela `jp2CharContext`.
#[test]
fn shift_jis_hiragana_out_of_the_context_table() {
    let bytes = [0x82, 0xb1, 0x82, 0xf1, 0x82, 0xc9, 0x82, 0xbf, 0x82, 0xcd, 0x82, 0xf2, 0x82, 0xf2];
    assert!(matches!(try_detect(&bytes, None), Err(Error::HiraganaOrderOutOfRange(_))));
    assert_eq!(detect(&bytes, None), None);
}

#[test]
fn state_machine_models_are_consistent() {
    use jschardet::escsm::{HZ_SM_MODEL, ISO2022CN_SM_MODEL, ISO2022JP_SM_MODEL, ISO2022KR_SM_MODEL};
    use jschardet::mbcssm::{big5::BIG5_SM_MODEL, eucjp::EUCJP_SM_MODEL, euckr::EUCKR_SM_MODEL, euctw::EUCTW_SM_MODEL, gb2312::GB2312_SM_MODEL};
    use jschardet::mbcssm::{sjis::SJIS_SM_MODEL, ucs2be::UCS2BE_SM_MODEL, ucs2le::UCS2LE_SM_MODEL, utf8::UTF8_SM_MODEL};

    let models: [&SmModel; 13] = [
        &HZ_SM_MODEL,
        &ISO2022CN_SM_MODEL,
        &ISO2022JP_SM_MODEL,
        &ISO2022KR_SM_MODEL,
        &BIG5_SM_MODEL,
        &EUCJP_SM_MODEL,
        &EUCKR_SM_MODEL,
        &EUCTW_SM_MODEL,
        &GB2312_SM_MODEL,
        &SJIS_SM_MODEL,
        &UCS2BE_SM_MODEL,
        &UCS2LE_SM_MODEL,
        &UTF8_SM_MODEL,
    ];
    for model in models {
        assert_eq!(model.class_table.len(), 256, "{}", model.name);
        assert_eq!(model.char_len_table.len(), model.class_factor, "{}", model.name);
        assert!(model.class_table.iter().all(|&class| usize::from(class) < model.class_factor), "{}", model.name);
        // Os estados `error` (1) e `itsMe` (2) são terminais; qualquer outro índice de estado tem a sua linha
        // inteira dentro da tabela (a tabela não é múltipla de `class_factor` em várias codificações).
        assert!(
            model.state_table.iter().all(|&state| matches!(state, 1 | 2) || usize::from(state) * model.class_factor + model.class_factor <= model.state_table.len()),
            "{}",
            model.name
        );
    }
}

#[test]
fn language_models_are_consistent() {
    use jschardet::langbulgarianmodel::{LATIN5_BULGARIAN_MODEL, WIN1251_BULGARIAN_MODEL};
    use jschardet::langcyrillicmodel::{IBM855_MODEL, IBM866_MODEL, KOI8R_MODEL, LATIN5_CYRILLIC_MODEL, MAC_CYRILLIC_MODEL, WIN1251_CYRILLIC_MODEL};
    use jschardet::langgreekmodel::{LATIN7_GREEK_MODEL, WIN1253_GREEK_MODEL};
    use jschardet::langhebrewmodel::WIN1255_HEBREW_MODEL;
    use jschardet::langhungarianmodel::{LATIN2_HUNGARIAN_MODEL, WIN1250_HUNGARIAN_MODEL};
    use jschardet::langthaimodel::TIS620_THAI_MODEL;
    use jschardet::sbcharsetprober::SbModel;

    let models: [&SbModel; 14] = [
        &KOI8R_MODEL,
        &WIN1251_CYRILLIC_MODEL,
        &LATIN5_CYRILLIC_MODEL,
        &MAC_CYRILLIC_MODEL,
        &IBM866_MODEL,
        &IBM855_MODEL,
        &LATIN7_GREEK_MODEL,
        &WIN1253_GREEK_MODEL,
        &LATIN5_BULGARIAN_MODEL,
        &WIN1251_BULGARIAN_MODEL,
        &LATIN2_HUNGARIAN_MODEL,
        &WIN1250_HUNGARIAN_MODEL,
        &TIS620_THAI_MODEL,
        &WIN1255_HEBREW_MODEL,
    ];
    for model in models {
        assert_eq!(model.char_to_order_map.len(), 256, "{}", model.charset_name);
        assert_eq!(model.precedence_matrix.len(), 64 * 64, "{}", model.charset_name);
        assert!(model.precedence_matrix.iter().all(|&category| category <= 3), "{}", model.charset_name);
    }
    assert_eq!(models.iter().filter(|model| model.charset_name == ISO_8859_5).count(), 2);
}

#[test]
fn frequency_tables_have_the_documented_sizes() {
    use jschardet::{big5freq, euckrfreq, euctwfreq, gb2312freq, jisfreq};
    assert_eq!(big5freq::BIG5_CHAR_TO_FREQ_ORDER.len(), big5freq::BIG5_TABLE_SIZE);
    assert_eq!(euckrfreq::EUCKR_CHAR_TO_FREQ_ORDER.len(), euckrfreq::EUCKR_TABLE_SIZE);
    assert_eq!(gb2312freq::GB2312_CHAR_TO_FREQ_ORDER.len(), gb2312freq::GB2312_TABLE_SIZE);
    assert_eq!(jisfreq::JIS_CHAR_TO_FREQ_ORDER.len(), jisfreq::JIS_TABLE_SIZE);
    // O array do original é menor que a constante de tamanho (6 016 contra 8 102).
    assert_eq!(euctwfreq::EUCTW_CHAR_TO_FREQ_ORDER.len(), 6016);
    assert_eq!(euctwfreq::EUCTW_TABLE_SIZE, 8102);
}
