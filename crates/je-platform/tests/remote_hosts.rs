//! Porte de `src/vs/platform/remote/test/common/remoteHosts.test.ts` (8 casos, na ordem do original).

use je_base::JsString;
use je_platform::remote::remote_hosts::{
    HostAndPort, is_loopback_host, parse_authority_with_optional_port, parse_authority_with_port,
};

fn js(text: &str) -> JsString {
    JsString::from(text)
}

fn host_and_port(host: &str, port: f64) -> HostAndPort {
    HostAndPort { host: js(host), port }
}

#[test]
fn parse_authority_hostname() {
    assert_eq!(parse_authority_with_port(&js("localhost:8080")), host_and_port("localhost", 8080.0));
}

#[test]
fn parse_authority_ipv4() {
    assert_eq!(parse_authority_with_port(&js("127.0.0.1:8080")), host_and_port("127.0.0.1", 8080.0));
}

#[test]
fn parse_authority_ipv6() {
    assert_eq!(
        parse_authority_with_port(&js("[2001:0db8:85a3:0000:0000:8a2e:0370:7334]:8080")),
        host_and_port("[2001:0db8:85a3:0000:0000:8a2e:0370:7334]", 8080.0)
    );
}

#[test]
fn parse_authority_with_optional_port_hostname() {
    assert_eq!(parse_authority_with_optional_port(&js("localhost:8080"), 123.0), host_and_port("localhost", 8080.0));
    assert_eq!(parse_authority_with_optional_port(&js("localhost"), 123.0), host_and_port("localhost", 123.0));
}

#[test]
fn parse_authority_with_optional_port_ipv4() {
    assert_eq!(parse_authority_with_optional_port(&js("127.0.0.1:8080"), 123.0), host_and_port("127.0.0.1", 8080.0));
    assert_eq!(parse_authority_with_optional_port(&js("127.0.0.1"), 123.0), host_and_port("127.0.0.1", 123.0));
}

#[test]
fn parse_authority_with_optional_port_ipv6() {
    assert_eq!(
        parse_authority_with_optional_port(&js("[2001:0db8:85a3:0000:0000:8a2e:0370:7334]:8080"), 123.0),
        host_and_port("[2001:0db8:85a3:0000:0000:8a2e:0370:7334]", 8080.0)
    );
    assert_eq!(
        parse_authority_with_optional_port(&js("[2001:0db8:85a3:0000:0000:8a2e:0370:7334]"), 123.0),
        host_and_port("[2001:0db8:85a3:0000:0000:8a2e:0370:7334]", 123.0)
    );
}

#[test]
fn issue_151748_remote_authorities_containing_plus_need_to_be_resolved() {
    assert_eq!(
        parse_authority_with_optional_port(&js("codespaces+aaaaa-aaaaa-aaaa-aaaaa-a111aa111"), 123.0),
        host_and_port("codespaces+aaaaa-aaaaa-aaaa-aaaaa-a111aa111", 123.0)
    );
}

#[test]
fn is_loopback_host_cases() {
    // loopback hosts
    assert!(is_loopback_host(&js("localhost")));
    assert!(is_loopback_host(&js("LOCALHOST")));
    assert!(is_loopback_host(&js("127.0.0.1")));
    assert!(is_loopback_host(&js("::1")));
    assert!(is_loopback_host(&js("[::1]")));

    // non-loopback hosts
    assert!(!is_loopback_host(&js("evil.com")));
    assert!(!is_loopback_host(&js("192.168.0.1")));
    assert!(!is_loopback_host(&js("10.0.0.1")));
    assert!(!is_loopback_host(&js("[2001:0db8:85a3:0000:0000:8a2e:0370:7334]")));
}
