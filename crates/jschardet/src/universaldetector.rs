//! Porte de `src/universaldetector.js`: o detector que alimenta os probers e decide o resultado.
//!
//! This is a port from the python port, version "2.0.1"

use std::sync::OnceLock;

use crate::Error;
use crate::charsetprober::CharsetProber;
use crate::constants::FOUND_IT;
use crate::escprober::EscCharSetProber;
use crate::latin1prober::Latin1Prober;
use crate::mbcsgroupprober::mbcs_group_prober;
use crate::sbcsgroupprober::sbcs_group_prober;

/// `{ encoding, confidence }` do original.
#[derive(Debug, Clone, PartialEq)]
pub struct Detected {
    pub encoding: String,
    pub confidence: f64,
}

/// As opções de `new UniversalDetector(options)`: `detectEncodings` e `minimumThreshold` do original.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct DetectOptions {
    pub detect_encodings: Option<Vec<String>>,
    pub min_confidence: Option<f64>,
}

const BOM_UTF: [&str; 8] = ["UTF-8", "UTF-32LE", "UTF-32BE", "UTF-32BE", "UTF-16LE", "UTF-16BE", "X-ISO-10646-UCS-4-3412", "X-ISO-10646-UCS-4-2143"];

/// `supportedEncodings`: os nomes dos BOMs e os que cada prober suporta, calculados uma vez.
fn supported_encodings() -> &'static [String] {
    static SUPPORTED: OnceLock<Vec<String>> = OnceLock::new();
    SUPPORTED.get_or_init(|| {
        let mut encodings: Vec<String> = BOM_UTF.iter().map(|encoding| (*encoding).to_string()).collect();
        encodings.extend(EscCharSetProber::new().get_supported_charset_names());
        encodings.extend(mbcs_group_prober().get_supported_charset_names());
        encodings.extend(sbcs_group_prober().get_supported_charset_names());
        encodings.extend(Latin1Prober::new().get_supported_charset_names());
        encodings
    })
}

/// `supportedEncodingsDenormalized`: cada nome em minúsculas, com e sem os hífens.
fn supported_encodings_denormalized() -> &'static [String] {
    static DENORMALIZED: OnceLock<Vec<String>> = OnceLock::new();
    DENORMALIZED.get_or_init(|| {
        supported_encodings()
            .iter()
            .flat_map(|encoding| {
                let lower = encoding.to_lowercase();
                let without_hyphens = lower.replace('-', "");
                [lower, without_hyphens]
            })
            .collect()
    })
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum InputState {
    PureAscii,
    EscAscii,
    HighByte,
}

pub struct UniversalDetector {
    detect_encodings: Option<Vec<String>>,
    minimum_threshold: f64,
    result: Option<Detected>,
    results: Vec<Detected>,
    done: bool,
    got_data: bool,
    input_state: InputState,
    last_char: Option<u8>,
    bom: Vec<u8>,
    esc_charset_prober: Option<EscCharSetProber>,
    charset_probers: Vec<Box<dyn CharsetProber>>,
}

impl UniversalDetector {
    /// Falha com [`Error::UnsupportedEncoding`] onde o construtor do original lança `Error`.
    pub fn new(options: Option<&DetectOptions>) -> Result<Self, Error> {
        let default_options = DetectOptions::default();
        let options = options.unwrap_or(&default_options);

        let minimum_threshold = match (options.min_confidence, &options.detect_encodings) {
            (Some(threshold), _) => threshold,
            // If encodings are narrowed down by the user allow for any threshold to be returned.
            (None, Some(_)) => 0.0,
            (None, None) => 0.20,
        };

        if let Some(detect_encodings) = &options.detect_encodings {
            for encoding in detect_encodings {
                if !supported_encodings_denormalized().contains(&encoding.to_lowercase()) {
                    return Err(Error::UnsupportedEncoding { encoding: encoding.clone(), supported: supported_encodings().to_vec() });
                }
            }
        }

        let mut detector = Self {
            detect_encodings: options.detect_encodings.clone(),
            minimum_threshold,
            result: None,
            results: Vec::new(),
            done: false,
            got_data: false,
            input_state: InputState::PureAscii,
            last_char: None,
            bom: Vec::new(),
            esc_charset_prober: None,
            charset_probers: Vec::new(),
        };
        detector.reset();
        Ok(detector)
    }

    /// `this.result`: `None` onde o original tem `{"encoding": null, "confidence": 0.0}`.
    pub fn result(&self) -> Option<&Detected> {
        self.result.as_ref()
    }

    /// `this.results`.
    pub fn results(&self) -> &[Detected] {
        &self.results
    }

    fn can_detect_encoding(&self, encoding: &str) -> bool {
        match &self.detect_encodings {
            None => true,
            Some(detect_encodings) => {
                let encoding = encoding.to_lowercase();
                detect_encodings.iter().any(|candidate| candidate.to_lowercase() == encoding)
            }
        }
    }

    pub fn reset(&mut self) {
        self.result = None;
        self.results = Vec::new();
        self.done = false;
        self.got_data = false;
        self.input_state = InputState::PureAscii;
        self.last_char = None;
        self.bom = Vec::new();
        if let Some(esc_charset_prober) = &mut self.esc_charset_prober {
            esc_charset_prober.reset();
        }
        for prober in &mut self.charset_probers {
            prober.reset();
        }
    }

    pub fn feed(&mut self, buf: &[u8]) -> Result<(), Error> {
        if self.done || buf.is_empty() {
            return Ok(());
        }

        if !self.got_data {
            // Só os quatro primeiros bytes importam para o BOM, e `_mBOM.length > 3` vira `len >= 4`.
            let room = 4usize.saturating_sub(self.bom.len());
            self.bom.extend(buf.iter().take(room));
            // If the data starts with BOM, we know it is UTF
            let bom_encodings: [(&[u8], &str); 7] = [
                (&[0xEF, 0xBB, 0xBF], "UTF-8"),                      // EF BB BF  UTF-8 with BOM
                (&[0xFF, 0xFE, 0x00, 0x00], "UTF-32LE"),             // FF FE 00 00  UTF-32, little-endian BOM
                (&[0x00, 0x00, 0xFE, 0xFF], "UTF-32BE"),             // 00 00 FE FF  UTF-32, big-endian BOM
                (&[0xFE, 0xFF, 0x00, 0x00], "X-ISO-10646-UCS-4-3412"), // FE FF 00 00  UCS-4, unusual octet order BOM (3412)
                (&[0x00, 0x00, 0xFF, 0xFE], "X-ISO-10646-UCS-4-2143"), // 00 00 FF FE  UCS-4, unusual octet order BOM (2143)
                (&[0xFF, 0xFE], "UTF-16LE"),                         // FF FE  UTF-16, little endian BOM
                (&[0xFE, 0xFF], "UTF-16BE"),                         // FE FF  UTF-16, big endian BOM
            ];
            for (bom, encoding) in bom_encodings {
                if self.bom.starts_with(bom) && self.can_detect_encoding(encoding) {
                    self.result = Some(Detected { encoding: encoding.to_string(), confidence: 1.0 });
                    break;
                }
            }

            if let Some(result) = &self.result
                && result.confidence > 0.0
            {
                self.results = vec![result.clone()];
            }

            // If we got to 4 chars without being able to detect a BOM we stop trying.
            if self.bom.len() > 3 {
                self.got_data = true;
            }
        }

        if self.result.as_ref().is_some_and(|result| result.confidence > 0.0) {
            self.done = true;
            return Ok(());
        }

        if self.input_state == InputState::PureAscii {
            if buf.iter().any(|&byte| byte >= 0x80) {
                self.input_state = InputState::HighByte;
            } else if has_esc(self.last_char, buf) {
                self.input_state = InputState::EscAscii;
            }
        }

        self.last_char = buf.last().copied();

        if self.input_state == InputState::EscAscii {
            let esc_charset_prober = self.esc_charset_prober.get_or_insert_with(EscCharSetProber::new);
            let found_it = esc_charset_prober.feed(buf)? == FOUND_IT;
            let name = esc_charset_prober.get_charset_name();
            let confidence = esc_charset_prober.get_confidence();
            if found_it
                && let Some(name) = name
                && self.can_detect_encoding(&name)
            {
                self.record_found(name, confidence);
            }
        } else if self.input_state == InputState::HighByte {
            if self.charset_probers.is_empty() {
                self.charset_probers = vec![Box::new(mbcs_group_prober()), Box::new(sbcs_group_prober()), Box::new(Latin1Prober::new())];
            }
            for index in 0..self.charset_probers.len() {
                let found_it = self.charset_probers[index].feed(buf)? == FOUND_IT;
                let name = self.charset_probers[index].get_charset_name();
                if found_it
                    && let Some(name) = name
                    && self.can_detect_encoding(&name)
                {
                    let confidence = self.charset_probers[index].get_confidence();
                    self.record_found(name, confidence);
                    break;
                }
            }
        }

        Ok(())
    }

    fn record_found(&mut self, encoding: String, confidence: f64) {
        let result = Detected { encoding, confidence };
        self.results = vec![result.clone()];
        self.result = Some(result);
        self.done = true;
    }

    pub fn close(&mut self) -> Option<Detected> {
        if self.done {
            return None;
        }
        if self.bom.is_empty() {
            return None;
        }
        self.done = true;

        if self.input_state == InputState::PureAscii && self.can_detect_encoding("ascii") {
            let result = Detected { encoding: "ascii".to_string(), confidence: 1.0 };
            self.result = Some(result.clone());
            self.results.push(result.clone());
            return Some(result);
        }

        if self.input_state == InputState::HighByte {
            let mut windows_1252_confidence = 0.0;
            let mut windows_1250_detected = false;
            let mut found = Vec::new();
            for prober in &self.charset_probers {
                let charset_name = prober.get_charset_name();
                let confidence = prober.get_confidence();
                if charset_name.as_deref() == Some("windows-1252") {
                    windows_1252_confidence = confidence;
                }
                let Some(charset_name) = charset_name else { continue };
                if !self.can_detect_encoding(&charset_name) {
                    continue;
                }
                if charset_name == "windows-1250" {
                    windows_1250_detected = true;
                }
                found.push(Detected { encoding: charset_name, confidence });
            }
            self.results.extend(found);
            // HACK: When windows-1252 is detected it's almost sure that it can
            // also be windows-1250.
            // https://en.wikipedia.org/wiki/Windows-1250 (Central European)
            if windows_1252_confidence != 0.0 && !windows_1250_detected && self.can_detect_encoding("windows-1250") {
                self.results.push(Detected {
                    encoding: "windows-1250".to_string(),
                    // Report the confidence just a bit under windows-1252's.
                    confidence: windows_1252_confidence - 0.5f64.powi(js_number_to_string(windows_1252_confidence).len() as i32 - 1),
                });
            }
            self.results.sort_by(|a, b| b.confidence.partial_cmp(&a.confidence).unwrap_or(std::cmp::Ordering::Equal));
            if let Some(top_result) = self.results.first()
                && top_result.confidence >= self.minimum_threshold
            {
                let top_result = top_result.clone();
                self.result = Some(top_result.clone());
                return Some(top_result);
            }
        }

        None
    }
}

/// `this._escDetector.test(this._mLastChar.join('') + aBuf)`, com `/(\x1B|~\{)/`.
fn has_esc(last_char: Option<u8>, buf: &[u8]) -> bool {
    let mut previous = last_char;
    for &byte in buf {
        if byte == 0x1B || (previous == Some(b'~') && byte == b'{') {
            return true;
        }
        previous = Some(byte);
    }
    false
}

/// `String(number)` do JavaScript para um número finito (a especificação de `Number::toString`): o que o
/// original mede com `.length` no ajuste do `windows-1250`.
fn js_number_to_string(value: f64) -> String {
    if value == 0.0 {
        return "0".to_string();
    }
    let sign = if value < 0.0 { "-" } else { "" };
    let scientific = format!("{:e}", value.abs());
    let Some((mantissa, exponent)) = scientific.split_once('e') else { return value.to_string() };
    let Ok(exponent) = exponent.parse::<i32>() else { return value.to_string() };
    let digits: String = mantissa.chars().filter(|c| *c != '.').collect();
    let k = digits.len() as i32; // quantidade de dígitos significativos
    let n = exponent + 1; // posição do ponto decimal em relação aos dígitos

    let body = if k <= n && n <= 21 {
        format!("{digits}{}", "0".repeat((n - k) as usize))
    } else if 0 < n && n <= 21 {
        format!("{}.{}", &digits[..n as usize], &digits[n as usize..])
    } else if -6 < n && n <= 0 {
        format!("0.{}{digits}", "0".repeat((-n) as usize))
    } else {
        let exponent_sign = if n - 1 < 0 { "-" } else { "+" };
        let exponent_digits = (n - 1).abs();
        if k == 1 { format!("{digits}e{exponent_sign}{exponent_digits}") } else { format!("{}.{}e{exponent_sign}{exponent_digits}", &digits[..1], &digits[1..]) }
    };
    format!("{sign}{body}")
}
