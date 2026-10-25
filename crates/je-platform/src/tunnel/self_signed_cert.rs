//! Porte de `src/vs/platform/tunnel/node/selfSignedCert.ts`: o certificado autoassinado ECDSA (P-256) para `127.0.0.1`
//! do proxy de túnel, montado em DER à mão (um X.509 v3 mínimo) e assinado com o `p256` (RustCrypto, Rust puro).
//!
//! Decisões de tradução:
//! - `crypto.generateKeyPairSync('ec', { namedCurve: 'prime256v1' })` é `p256::ecdsa::SigningKey::random`; a chave sai em
//!   PKCS#8 PEM e a pública em SPKI DER, como o Node entrega.
//! - `crypto.createSign('SHA256').sign(key)` produz a assinatura ECDSA em DER (o formato padrão do Node): é o
//!   `Signature::to_der` do `p256`, que faz o SHA-256 da mensagem como o Node.
//! - `notAfter.setFullYear(now.getFullYear() + 1)` soma um ano no calendário local; um 29 de fevereiro sem
//!   equivalente no ano seguinte vira 1 de março, como no JavaScript.

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use je_base::errors::Error;
use jiff::civil::Date;
use jiff::tz::TimeZone;
use jiff::Timestamp;
use p256::ecdsa::{Signature, SigningKey, signature::Signer};
use p256::pkcs8::{EncodePrivateKey, EncodePublicKey, LineEnding};
use rand_core::{OsRng, RngCore};
use sha2::{Digest, Sha256};

/// `ISelfSignedCert`: result of generating a self-signed certificate.
#[derive(Clone, Debug)]
pub struct ISelfSignedCert {
    /// PEM-encoded private key.
    pub key: String,
    /// PEM-encoded X.509 certificate.
    pub cert: String,
    /// SHA-256 fingerprint in Electron's format: `sha256/<base64>`.
    pub fingerprint: String,
}

/// `generateSelfSignedCert`: generate a self-signed ECDSA (P-256) certificate for `127.0.0.1`. The certificate is
/// valid for one year from the current time.
///
/// **Security note:** this certificate is a defence-in-depth measure for a
/// proxy that is already bound exclusively to `127.0.0.1`. TLS prevents
/// other local processes from passively sniffing tunnel traffic and the
/// pinned fingerprint stops active MITM on loopback. If certificate
/// generation fails the proxy simply will not start; the failure is
/// non-critical to the overall application.
///
/// Do not rely on this certificate for security-critical scenarios.
pub fn generate_self_signed_cert() -> Result<ISelfSignedCert, Error> {
    let signing_key = SigningKey::random(&mut OsRng);
    let private_key_pem = signing_key.to_pkcs8_pem(LineEnding::LF).map_err(|error| Error::new(format!("failed to encode the private key: {error}")))?.to_string();
    let public_key_der = signing_key.verifying_key().to_public_key_der().map_err(|error| Error::new(format!("failed to encode the public key: {error}")))?;

    let cert_der = create_self_signed_cert_der(&signing_key, public_key_der.as_bytes())?;
    let cert = der_to_pem(&cert_der);

    // Compute SHA-256 fingerprint in Electron's format: "sha256/<base64>"
    let hash = STANDARD.encode(Sha256::digest(&cert_der));
    Ok(ISelfSignedCert { key: private_key_pem, cert, fingerprint: format!("sha256/{hash}") })
}

/// `createSelfSignedCertPem` sem a codificação PEM: build a minimal self-signed X.509 v3 certificate in DER. Uses raw
/// ASN.1 construction to avoid external dependencies.
fn create_self_signed_cert_der(signing_key: &SigningKey, spki_der: &[u8]) -> Result<Vec<u8>, Error> {
    // Build the TBS (To Be Signed) certificate
    let mut serial = [0u8; 8];
    OsRng.fill_bytes(&mut serial);
    // Ensure serial is positive (clear high bit)
    serial[0] &= 0x7f;

    let now = Timestamp::now();
    let not_after = one_year_later(now)?;

    let cn_oid = der_oid(&[0x55, 0x04, 0x03])?; // 2.5.4.3

    let issuer_and_subject = der_sequence(&[der_set(&[der_sequence(&[cn_oid, der_utf8_string("TunnelProxy")?])?])?])?;

    // Validity
    let validity = der_sequence(&[der_time(now)?, der_time(not_after)?])?;

    // Version v3 [0] EXPLICIT INTEGER 2
    let version = vec![0xa0, 0x03, 0x02, 0x01, 0x02];

    // Serial number
    let serial_number = der_integer(&serial)?;

    // Signature algorithm: ecdsa-with-SHA256 (1.2.840.10045.4.3.2)
    let sig_alg = der_sequence(&[der_oid(&[0x2a, 0x86, 0x48, 0xce, 0x3d, 0x04, 0x03, 0x02])?])?;

    // Extensions [3] EXPLICIT SEQUENCE: SAN with IP 127.0.0.1
    let san_extension = build_san_extension()?;
    let extensions = [vec![0xa3], der_length_prefix(&der_sequence(&[san_extension])?)?].concat();

    // TBSCertificate
    let tbs = der_sequence(&[version, serial_number, sig_alg.clone(), issuer_and_subject.clone(), validity, issuer_and_subject, spki_der.to_vec(), extensions])?;

    // Sign the TBS
    let signature: Signature = signing_key.sign(&tbs);
    let signature = signature.to_der();
    let signature = signature.as_bytes();

    // Wrap signature as BIT STRING
    let sig_bit_string = [vec![0x03], der_length(signature.len() + 1)?, vec![0x00] /* no unused bits */, signature.to_vec()].concat();

    // Full certificate
    der_sequence(&[tbs, sig_alg, sig_bit_string])
}

/// PEM encode.
fn der_to_pem(der: &[u8]) -> String {
    let b64 = STANDARD.encode(der);
    let lines: Vec<&str> = b64.as_bytes().chunks(64).map(|line| std::str::from_utf8(line).unwrap_or_default()).collect();
    format!("-----BEGIN CERTIFICATE-----\n{}\n-----END CERTIFICATE-----\n", lines.join("\n"))
}

/// `notAfter.setFullYear(now.getFullYear() + 1)`, no calendário local.
fn one_year_later(now: Timestamp) -> Result<Timestamp, Error> {
    let zone = TimeZone::system();
    let local = now.to_zoned(zone.clone()).datetime();
    let date = local.date();
    let next_year = date.year() + 1;
    let target = Date::new(next_year, date.month(), date.day()).or_else(|_| Date::new(next_year, 3, 1)).map_err(|error| Error::new(format!("invalid expiry date: {error}")))?;
    target.to_datetime(local.time()).to_zoned(zone).map(|zoned| zoned.timestamp()).map_err(|error| Error::new(format!("invalid expiry date: {error}")))
}

/// Build a SAN extension with IP:127.0.0.1
fn build_san_extension() -> Result<Vec<u8>, Error> {
    // Extension OID: 2.5.29.17 (subjectAltName)
    let san_oid = der_oid(&[0x55, 0x1d, 0x11])?;

    // GeneralName: iPAddress [7] 127.0.0.1
    let ip_bytes = vec![0x87, 0x04, 0x7f, 0x00, 0x00, 0x01];

    let san_value = der_octet_string(&der_sequence(&[ip_bytes])?)?;

    der_sequence(&[san_oid, san_value])
}

// #region ASN.1 DER helpers

/// `pemToDer`: o conteúdo base64 do PEM, sem as linhas `-----...-----` nem os espaços.
pub(crate) fn pem_to_der(pem: &str) -> Result<Vec<u8>, Error> {
    let b64: String = pem.lines().filter(|line| !line.starts_with("-----")).flat_map(str::chars).filter(|c| !c.is_whitespace()).collect();
    STANDARD.decode(b64).map_err(|error| Error::new(format!("invalid PEM: {error}")))
}

fn der_length(length: usize) -> Result<Vec<u8>, Error> {
    if length < 0x80 {
        Ok(vec![(length & 0xff) as u8])
    } else if length < 0x100 {
        Ok(vec![0x81, (length & 0xff) as u8])
    } else if length < 0x10000 {
        Ok(vec![0x82, ((length >> 8) & 0xff) as u8, (length & 0xff) as u8])
    } else if length < 0x100_0000 {
        Ok(vec![0x83, ((length >> 16) & 0xff) as u8, ((length >> 8) & 0xff) as u8, (length & 0xff) as u8])
    } else {
        // X.690 section 8.1.3 allows up to 0x7f length octets, but anything
        // beyond 3 bytes (16 MiB) is well outside our use case and most
        // likely indicates a bug. Fail loudly rather than silently emit
        // a truncated, malformed length.
        Err(Error::new(format!("derLength: value too large ({length})")))
    }
}

fn der_length_prefix(content: &[u8]) -> Result<Vec<u8>, Error> {
    Ok([der_length(content.len())?, content.to_vec()].concat())
}

/// Tag, comprimento e conteúdo.
fn der_tlv(tag: u8, content: &[u8]) -> Result<Vec<u8>, Error> {
    Ok([vec![tag], der_length(content.len())?, content.to_vec()].concat())
}

fn der_sequence(items: &[Vec<u8>]) -> Result<Vec<u8>, Error> {
    der_tlv(0x30, &items.concat())
}

fn der_set(items: &[Vec<u8>]) -> Result<Vec<u8>, Error> {
    der_tlv(0x31, &items.concat())
}

fn der_integer(value: &[u8]) -> Result<Vec<u8>, Error> {
    // Canonical DER INTEGER encoding (X.690 section 8.3.2): the contents octets
    // must use the smallest number of octets. Strip leading 0x00 bytes
    // that are not needed to keep the high bit unset, then prepend a
    // single 0x00 if the high bit is set (to keep the value positive).
    // INTEGER value MUST contain at least one octet.
    if value.is_empty() {
        return Err(Error::new("derInteger: value must be non-empty"));
    }
    let mut start = 0;
    while start < value.len() - 1 && value[start] == 0 && (value[start + 1] & 0x80) == 0 {
        start += 1;
    }
    let mut content = value[start..].to_vec();
    if content[0] & 0x80 != 0 {
        content.insert(0, 0x00);
    }
    der_tlv(0x02, &content)
}

fn der_oid(value: &[u8]) -> Result<Vec<u8>, Error> {
    der_tlv(0x06, value)
}

fn der_utf8_string(text: &str) -> Result<Vec<u8>, Error> {
    der_tlv(0x0c, text.as_bytes())
}

fn der_octet_string(content: &[u8]) -> Result<Vec<u8>, Error> {
    der_tlv(0x04, content)
}

fn der_time(timestamp: Timestamp) -> Result<Vec<u8>, Error> {
    // RFC 5280 section 4.1.2.5: CAs MUST encode times through 2049 as UTCTime
    // (YY...) and times from 2050 onward as GeneralizedTime (YYYY...).
    // UTCTime two-digit years 50-99 are interpreted as 1950-1999 and
    // 00-49 as 2000-2049, so a UTCTime for 2050 would be misread as 1950.
    let utc = timestamp.to_zoned(TimeZone::UTC);
    let year = utc.year();
    let rest = format!("{:02}{:02}{:02}{:02}{:02}Z", utc.month(), utc.day(), utc.hour(), utc.minute(), utc.second());
    if (1950..2050).contains(&year) {
        // UTCTime: YYMMDDHHMMSSZ
        der_tlv(0x17, format!("{:02}{rest}", year % 100).as_bytes())
    } else {
        // GeneralizedTime: YYYYMMDDHHMMSSZ
        der_tlv(0x18, format!("{year:04}{rest}").as_bytes())
    }
}

// #endregion
