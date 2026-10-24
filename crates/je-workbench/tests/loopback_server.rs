//! Porte de `src/vs/workbench/api/test/node/loopbackServer.test.ts` (suíte `LoopbackAuthServer - getHtml`), caso a caso e
//! na mesma ordem.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` não tem o que conferir: o servidor só abre o socket em `start`, e os casos só
//!   chamam `getHtml`.
//! - `new NullLogger()` é `Rc<NullLogger>`; `URI.from({...})` é `URI::from` com `UriComponents`.
//! - As expressões regulares do original (`/&(lt|gt|quot|apos|amp);/g`, `/"[^"]*"/g`, `/\son[a-z]+\s*=/i`,
//!   `/href="([^"]*)"/`, `/["'<>]/`) viram varreduras equivalentes sobre o texto.

use std::rc::Rc;

use je_base::JsString;
use je_base::uri::{URI, UriComponents};
use je_platform::log::NullLogger;
use je_workbench::api::node::loopback_server::LoopbackAuthServer;

/// Decodes the five entities produced by `htmlAttributeEncodeValue`, the same way a browser
/// does when reading a quoted attribute value back via `getAttribute()`.
fn decode_html_attribute(value: &str) -> String {
    const ENTITIES: [(&str, char); 5] = [("&lt;", '<'), ("&gt;", '>'), ("&quot;", '"'), ("&apos;", '\''), ("&amp;", '&')];
    let mut out = String::new();
    let mut rest = value;
    while let Some(character) = rest.chars().next() {
        match ENTITIES.iter().find(|(entity, _)| rest.starts_with(entity)) {
            Some((entity, decoded)) => {
                out.push(*decoded);
                rest = &rest[entity.len()..];
            }
            None => {
                out.push(character);
                rest = &rest[character.len_utf8()..];
            }
        }
    }
    out
}

/// Extracts the source of the redirect anchor tag. Deliberately keys off page text rather than
/// any attribute added by this change, so a regression fails on its own merits.
fn get_redirect_anchor(html: &str) -> &str {
    let end = html.find(">click here</a>").expect("redirect anchor should be present");
    let start = html[..end].rfind("<a ").expect("redirect anchor should have an opening tag");
    &html[start..=end]
}

/// Strips quoted attribute values, leaving only the region a browser tokenizes as attribute
/// names. Markup inside a quoted value is inert, so this separates a real handler from an
/// escaped one that merely looks like markup.
fn get_anchor_attribute_region(html: &str) -> String {
    let anchor = get_redirect_anchor(html);
    let mut out = String::new();
    let mut rest = anchor;
    while let Some(open) = rest.find('"') {
        match rest[open + 1..].find('"') {
            Some(close) => {
                out.push_str(&rest[..open]);
                rest = &rest[open + 1 + close + 1..];
            }
            None => break,
        }
    }
    out.push_str(rest);
    out
}

/// Extracts the anchor's `href` attribute value, the way a browser parses a quoted attribute.
fn get_redirect_href(html: &str) -> String {
    let anchor = get_redirect_anchor(html);
    let start = anchor.find("href=\"").expect("redirect anchor should have a quoted href attribute") + "href=\"".len();
    let length = anchor[start..].find('"').expect("redirect anchor should have a quoted href attribute");
    anchor[start..start + length].to_owned()
}

/// Extracts the trailing inline `<script>` block that performs the redirect.
fn get_script_block(html: &str) -> &str {
    let start = html.rfind("<script>").expect("inline script block should be present");
    let end = html.rfind("</script>").expect("inline script block should be present");
    assert!(end > start, "inline script block should be present");
    &html[start + "<script>".len()..end]
}

/// `/\son[a-z]+\s*=/i`: um espaço, `on`, letras e `=` (com espaços opcionais antes dele).
fn has_event_handler(region: &str) -> bool {
    let chars: Vec<char> = region.chars().collect();
    (0..chars.len()).any(|i| {
        if !chars[i].is_whitespace() || i + 2 >= chars.len() || !chars[i + 1].eq_ignore_ascii_case(&'o') || !chars[i + 2].eq_ignore_ascii_case(&'n') {
            return false;
        }
        let letters = chars[i + 3..].iter().take_while(|c| c.is_ascii_alphabetic()).count();
        if letters == 0 {
            return false;
        }
        chars[i + 3 + letters..].iter().find(|c| !c.is_whitespace()) == Some(&'=')
    })
}

fn render(app_uri: URI, app_name: &str) -> String {
    LoopbackAuthServer::new(Rc::new(NullLogger::new()), app_uri, app_name).get_html()
}

fn render_default(app_uri: URI) -> String {
    render(app_uri, "Visual Studio Code")
}

/// Builds a callback URI whose path segment carries the given authorization server authority,
/// mirroring how the dynamic auth provider constructs it.
fn app_uri_for_authority(authority: &str) -> URI {
    URI::from(
        &UriComponents {
            scheme: Some("vscode".into()),
            authority: Some("dynamicauthprovider".into()),
            path: Some(JsString::from(format!("/{authority}/redirect"))),
            query: Some("nonce=NONCE123".into()),
            ..Default::default()
        },
        false,
    )
}

#[test]
fn does_not_interpolate_the_app_uri_into_the_inline_script() {
    let app_uri = app_uri_for_authority("x';alert(1)//@evil.example");
    let first = render_default(app_uri.clone());
    let second = render_default(app_uri);

    assert_eq!(
        (get_script_block(&first).contains("vscode://"), get_script_block(&second).contains("alert(1)")),
        (false, false)
    );
}

#[test]
fn encodes_characters_that_would_break_out_of_the_href_attribute() {
    let app_uri = app_uri_for_authority("x\" onmouseover=alert(1) z@evil.example");
    let first = render_default(app_uri.clone());
    let second = render_default(app_uri);

    assert_eq!(
        (
            // A raw quote terminates the attribute early and injects an event handler.
            has_event_handler(&get_anchor_attribute_region(&first)),
            get_redirect_href(&second).contains(['"', '\'', '<', '>']),
        ),
        (false, false)
    );
}

#[test]
fn href_round_trips_to_the_exact_app_uri() {
    // `$waitForUriHandler` compares scheme, authority and path, so the value must survive
    // byte-exact. Production reads `getAttribute`, not `.href`, which normalizes the URL.
    let uris = [
        app_uri_for_authority("login.microsoftonline.com"),
        app_uri_for_authority("x';alert(1)//@evil.example"),
        URI::from(
            &UriComponents {
                scheme: Some("vscode".into()),
                authority: Some("dynamicauthprovider".into()),
                path: Some("/a\"b<c>d&e/redirect".into()),
                query: Some("nonce=ABC&state=XYZ".into()),
                ..Default::default()
            },
            false,
        ),
    ];

    assert_eq!(
        uris.iter().map(|uri| decode_html_attribute(&get_redirect_href(&render_default(uri.clone())))).collect::<Vec<_>>(),
        uris.iter().map(|uri| uri.to_string(true).to_string_lossy()).collect::<Vec<_>>()
    );
}

#[test]
fn escapes_the_app_name() {
    let html = render(app_uri_for_authority("login.example.com"), "Code <img src=x onerror=alert(1)>");

    assert_eq!(
        (html.contains("<img src=x"), html.contains("&lt;img src=x onerror=alert(1)&gt;")),
        (false, true)
    );
}
