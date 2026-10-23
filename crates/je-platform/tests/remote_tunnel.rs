//! Porte de `src/vs/platform/remoteTunnel/test/common/remoteTunnel.test.ts` (suíte `Remote tunnel name
//! normalization`), caso a caso e na mesma ordem.
//!
//! Divergências: `ensureNoDisposablesAreLeakedInTestSuite` não tem o que vigiar (nada aqui é descartável);
//! `assert.deepStrictEqual` sobre arrays de strings é `assert_eq!` sobre `Vec<String>`.

use je_platform::remote_tunnel::remote_tunnel::{MAX_TUNNEL_NAME_LENGTH, normalize_tunnel_name, tunnel_name_from_hostname};

#[test]
fn derives_host_names_the_same_way_the_cli_does() {
    // Mirrors `test_clean_hostname_for_tunnel` in the CLI's
    // `cli/src/tunnels/dev_tunnels.rs`; the two must agree or a machine ends
    // up with one dev tunnel per producer.
    assert_eq!(
        vec![
            tunnel_name_from_hostname("hello123"),
            tunnel_name_from_hostname("-cool-name-"),
            tunnel_name_from_hostname("cool!name with_chars"),
            tunnel_name_from_hostname("z"),
            tunnel_name_from_hostname("Connor-PC"),
            tunnel_name_from_hostname(&"a".repeat(MAX_TUNNEL_NAME_LENGTH + 5)),
        ],
        vec![
            "hello123".to_owned(),
            "cool-name".to_owned(),
            "coolname-with-chars".to_owned(),
            "remote-machine".to_owned(),
            "connor-pc".to_owned(),
            "a".repeat(MAX_TUNNEL_NAME_LENGTH),
        ]
    );
}

#[test]
fn normalizes_explicitly_configured_names() {
    assert_eq!(
        vec![
            normalize_tunnel_name("Connor-PC"),
            normalize_tunnel_name("---Connor-PC"),
            normalize_tunnel_name("Connor!PC"),
            normalize_tunnel_name(&"a".repeat(MAX_TUNNEL_NAME_LENGTH + 1)),
            normalize_tunnel_name("---!@#$"),
        ],
        vec![
            "connor-pc".to_owned(),
            "connor-pc".to_owned(),
            "connorpc".to_owned(),
            "a".repeat(MAX_TUNNEL_NAME_LENGTH),
            String::new(),
        ]
    );
}
