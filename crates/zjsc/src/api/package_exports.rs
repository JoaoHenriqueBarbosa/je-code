//! Campo `exports` do `package.json` na resolução de pacotes do Bun (medido no bun 1.4.2).
//!
//! Cobre `exports` como string, como objeto de subcaminhos exatos (`"."`, `"./x"`) e como objeto de
//! condições (`import`, `require`, `bun`, `node`, `default`, na ordem do objeto; as demais chaves são
//! ignoradas e o import ESM escolhe `import`). Sem `exports` (ou com `exports: null`) vale o
//! `main`/`index` do pacote, pelo mesmo caminho de `resolve_node_modules`. Também padrões com `*` (vale
//! o de maior prefixo; o trecho capturado entra em todo `*` do alvo), pastas `./x/` e vetores de alvos (o
//! primeiro elemento que decide vence; objeto sem condição casada passa ao próximo). `./package.json`
//! sempre resolve ao próprio arquivo, mesmo fora de `exports`.

use super::module_probe::{import_not_found_message, require_not_found_message, resolve_node_modules_at, ModuleFs};
use super::package_json::{parse, Json};

/// Alvo `./x` válido: relativo ao pacote, sem `..` nem `node_modules` nos segmentos, sem barra final.
fn valid_target(target: &str) -> bool {
  target.starts_with("./")
    && !target.ends_with('/')
    && target[2..].split('/').all(|seg| !matches!(seg, ".." | "." | "node_modules" | ""))
}

/// Pasta `./x/` (obsoleta): alvo relativo terminado em `/`, sem `..` nem `node_modules` nos segmentos.
fn valid_folder(target: &str) -> bool {
  target.len() >= 3
    && target.starts_with("./")
    && target.ends_with('/')
    && target[2..target.len() - 1].split('/').all(|seg| !matches!(seg, ".." | "." | "node_modules" | ""))
}

/// O trecho que o `*` (ou a pasta) captura não pode escapar do pacote.
fn safe_rest(rest: &str) -> bool {
  rest.split('/').all(|seg| !matches!(seg, ".." | "." | "node_modules"))
}

/// O que a chave casada injeta no alvo: nada (subcaminho exato), o trecho do `*` ou o resto da pasta.
#[derive(Clone, Copy)]
enum Subst<'a> {
  Exact,
  Star(&'a str),
  Folder(&'a str),
}

fn condition_matches(key: &str, esm: bool) -> bool {
  match key {
    "import" => esm,
    "require" => !esm,
    "bun" | "node" | "default" => true,
    _ => false,
  }
}

/// Resolve um alvo. `None`: nada casou, o chamador tenta a próxima chave; `Some(None)`: falha definitiva
/// (alvo inválido, `null`); `Some(Some(rel))`: caminho relativo ao pacote. A primeira condição que casa
/// decide, mesmo que o arquivo não exista.
fn target(value: &Json, esm: bool, subst: Subst) -> Option<Option<String>> {
  match value {
    Json::Str(t) => Some(match subst {
      Subst::Exact => valid_target(t).then(|| t[2..].to_string()),
      Subst::Star(star) => (valid_target(t) && safe_rest(star)).then(|| {
        let joined = t[2..].replace('*', star);
        joined.split('/').filter(|seg| !seg.is_empty()).collect::<Vec<_>>().join("/")
      }),
      Subst::Folder(rest) => (valid_folder(t) && safe_rest(rest)).then(|| format!("{}{rest}", &t[2..])),
    }),
    Json::Object(members) => members
      .iter()
      .filter(|(key, _)| condition_matches(key, esm))
      .find_map(|(_, inner)| target(inner, esm, subst)),
    // Vetor: o primeiro elemento que decide vence; objeto sem condição casada passa ao próximo.
    Json::Array(items) => items.iter().find_map(|inner| target(inner, esm, subst)),
    _ => Some(None),
  }
}

/// Chave de padrão (`prefixo*sufixo`, um só `*`) ou de pasta (`./x/`) que casa `subpath`: devolve o tamanho
/// do prefixo (quanto maior, mais específico) e o que injetar. O trecho capturado nunca é vazio.
fn key_match<'a>(key: &str, subpath: &'a str) -> Option<(usize, Subst<'a>)> {
  if key.matches('*').count() == 1 {
    let at = key.find('*')?;
    let (prefix, suffix) = (&key[..at], &key[at + 1..]);
    let fits = subpath.len() > prefix.len() + suffix.len() && subpath.starts_with(prefix) && subpath.ends_with(suffix);
    return fits.then(|| (prefix.len(), Subst::Star(&subpath[prefix.len()..subpath.len() - suffix.len()])));
  }
  let folder = key.ends_with('/') && subpath.len() > key.len() && subpath.starts_with(key);
  folder.then(|| (key.len(), Subst::Folder(&subpath[key.len()..])))
}

fn lookup(exports: &Json, subpath: &str, esm: bool) -> Option<String> {
  let Json::Object(members) = exports else {
    return if subpath == "." { target(exports, esm, Subst::Exact).flatten() } else { None };
  };
  let dotted = members.iter().filter(|(key, _)| key.starts_with('.')).count();
  if dotted == 0 {
    return if subpath == "." { target(exports, esm, Subst::Exact).flatten() } else { None };
  }
  if dotted != members.len() {
    return None;
  }
  // Subcaminho exato primeiro; um `null` exato cai nos padrões (medido no bun), um `null` de padrão bloqueia.
  if let Some((_, value)) = members.iter().find(|(key, _)| key == subpath) {
    if !matches!(value, Json::Null) {
      return target(value, esm, Subst::Exact).flatten();
    }
  }
  // Padrão ou pasta mais específico: maior prefixo, depois a chave mais longa.
  let (_, _, value, subst) = members
    .iter()
    .filter_map(|(key, value)| key_match(key, subpath).map(|(prefix, subst)| (prefix, key.len(), value, subst)))
    .max_by_key(|(prefix, key_len, _, _)| (*prefix, *key_len))?;
  target(value, esm, subst).flatten()
}

/// Resolve `specifier` (`pkg`, `pkg/sub`, `@s/pkg/sub`) como `require` (`esm` falso) ou `import` (`esm`
/// verdadeiro). `Err` é `<CÓDIGO>: <mensagem>`; no import a mensagem cita `t.mjs`, o importador que o golden
/// normaliza. Use [`resolve_package_from`] para informar o importador real.
pub fn resolve_package(fs: &dyn ModuleFs, importer_dir: &str, specifier: &str, esm: bool) -> Result<String, String> {
  resolve_package_from(fs, importer_dir, specifier, esm, if esm { "t.mjs" } else { "t.cjs" })
}

/// Como [`resolve_package`], com o nome do arquivo importador para as mensagens de erro.
pub fn resolve_package_from(
  fs: &dyn ModuleFs,
  importer_dir: &str,
  specifier: &str,
  esm: bool,
  importer: &str,
) -> Result<String, String> {
  let not_found = || {
    if esm {
      format!("ERR_MODULE_NOT_FOUND: {}", import_not_found_message(specifier, importer))
    } else {
      let first = require_not_found_message(specifier, importer);
      format!("MODULE_NOT_FOUND: {}", first.lines().next().unwrap_or(""))
    }
  };
  let segments = if specifier.starts_with('@') { 2 } else { 1 };
  let parts: Vec<&str> = specifier.splitn(segments + 1, '/').collect();
  if parts.len() < segments {
    return Err(not_found());
  }
  let name = parts[..segments].join("/");
  let subpath = match parts.get(segments) {
    Some(rest) => format!("./{rest}"),
    None => ".".to_string(),
  };
  let spec = specifier.replace('\\', "/");
  let mut dir = importer_dir.trim_end_matches('/').to_string();
  loop {
    if !dir.ends_with("/node_modules") {
      let pkg_dir = format!("{dir}/node_modules/{name}");
      let manifest = fs.read_file(&format!("{pkg_dir}/package.json")).as_deref().and_then(parse);
      if let Some(exports) = manifest.as_ref().and_then(|m| m.get("exports")).filter(|e| !matches!(e, Json::Null)) {
        let exported = |subpath: &str| {
          let relative = if subpath == "./package.json" { Some("package.json".to_string()) } else { lookup(exports, subpath, esm) };
          relative.map(|rel| format!("{pkg_dir}/{rel}")).filter(|path| fs.is_file(path))
        };
        // Medido no bun: se o subcaminho não leva a arquivo, um `.js` final cai fora e a busca repete (`./x.js` com
        // `"./*": "./src/*.mjs"` acha `src/x.mjs`; o `./x.js.js` que existe vence). Nenhum outro sufixo e nenhum
        // arquivo fora do `exports` (`./b.js` com só `"./a"` falha).
        let file = exported(&subpath).or_else(|| subpath.strip_suffix(".js").filter(|rest| rest.len() > 1).and_then(exported));
        return file.map(|path| fs.realpath(&path).unwrap_or(path)).ok_or_else(not_found);
      }
    }
    if let Some(found) = resolve_node_modules_at(fs, &dir, &spec) {
      return Ok(found);
    }
    if dir.is_empty() {
      return Err(not_found());
    }
    dir.truncate(dir.rfind('/').unwrap_or(0));
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::api::module_probe::MemoryFs;

  fn fs(exports: &str) -> MemoryFs {
    let manifest = format!("{{\"exports\":{exports},\"main\":\"main.js\"}}");
    MemoryFs::new(&[
      ("/node_modules/pkg/package.json", &manifest),
      ("/node_modules/pkg/a.js", "1"),
      ("/node_modules/pkg/b.js", "1"),
      ("/node_modules/pkg/i.mjs", "1"),
      ("/node_modules/pkg/r.cjs", "1"),
      ("/node_modules/pkg/def.js", "1"),
      ("/node_modules/pkg/bun.js", "1"),
      ("/node_modules/pkg/node.js", "1"),
      ("/node_modules/pkg/main.js", "1"),
    ])
  }

  fn ok(exports: &str, specifier: &str, esm: bool) -> String {
    resolve_package(&fs(exports), "/", specifier, esm).unwrap()
  }

  #[test]
  fn string_exports_only_dot() {
    let fs = fs("\"./a.js\"");
    assert_eq!(resolve_package(&fs, "/", "pkg", false).unwrap(), "/node_modules/pkg/a.js");
    assert_eq!(resolve_package(&fs, "/", "pkg/x", false).unwrap_err(), "MODULE_NOT_FOUND: Cannot find module 'pkg/x'");
  }

  #[test]
  fn subpath_object_and_null() {
    let fs = fs("{\".\":\"./a.js\",\"./b\":\"./b.js\",\"./c\":null}");
    assert_eq!(resolve_package(&fs, "/", "pkg/b", true).unwrap(), "/node_modules/pkg/b.js");
    assert_eq!(
      resolve_package(&fs, "/", "pkg/c", true).unwrap_err(),
      "ERR_MODULE_NOT_FOUND: Cannot find package 'pkg' imported from t.mjs"
    );
    assert_eq!(resolve_package(&fs, "/", "pkg/package.json", false).unwrap(), "/node_modules/pkg/package.json");
  }

  #[test]
  fn conditions_follow_object_order_and_the_mode() {
    let both = "{\"import\":\"./i.mjs\",\"require\":\"./r.cjs\"}";
    assert_eq!(ok(both, "pkg", false), "/node_modules/pkg/r.cjs");
    assert_eq!(ok(both, "pkg", true), "/node_modules/pkg/i.mjs");
    let late_default = "{\".\":{\"default\":\"./def.js\",\"import\":\"./i.mjs\"}}";
    assert_eq!(ok(late_default, "pkg", true), "/node_modules/pkg/def.js");
    let bun_first = "{\".\":{\"bun\":\"./bun.js\",\"node\":\"./node.js\"}}";
    assert_eq!(ok(bun_first, "pkg", true), "/node_modules/pkg/bun.js");
    let node_first = "{\".\":{\"node\":\"./node.js\",\"bun\":\"./bun.js\"}}";
    assert_eq!(ok(node_first, "pkg", false), "/node_modules/pkg/node.js");
    let nested = "{\".\":{\"import\":{\"nope\":\"./a.js\"},\"default\":\"./def.js\"}}";
    assert_eq!(ok(nested, "pkg", true), "/node_modules/pkg/def.js");
  }

  #[test]
  fn a_matching_condition_is_final() {
    let null_import = fs("{\".\":{\"import\":null,\"default\":\"./def.js\"}}");
    assert!(resolve_package(&null_import, "/", "pkg", true).is_err());
    assert_eq!(resolve_package(&null_import, "/", "pkg", false).unwrap(), "/node_modules/pkg/def.js");
    let missing = fs("{\".\":{\"import\":\"./nope.js\",\"default\":\"./def.js\"}}");
    assert!(resolve_package(&missing, "/", "pkg", true).is_err());
    let mixed = fs("{\".\":\"./a.js\",\"import\":\"./i.mjs\"}");
    assert!(resolve_package(&mixed, "/", "pkg", false).is_err());
    assert!(resolve_package(&fs("{\".\":{\"nope\":\"./a.js\"}}"), "/", "pkg", false).is_err());
    assert!(resolve_package(&fs("{}"), "/", "pkg", false).is_err());
    assert!(resolve_package(&fs("true"), "/", "pkg", false).is_err());
  }

  fn tree(exports: &str, files: &[&str]) -> MemoryFs {
    let manifest = format!("{{\"exports\":{exports}}}");
    let mut all: Vec<(String, String)> = vec![("/node_modules/pkg/package.json".into(), manifest)];
    all.extend(files.iter().map(|f| (format!("/node_modules/pkg/{f}"), "1".to_string())));
    let refs: Vec<(&str, &str)> = all.iter().map(|(p, c)| (p.as_str(), c.as_str())).collect();
    MemoryFs::new(&refs)
  }

  #[test]
  fn star_patterns_pick_the_longest_prefix_and_substitute_everywhere() {
    let fs = tree("{\"./f/*\":\"./lib/*.js\",\"./f/x/*\":\"./lib/y/*\"}", &["lib/q.js", "lib/y/g.js", "lib/q.js.js"]);
    assert_eq!(resolve_package(&fs, "/", "pkg/f/q", true).unwrap(), "/node_modules/pkg/lib/q.js");
    assert_eq!(resolve_package(&fs, "/", "pkg/f/q.js", true).unwrap(), "/node_modules/pkg/lib/q.js.js");
    assert_eq!(resolve_package(&fs, "/", "pkg/f/x/g.js", true).unwrap(), "/node_modules/pkg/lib/y/g.js");
  }

  #[test]
  fn star_edge_cases() {
    let fs = tree("{\"./f/*\":\"./lib/*.js\"}", &["lib/x.js", "lib/y/z.js"]);
    assert!(resolve_package(&fs, "/", "pkg/f/", false).is_err());
    assert_eq!(resolve_package(&fs, "/", "pkg/f/y/z", false).unwrap(), "/node_modules/pkg/lib/y/z.js");
    assert!(resolve_package(&fs, "/", "pkg/f/../x", false).is_err());
    let blocked = tree("{\"./f/*\":\"./lib/*\",\"./f/y/*\":null}", &["lib/x.js", "lib/y/z.js"]);
    assert!(resolve_package(&blocked, "/", "pkg/f/y/z.js", false).is_err());
    assert!(resolve_package(&blocked, "/", "pkg/f/x.js", false).is_ok());
    let exact_null = tree("{\"./f/*\":\"./lib/*\",\"./f/x.js\":null}", &["lib/x.js"]);
    assert_eq!(resolve_package(&exact_null, "/", "pkg/f/x.js", false).unwrap(), "/node_modules/pkg/lib/x.js");
    let multi = tree("{\"./f*\":\"./lib/*\"}", &["lib/x.js"]);
    assert_eq!(resolve_package(&multi, "/", "pkg/f/x.js", false).unwrap(), "/node_modules/pkg/lib/x.js");
    let conditions = tree("{\"./f/*\":{\"import\":\"./lib/*.js\",\"default\":\"./a.js\"}}", &["lib/x.js", "a.js"]);
    assert_eq!(resolve_package(&conditions, "/", "pkg/f/x", true).unwrap(), "/node_modules/pkg/lib/x.js");
    assert_eq!(resolve_package(&conditions, "/", "pkg/f/x", false).unwrap(), "/node_modules/pkg/a.js");
  }

  #[test]
  fn trailing_slash_folders() {
    let fs = tree("{\"./a/\":\"./lib/\"}", &["lib/x.js", "lib/y/z.js"]);
    assert_eq!(resolve_package(&fs, "/", "pkg/a/x.js", true).unwrap(), "/node_modules/pkg/lib/x.js");
    assert_eq!(resolve_package(&fs, "/", "pkg/a/y/z.js", true).unwrap(), "/node_modules/pkg/lib/y/z.js");
    assert!(resolve_package(&fs, "/", "pkg/a/", true).is_err());
    assert!(resolve_package(&fs, "/", "pkg/a/nope.js", true).is_err());
  }

  #[test]
  fn arrays_take_the_first_deciding_element() {
    let first = tree("[\"./a.js\",\"./b.js\"]", &["a.js", "b.js"]);
    assert_eq!(resolve_package(&first, "/", "pkg", true).unwrap(), "/node_modules/pkg/a.js");
    let skipped = tree("{\".\":[{\"nope\":\"./b.js\"},\"./a.js\"]}", &["a.js", "b.js"]);
    assert_eq!(resolve_package(&skipped, "/", "pkg", true).unwrap(), "/node_modules/pkg/a.js");
    let missing = tree("{\".\":[\"./nope.js\",\"./a.js\"]}", &["a.js"]);
    assert!(resolve_package(&missing, "/", "pkg", true).is_err());
    let invalid = tree("{\".\":[\"i.mjs\",\"./a.js\"]}", &["a.js"]);
    assert!(resolve_package(&invalid, "/", "pkg", true).is_err());
    assert!(resolve_package(&tree("{\".\":[]}", &["a.js"]), "/", "pkg", true).is_err());
  }

  #[test]
  fn no_exports_falls_back_to_main_and_index() {
    assert_eq!(ok("null", "pkg", false), "/node_modules/pkg/main.js");
    let plain = MemoryFs::new(&[("/node_modules/q/package.json", "{}"), ("/node_modules/q/index.js", "")]);
    assert_eq!(resolve_package(&plain, "/", "q", true).unwrap(), "/node_modules/q/index.js");
    assert!(resolve_package(&plain, "/", "q/zzz", true).is_err());
  }

  /// Medido no bun 1.4.2 (`require.resolve("pkg/x.js")`): sem arquivo para o subcaminho, um `.js` final cai fora e a busca
  /// repete uma vez; o mapeamento exato que existe vence; nenhum outro sufixo; nenhum arquivo fora do `exports`.
  #[test]
  fn trailing_js_is_dropped_once_when_the_subpath_maps_to_nothing() {
    let pattern = |exports: &str, files: &[&str]| {
      let manifest = format!("{{\"main\":\"main.js\",\"exports\":{exports}}}");
      let mut all = vec![("/node_modules/pkg/package.json".to_string(), manifest)];
      all.extend(files.iter().map(|file| (format!("/node_modules/pkg/{file}"), "1".to_string())));
      let refs: Vec<(&str, &str)> = all.iter().map(|(path, text)| (path.as_str(), text.as_str())).collect();
      MemoryFs::new(&refs)
    };
    let to_mjs = pattern(r#"{"./*":"./src/*.mjs"}"#, &["src/x.mjs"]);
    assert_eq!(resolve_package(&to_mjs, "/", "pkg/x.js", false).unwrap(), "/node_modules/pkg/src/x.mjs");
    let both = pattern(r#"{"./*":"./src/*.js"}"#, &["src/x.js", "src/x.js.js"]);
    assert_eq!(resolve_package(&both, "/", "pkg/x.js", false).unwrap(), "/node_modules/pkg/src/x.js.js");
    let one = pattern(r#"{"./*":"./src/*.js"}"#, &["src/x.js"]);
    assert_eq!(resolve_package(&one, "/", "pkg/x.js", false).unwrap(), "/node_modules/pkg/src/x.js");
    assert!(resolve_package(&one, "/", "pkg/x.js.js", false).is_err(), "só um `.js` cai");
    assert!(resolve_package(&one, "/", "pkg/x.mjs", false).is_err(), "só o `.js` cai");
    let exact = pattern(r#"{"./a":"./a.js"}"#, &["a.js", "b.js"]);
    assert_eq!(resolve_package(&exact, "/", "pkg/a.js", false).unwrap(), "/node_modules/pkg/a.js");
    assert!(resolve_package(&exact, "/", "pkg/b.js", false).is_err(), "o arquivo fora do `exports` não resolve");
  }
}
