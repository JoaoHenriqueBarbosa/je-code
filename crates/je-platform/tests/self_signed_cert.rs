//! Porte de `src/vs/platform/tunnel/test/node/selfSignedCert.test.ts`.
//!
//! O original lê o certificado com o `crypto.X509Certificate` do Node e valida o par chave/certificado com `tls.createServer`
//! mais `tls.connect`. Aqui o X.509 é lido por um leitor DER mínimo (escrito neste arquivo, só o que o teste consulta:
//! emissor, assunto, validade, SAN e o número de série), e o TLS é o `rustls` (provedor RustCrypto, Rust puro): o servidor
//! com a chave e o certificado gerados e um cliente sem verificação de certificado (o `rejectUnauthorized: false`).
//!
//! Adaptações ao Rust: `tls.createSecureContext({ key, cert })` (que no OpenSSL rejeita um INTEGER fora da forma canônica
//! com `ASN.1 INVALID_INTEGER`) é a montagem do `ServerConfig` do `rustls`, que lê a chave e confere o certificado; o teste da
//! regressão confere, além disso, a forma canônica do número de série no próprio DER, para não depender de o leitor ser
//! rígido. `ensureNoDisposablesAreLeakedInTestSuite` não tem equivalente (o teste não cria descartáveis).

#![cfg(not(target_arch = "wasm32"))]

use std::io::{self, Read};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, OnceLock};
use std::thread;
use std::time::Duration;

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use jiff::Timestamp;
use jiff::tz::TimeZone;
use je_platform::tunnel::self_signed_cert::{ISelfSignedCert, generate_self_signed_cert};
use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer, ServerName, UnixTime};
use rustls::{DigitallySignedStruct, SignatureScheme};
use sha2::{Digest, Sha256};

/// O prazo de cada leitura do handshake: um teste que trava falha em vez de pendurar.
const IO_TIMEOUT: Duration = Duration::from_secs(20);

/// `suiteSetup`: o certificado compartilhado pelos casos.
fn cert() -> &'static ISelfSignedCert {
    static CERT: OnceLock<ISelfSignedCert> = OnceLock::new();
    CERT.get_or_init(|| generate_self_signed_cert().expect("generate the self-signed certificate"))
}

// #region Leitor DER mínimo

/// Um TLV: a tag, o conteúdo e o que vem depois.
fn read_tlv(input: &[u8]) -> (u8, &[u8], &[u8]) {
    let tag = input[0];
    let (length, header) = if input[1] < 0x80 {
        (usize::from(input[1]), 2)
    } else {
        let count = usize::from(input[1] & 0x7f);
        (input[2..2 + count].iter().fold(0usize, |length, byte| (length << 8) | usize::from(*byte)), 2 + count)
    };
    (tag, &input[header..header + length], &input[header + length..])
}

/// Um TLV com a tag esperada: o conteúdo e o resto.
fn expect_tlv(input: &[u8], expected_tag: u8) -> (&[u8], &[u8]) {
    let (tag, content, rest) = read_tlv(input);
    assert_eq!(tag, expected_tag, "unexpected ASN.1 tag");
    (content, rest)
}

/// `pemToDer` do original: o base64 do PEM, sem as linhas `-----...-----` nem os espaços.
fn pem_to_der(pem: &str) -> Vec<u8> {
    let b64: String = pem.lines().filter(|line| !line.starts_with("-----")).flat_map(str::chars).filter(|c| !c.is_whitespace()).collect();
    STANDARD.decode(b64).expect("valid base64 in the PEM")
}

/// O que o teste consulta de um `X509Certificate`.
struct X509 {
    serial: Vec<u8>,
    issuer: String,
    subject: String,
    valid_from: Timestamp,
    valid_to: Timestamp,
    subject_alt_name: Option<String>,
}

/// O `issuer`/`subject` do Node: `CN=<valor>`.
fn name_to_string(name: &[u8]) -> String {
    let (name, rest) = expect_tlv(name, 0x30);
    assert!(rest.is_empty());
    let (relative_name, rest) = expect_tlv(name, 0x31);
    assert!(rest.is_empty());
    let (attribute, rest) = expect_tlv(relative_name, 0x30);
    assert!(rest.is_empty());
    let (oid, value) = expect_tlv(attribute, 0x06);
    assert_eq!(oid, [0x55, 0x04, 0x03], "the attribute is not a CN");
    let (value, rest) = expect_tlv(value, 0x0c);
    assert!(rest.is_empty());
    format!("CN={}", String::from_utf8_lossy(value))
}

/// `UTCTime` (`YYMMDDHHMMSSZ`) ou `GeneralizedTime` (`YYYYMMDDHHMMSSZ`) em UTC.
fn parse_time(tag: u8, content: &[u8]) -> Timestamp {
    let text = std::str::from_utf8(content).expect("ASCII time");
    let digits = text.strip_suffix('Z').expect("a time in UTC");
    let (year, rest): (i16, &str) = match tag {
        0x17 => {
            let two_digits: i16 = digits[..2].parse().expect("a two-digit year");
            // RFC 5280: 50-99 are 1950-1999 and 00-49 are 2000-2049.
            (if two_digits >= 50 { 1900 + two_digits } else { 2000 + two_digits }, &digits[2..])
        }
        0x18 => (digits[..4].parse().expect("a four-digit year"), &digits[4..]),
        other => panic!("unexpected time tag {other:#x}"),
    };
    let field = |index: usize| -> i8 { rest[index * 2..index * 2 + 2].parse().expect("a two-digit time field") };
    jiff::civil::date(year, field(0), field(1)).at(field(2), field(3), field(4), 0).to_zoned(TimeZone::UTC).expect("a valid time").timestamp()
}

/// O `subjectAltName` do Node: `IP Address:a.b.c.d`, separado por `, `.
fn subject_alt_name_to_string(extension_value: &[u8]) -> String {
    let (names, rest) = expect_tlv(extension_value, 0x30);
    assert!(rest.is_empty());
    let mut entries = Vec::new();
    let mut remaining = names;
    while !remaining.is_empty() {
        let (tag, content, rest) = read_tlv(remaining);
        // [7] iPAddress
        assert_eq!(tag, 0x87, "unexpected GeneralName");
        let address: Vec<String> = content.iter().map(u8::to_string).collect();
        entries.push(format!("IP Address:{}", address.join(".")));
        remaining = rest;
    }
    entries.join(", ")
}

fn parse_x509(der: &[u8]) -> X509 {
    let (certificate, rest) = expect_tlv(der, 0x30);
    assert!(rest.is_empty(), "trailing bytes after the certificate");
    let (tbs, rest) = expect_tlv(certificate, 0x30);
    // signatureAlgorithm and signatureValue
    let (_signature_algorithm, rest) = expect_tlv(rest, 0x30);
    let (_signature, rest) = expect_tlv(rest, 0x03);
    assert!(rest.is_empty());

    // [0] EXPLICIT version, serialNumber, signature, issuer, validity, subject, subjectPublicKeyInfo, [3] extensions
    let (version, rest) = expect_tlv(tbs, 0xa0);
    assert_eq!(version, [0x02, 0x01, 0x02], "not an X.509 v3 certificate");
    let (serial, rest) = expect_tlv(rest, 0x02);
    let (_signature_algorithm, rest) = expect_tlv(rest, 0x30);
    let (issuer_content, rest) = expect_tlv(rest, 0x30);
    let (validity, rest) = expect_tlv(rest, 0x30);
    let (subject_content, rest) = expect_tlv(rest, 0x30);
    let (_spki, rest) = expect_tlv(rest, 0x30);
    let (extensions, rest) = expect_tlv(rest, 0xa3);
    assert!(rest.is_empty());

    let (not_before_tag, not_before, validity_rest) = read_tlv(validity);
    let (not_after_tag, not_after, validity_rest) = read_tlv(validity_rest);
    assert!(validity_rest.is_empty());

    let (extensions, rest) = expect_tlv(extensions, 0x30);
    assert!(rest.is_empty());
    let mut subject_alt_name = None;
    let mut remaining = extensions;
    while !remaining.is_empty() {
        let (extension, rest) = expect_tlv(remaining, 0x30);
        let (oid, extension_rest) = expect_tlv(extension, 0x06);
        let (value, _) = expect_tlv(extension_rest, 0x04);
        // 2.5.29.17 (subjectAltName)
        if oid == [0x55, 0x1d, 0x11] {
            subject_alt_name = Some(subject_alt_name_to_string(value));
        }
        remaining = rest;
    }

    // `read_tlv` precisa do TLV inteiro (a tag e o comprimento) para o nome, então refaz o cabeçalho.
    let wrap = |content: &[u8]| -> Vec<u8> { [vec![0x30], der_length(content.len()), content.to_vec()].concat() };
    X509 {
        serial: serial.to_vec(),
        issuer: name_to_string(&wrap(issuer_content)),
        subject: name_to_string(&wrap(subject_content)),
        valid_from: parse_time(not_before_tag, not_before),
        valid_to: parse_time(not_after_tag, not_after),
        subject_alt_name,
    }
}

fn der_length(length: usize) -> Vec<u8> {
    if length < 0x80 {
        vec![length as u8]
    } else {
        let bytes: Vec<u8> = length.to_be_bytes().into_iter().skip_while(|byte| *byte == 0).collect();
        [vec![0x80 | bytes.len() as u8], bytes].concat()
    }
}

/// X.690 section 8.3.2: o INTEGER positivo usa o menor número de octetos possível.
fn assert_canonical_positive_der_integer(content: &[u8]) {
    assert!(!content.is_empty(), "INTEGER must contain at least one octet");
    assert_eq!(content[0] & 0x80, 0, "the serial must be positive");
    if content.len() > 1 {
        assert!(!(content[0] == 0x00 && content[1] & 0x80 == 0), "non-canonical DER INTEGER: {content:02x?}");
    }
}

// #endregion

// #region TLS

/// `rejectUnauthorized: false`: aceita o certificado autoassinado do proxy.
#[derive(Debug)]
struct AcceptAnyCertificate;

impl ServerCertVerifier for AcceptAnyCertificate {
    fn verify_server_cert(&self, _end_entity: &CertificateDer<'_>, _intermediates: &[CertificateDer<'_>], _server_name: &ServerName<'_>, _ocsp_response: &[u8], _now: UnixTime) -> Result<ServerCertVerified, rustls::Error> {
        Ok(ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(&self, _message: &[u8], _cert: &CertificateDer<'_>, _dss: &DigitallySignedStruct) -> Result<HandshakeSignatureValid, rustls::Error> {
        Ok(HandshakeSignatureValid::assertion())
    }

    fn verify_tls13_signature(&self, _message: &[u8], _cert: &CertificateDer<'_>, _dss: &DigitallySignedStruct) -> Result<HandshakeSignatureValid, rustls::Error> {
        Ok(HandshakeSignatureValid::assertion())
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        vec![SignatureScheme::ECDSA_NISTP256_SHA256, SignatureScheme::ECDSA_NISTP384_SHA384, SignatureScheme::ED25519, SignatureScheme::RSA_PSS_SHA256, SignatureScheme::RSA_PKCS1_SHA256]
    }
}

/// `tls.createSecureContext({ key, cert })` / `tls.createServer({ key, cert })`.
fn server_config(cert: &ISelfSignedCert) -> Result<Arc<rustls::ServerConfig>, rustls::Error> {
    let provider = Arc::new(rustls_rustcrypto::provider());
    rustls::ServerConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()?
        .with_no_client_auth()
        .with_single_cert(vec![CertificateDer::from(pem_to_der(&cert.cert))], PrivateKeyDer::from(PrivatePkcs8KeyDer::from(pem_to_der(&cert.key))))
        .map(Arc::new)
}

type TlsClient = rustls::StreamOwned<rustls::ClientConnection, TcpStream>;

/// `tls.connect({ host: '127.0.0.1', port, rejectUnauthorized: false })`, com o handshake já feito.
fn tls_connect(port: u16) -> io::Result<TlsClient> {
    let tcp = TcpStream::connect(("127.0.0.1", port))?;
    tcp.set_read_timeout(Some(IO_TIMEOUT))?;
    let config = rustls::ClientConfig::builder_with_provider(Arc::new(rustls_rustcrypto::provider()))
        .with_safe_default_protocol_versions()
        .map_err(io::Error::other)?
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(AcceptAnyCertificate))
        .with_no_client_auth();
    let server_name = ServerName::try_from("127.0.0.1").map_err(io::Error::other)?;
    let connection = rustls::ClientConnection::new(Arc::new(config), server_name).map_err(io::Error::other)?;
    let mut client = rustls::StreamOwned::new(connection, tcp);
    while client.conn.is_handshaking() {
        client.conn.complete_io(&mut client.sock)?;
    }
    Ok(client)
}

// #endregion

#[test]
fn returns_pem_encoded_key() {
    let cert = cert();
    assert!(cert.key.starts_with("-----BEGIN PRIVATE KEY-----"));
    assert!(cert.key.trim_end().ends_with("-----END PRIVATE KEY-----"));
}

#[test]
fn returns_pem_encoded_certificate() {
    let cert = cert();
    assert!(cert.cert.starts_with("-----BEGIN CERTIFICATE-----"));
    assert!(cert.cert.trim_end().ends_with("-----END CERTIFICATE-----"));
}

#[test]
fn fingerprint_is_in_electron_sha256_base64_format() {
    let cert = cert();
    assert!(cert.fingerprint.starts_with("sha256/"));
    let b64 = &cert.fingerprint["sha256/".len()..];
    // SHA-256 digest is 32 bytes -> 44 base64 chars (with padding)
    assert_eq!(STANDARD.decode(b64).expect("base64 fingerprint").len(), 32);
}

#[test]
fn fingerprint_matches_the_certificate_der() {
    let cert = cert();
    let der = pem_to_der(&cert.cert);
    let expected = format!("sha256/{}", STANDARD.encode(Sha256::digest(&der)));
    assert_eq!(cert.fingerprint, expected);
}

#[test]
fn certificate_can_be_parsed_as_x509() {
    let x509 = parse_x509(&pem_to_der(&cert().cert));
    assert_eq!(x509.issuer, "CN=TunnelProxy");
    assert_eq!(x509.subject, "CN=TunnelProxy");
}

#[test]
fn certificate_has_san_with_ip_127_0_0_1() {
    let x509 = parse_x509(&pem_to_der(&cert().cert));
    assert!(
        x509.subject_alt_name.as_deref().is_some_and(|san| san.contains("IP Address:127.0.0.1")),
        "Expected SAN to include IP 127.0.0.1, got: {:?}",
        x509.subject_alt_name
    );
}

#[test]
fn certificate_is_not_yet_expired() {
    let x509 = parse_x509(&pem_to_der(&cert().cert));
    let now = Timestamp::now();
    assert!(x509.valid_from <= now, "Certificate not yet valid: notBefore={}", x509.valid_from);
    assert!(x509.valid_to > now, "Certificate already expired: notAfter={}", x509.valid_to);
}

#[test]
fn key_and_certificate_form_a_valid_tls_pair() {
    // Start a TLS server with the generated cert, connect to it, and verify the handshake completes successfully.
    let cert = cert();
    let config = server_config(cert).expect("a TLS server from the generated key and certificate");
    let server = TcpListener::bind(("127.0.0.1", 0)).expect("listen");
    let port = server.local_addr().expect("server address").port();
    let handshake = thread::spawn(move || -> io::Result<()> {
        let (mut tcp, _) = server.accept()?;
        tcp.set_read_timeout(Some(IO_TIMEOUT))?;
        let mut connection = rustls::ServerConnection::new(config).map_err(io::Error::other)?;
        while connection.is_handshaking() {
            connection.complete_io(&mut tcp)?;
        }
        // O cliente fecha com `close_notify`: lê até o fim para o servidor não cair com a conexão aberta.
        let mut rest = Vec::new();
        let _ = rustls::Stream::new(&mut connection, &mut tcp).read_to_end(&mut rest);
        Ok(())
    });

    // Handshake succeeded
    let mut client = tls_connect(port).expect("the TLS handshake");
    let peer_certificates = client.conn.peer_certificates().expect("Should have a peer certificate");
    let peer = parse_x509(peer_certificates[0].as_ref());
    assert_eq!(peer.subject, "CN=TunnelProxy");
    client.conn.send_close_notify();
    let _ = client.conn.complete_io(&mut client.sock);

    handshake.join().expect("the server thread").expect("the server handshake");
}

#[test]
fn each_invocation_produces_a_unique_certificate() {
    let cert = cert();
    let cert2 = generate_self_signed_cert().expect("generate another certificate");
    // Different key pair -> different fingerprint
    assert_ne!(cert.fingerprint, cert2.fingerprint);
    assert_ne!(cert.key, cert2.key);
}

#[test]
fn produces_canonical_der_integer_serials_across_many_runs() {
    // Regression: a random 8-byte serial with `serial[0] &= 0x7f` occasionally yields a leading 0x00 byte where the next
    // byte's high bit is unset: that's non-canonical DER and OpenSSL rejects it with ASN.1 INVALID_INTEGER when the cert is
    // loaded. Run enough iterations that the probability of hitting the bad path is overwhelming: P(failure per cert) ~ 1/256,
    // so 200 runs gives P(no failure) ~ (255/256)^200 ~ 46%, comfortably high enough that a buggy encoder fails most
    // invocations.
    for _ in 0..200 {
        let c = generate_self_signed_cert().expect("generate a certificate");
        // Constructing a TLS server config is what triggers the loading error path.
        server_config(&c).expect("a TLS server from the generated key and certificate");
        assert_canonical_positive_der_integer(&parse_x509(&pem_to_der(&c.cert)).serial);
    }
}
