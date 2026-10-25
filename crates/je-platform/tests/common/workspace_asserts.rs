//! Auxiliares que `windowsStateHandler.test.ts` e `workspacesHistoryStorage.test.ts` definem iguais: `toWorkspace`,
//! `assertEqualURI`, `assertEqualWorkspace` e o `join(tmpdir(), ...)` dos caminhos de teste.

use je_base::uri::URI;
use je_platform::workspace::WorkspaceIdentifier;

/// `join(tmpdir(), ...segments)`: só conta de caminho, nenhum arquivo é tocado.
pub fn temp_path(segments: &[&str]) -> String {
    let tmpdir = std::env::temp_dir().to_string_lossy().into_owned();
    let mut parts = vec![tmpdir.as_str()];
    parts.extend_from_slice(segments);
    super::join(&parts)
}

/// `toWorkspace(uri)`.
pub fn to_workspace(uri: URI) -> WorkspaceIdentifier {
    WorkspaceIdentifier { id: "1234".to_string(), config_path: uri }
}

fn uri_text(uri: Option<&URI>) -> Option<String> {
    uri.map(|uri| uri.to_string(false).to_string_lossy())
}

/// `assertEqualURI`: compara o `toString()`.
pub fn assert_equal_uri(u1: Option<&URI>, u2: Option<&URI>, message: &str) {
    assert_eq!(uri_text(u1), uri_text(u2), "{message}");
}

/// `assertEqualWorkspace`.
pub fn assert_equal_workspace(w1: Option<&WorkspaceIdentifier>, w2: Option<&WorkspaceIdentifier>, message: &str) {
    let (Some(w1), Some(w2)) = (w1, w2) else {
        assert_eq!(w1, w2, "{message}");
        return;
    };
    assert_eq!(w1.id, w2.id, "{message}");
    assert_equal_uri(Some(&w1.config_path), Some(&w2.config_path), message);
}
