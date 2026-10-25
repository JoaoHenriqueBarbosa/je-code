//! Porte de `src/vs/platform/tunnel/test/common/tunnel.test.ts`.
//!
//! `ensureNoDisposablesAreLeakedInTestSuite` não tem equivalente: o teste não cria nenhum descartável.

#![cfg(not(target_arch = "wasm32"))]

use je_base::uri::URI;
use je_platform::tunnel::{LocalHostUriMetaData, extract_local_host_uri_meta_data_for_port_mapping, extract_query_local_host_uri_meta_data_for_port_mapping};

fn port_mapping_do_test(uri: &str, func: fn(&URI) -> Option<LocalHostUriMetaData>, expected_address: Option<&str>, expected_port: Option<u32>) {
    let res = func(&URI::parse(uri, false));
    assert_eq!(expected_address.is_none(), res.is_none(), "{uri}");
    assert_eq!(res.as_ref().map(|result| result.address.as_str()), expected_address, "{uri}");
    assert_eq!(res.as_ref().map(|result| result.port), expected_port, "{uri}");
}

fn port_mapping_test(uri: &str, expected_address: Option<&str>, expected_port: Option<u32>) {
    port_mapping_do_test(uri, extract_local_host_uri_meta_data_for_port_mapping, expected_address, expected_port);
}

fn port_mapping_test_query(uri: &str, expected_address: Option<&str>, expected_port: Option<u32>) {
    port_mapping_do_test(uri, extract_query_local_host_uri_meta_data_for_port_mapping, expected_address, expected_port);
}

#[test]
fn port_mapping() {
    port_mapping_test("file:///foo.bar/baz", None, None);
    port_mapping_test("http://foo.bar:1234", None, None);
    port_mapping_test("http://localhost:8080", Some("localhost"), Some(8080));
    port_mapping_test("https://localhost:443", Some("localhost"), Some(443));
    port_mapping_test("http://127.0.0.1:3456", Some("127.0.0.1"), Some(3456));
    port_mapping_test("http://0.0.0.0:7654", Some("0.0.0.0"), Some(7654));
    port_mapping_test("http://localhost:8080/path?foo=bar", Some("localhost"), Some(8080));
    port_mapping_test("http://localhost:8080/path?foo=http%3A%2F%2Flocalhost%3A8081", Some("localhost"), Some(8080));
    port_mapping_test_query("http://foo.bar/path?url=http%3A%2F%2Flocalhost%3A8081", Some("localhost"), Some(8081));
    port_mapping_test_query("http://foo.bar/path?url=http%3A%2F%2Flocalhost%3A8081&url2=http%3A%2F%2Flocalhost%3A8082", Some("localhost"), Some(8081));
    port_mapping_test_query("http://foo.bar/path?url=http%3A%2F%2Fmicrosoft.com%2Fbad&url2=http%3A%2F%2Flocalhost%3A8081", Some("localhost"), Some(8081));
}
