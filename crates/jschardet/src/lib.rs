//! Porte do `jschardet` 3.1.4 (npm), a detecção automática de codificação que o VS Code usa em
//! `guessEncodingByBuffer`. Porte de `src/index.js` e dos demais arquivos de `src/`, um módulo por arquivo.
//!
//! A entrada do original é uma "binary string" (um caractere por byte); aqui é `&[u8]`.

pub mod big5freq;
pub mod big5prober;
pub mod chardistribution;
pub mod charsetgroupprober;
pub mod charsetprober;
pub mod codingstatemachine;
pub mod constants;
pub mod escprober;
pub mod escsm;
pub mod eucjpprober;
pub mod euckrfreq;
pub mod euckrprober;
pub mod euctwfreq;
pub mod euctwprober;
pub mod gb2312freq;
pub mod gb2312prober;
pub mod hebrewprober;
pub mod jisfreq;
pub mod jpcntx;
pub mod langbulgarianmodel;
pub mod langcyrillicmodel;
pub mod langgreekmodel;
pub mod langhebrewmodel;
pub mod langhungarianmodel;
pub mod langthaimodel;
pub mod latin1prober;
pub mod mbcharsetprober;
pub mod mbcsgroupprober;
pub mod mbcssm;
pub mod sbcharsetprober;
pub mod sbcsgroupprober;
pub mod sjisprober;
pub mod universaldetector;
pub mod utf8prober;

use std::fmt;

pub use universaldetector::{DetectOptions, Detected, UniversalDetector};

/// Os casos em que o original lança uma exceção.
#[derive(Debug, Clone, PartialEq)]
pub enum Error {
    /// `Encoding ${encoding} is not supported. Supported encodings: ${supportedEncodings}.`
    UnsupportedEncoding { encoding: String, supported: Vec<String> },
    /// O `TypeError` de `jpcntx.js` quando `jp2CharContext[lastCharOrder]` é `undefined`: a ordem do último
    /// hiragana Shift_JIS passou da última linha da tabela.
    HiraganaOrderOutOfRange(i64),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::UnsupportedEncoding { encoding, supported } => {
                write!(f, "Encoding {encoding} is not supported. Supported encodings: {}.", supported.join(","))
            }
            Error::HiraganaOrderOutOfRange(order) => write!(f, "Cannot read properties of undefined (jp2CharContext[{order}])"),
        }
    }
}

impl std::error::Error for Error {}

/// `jschardet.detect`, com as exceções do original como [`Error`]. `Ok(None)` é o resultado com `encoding`
/// `null`.
pub fn try_detect(bytes: &[u8], options: Option<&DetectOptions>) -> Result<Option<Detected>, Error> {
    Ok(run_universal_detector(bytes, options)?.result().cloned())
}

/// `jschardet.detectAll`, com as exceções do original como [`Error`].
pub fn try_detect_all(bytes: &[u8], options: Option<&DetectOptions>) -> Result<Vec<Detected>, Error> {
    Ok(run_universal_detector(bytes, options)?.results().to_vec())
}

/// `jschardet.detect`: `None` quando o `encoding` do original seria `null` e também onde ele lança.
pub fn detect(bytes: &[u8], options: Option<&DetectOptions>) -> Option<Detected> {
    try_detect(bytes, options).ok().flatten()
}

/// `jschardet.detectAll`: vazio onde o original lança.
pub fn detect_all(bytes: &[u8], options: Option<&DetectOptions>) -> Vec<Detected> {
    try_detect_all(bytes, options).unwrap_or_default()
}

fn run_universal_detector(bytes: &[u8], options: Option<&DetectOptions>) -> Result<UniversalDetector, Error> {
    let mut detector = UniversalDetector::new(options)?;
    detector.reset();
    detector.feed(bytes)?;
    detector.close();
    Ok(detector)
}
