//! Caminhos de extensão. Dentro do crate, um local é o **caminho da URI** (`URI#path`, sempre com `/`), a forma
//! que o `joinPath`, o `isEqualOrParent` e o `basename` do original usam; a conversão para o caminho do sistema de
//! arquivos (`URI#fsPath`) e para o JSON de `URI#toJSON` (o formato do `extensions.json`) fica nas bordas.

use je_platform::resources::host_platform;
use je_base::path::{Platform, PathModule, ProcessEnvironment};
use je_base::uri::URI;
use serde_json::{Map, Value};

const ENV: ProcessEnvironment = ProcessEnvironment;

fn posix() -> PathModule<'static> {
    PathModule::posix(&ENV)
}

/// `resources.joinPath` sobre o caminho da URI: junta e normaliza (`..` e `.`).
pub fn join_path(base: &str, fragments: &[&str]) -> String {
    let mut all = vec![base];
    all.extend_from_slice(fragments);
    posix().join(&all)
}

/// `resources.dirname`.
pub fn dirname(path: &str) -> String {
    posix().dirname(path)
}

/// `resources.basename`.
pub fn basename(path: &str) -> String {
    posix().basename(path, None)
}

/// `resources.isEqualOrParent(candidate, base)`: o `candidate` é o `base` ou está dentro dele. Em plataformas de
/// caixa insensível a comparação seria sem caixa; aqui segue a semântica posix, a do Linux do oráculo.
pub fn is_equal_or_parent(candidate: &str, base: &str) -> bool {
    je_base::extpath::is_equal_or_parent(candidate, base, false, true, &ENV)
}

/// `resources.isEqual`.
pub fn is_equal(a: &str, b: &str) -> bool {
    a == b
}

fn file_uri(uri_path: &str) -> URI {
    if cfg!(windows) {
        // `/c:/dir` -> `c:\dir`; um caminho sem letra de unidade (UNC) segue como está.
        let bytes = uri_path.as_bytes();
        let drive = bytes.len() >= 3 && bytes[0] == b'/' && bytes[1].is_ascii_alphabetic() && bytes[2] == b':';
        let fs = if drive { uri_path[1..].replace('/', "\\") } else { uri_path.replace('/', "\\") };
        URI::file(fs, Platform::Win32)
    } else {
        URI::file(uri_path, Platform::Posix)
    }
}

/// O caminho do sistema de arquivos (`URI#fsPath`) de um caminho de URI.
pub fn to_fs_path(uri_path: &str) -> String {
    file_uri(uri_path).fs_path(host_platform()).to_string()
}

/// O caminho da URI (`URI.file(fsPath).path`) de um caminho do sistema de arquivos.
pub fn from_fs_path(fs_path: &str) -> String {
    URI::file(fs_path, host_platform()).path().to_string()
}

/// `URI.file(fsPath).toJSON()` como o `JSON.stringify` o escreve: `$mid`, `fsPath`, `external`, `path` e `scheme`.
/// É o formato do campo `location` do `extensions.json` do VS Code.
pub fn uri_json(uri_path: &str) -> Value {
    let uri = file_uri(uri_path);
    // `toJSON` só inclui o que já foi calculado: força os dois caches, como o `fsPath` e o `toString` fazem.
    let fs_path = uri.fs_path(host_platform()).to_string();
    let external = uri.to_string(false).to_string();
    let mut object = Map::new();
    object.insert("$mid".to_owned(), Value::from(1));
    if !fs_path.is_empty() {
        object.insert("fsPath".to_owned(), Value::from(fs_path));
    }
    object.insert("external".to_owned(), Value::from(external));
    object.insert("path".to_owned(), Value::from(uri.path().to_string()));
    object.insert("scheme".to_owned(), Value::from("file"));
    Value::Object(object)
}
