//! Sonda de extensão e de `index` da resolução relativa do Bun (medida no bun 1.4.2).
//!
//! Recebe o caminho já normalizado (sem `?query`) e consulta o sistema de arquivos pelo trait
//! [`ModuleFs`]; devolve o primeiro candidato que é arquivo. Não conhece o VFS nem o host: quem
//! carrega módulos fornece a implementação (a do VFS no sandbox, [`MemoryFs`] nos testes).

use crate::api::package_exports::resolve_package_from;
use crate::wtf::text::conversion_mode::ConversionMode;
use crate::wtf::text::wtf_string::String as WtfString;
use crate::wtf::url::URL;

/// Caminho de um especificador `file://` como o carregador do Bun o enxerga: só o prefixo `file://` em minúsculas
/// conta (`file:x`, `file:/x` e `FILE://x` são nome de pacote); o resto é `URL::fileSystemPath()`. Bytes que não
/// formam UTF-8 dão o caminho vazio. `None` para o que não é `file://`.
pub fn file_url_path(specifier: &str) -> Option<String> {
  if !specifier.starts_with("file://") {
    return None;
  }
  let path = URL::from_string(&WtfString::from_utf8(specifier.as_bytes())).file_system_path();
  Some(String::from_utf8(path.utf8(ConversionMode::LenientConversion)).unwrap_or_default())
}

/// `import.meta.url` de uma chave de módulo: `URL::fileURLWithFileSystemPath(key).string()`.
pub fn file_url_from_path(path: &str) -> String {
  let url = URL::file_url_with_file_system_path(&WtfString::from_utf8(path.as_bytes()));
  String::from_utf8(url.string().utf8(ConversionMode::LenientConversion)).unwrap_or_default()
}

/// `EntryKind` e `ModuleFs` moram no runtime (`runtime/module_fs.rs`), que também lê arquivo pelo `fetch`.
pub use crate::runtime::module_fs::{EntryKind, ModuleFs};

/// Ordem de extensões implícitas, medida com `import.meta.resolveSync("./a")` removendo o vencedor a
/// cada rodada: tsx, jsx, mts, ts, mjs, js, cts, cjs, json. A mesma ordem vale para `index.*`.
pub const IMPLICIT_EXTENSIONS: [&str; 9] = ["tsx", "jsx", "mts", "ts", "mjs", "js", "cts", "cjs", "json"];

/// Extensões TypeScript tentadas quando o especificador termina em extensão JS e o arquivo exato falta
/// (`./b.js` acha `b.ts`, depois `b.tsx`, depois `b.mts`; `./b.mjs` só `b.mts`; `./b.cjs` só `b.cts`;
/// `.jsx` como `.js`). Não há queda para `.jsx`, `.mjs` etc.
fn rewrite_extensions(ext: &str) -> &'static [&'static str] {
  match ext {
    "js" | "jsx" => &["ts", "tsx", "mts"],
    "mjs" => &["mts"],
    "cjs" => &["cts"],
    _ => &[],
  }
}

/// Resolve `path` (absoluto, sem barra final) para um arquivo existente, ou `None`.
///
/// Ordem: o arquivo exato; a reescrita `.js`/`.mjs`/`.cjs` para TypeScript; o caminho mais cada extensão
/// implícita; por fim `path/index.<ext>` na mesma ordem. Arquivo vence diretório (`c.js` antes de
/// `c/index.js`). `trailing_slash` (`./c/`) pula as tentativas de arquivo e vai direto ao `index`.
pub fn probe(fs: &dyn ModuleFs, path: &str, trailing_slash: bool) -> Option<String> {
  probe_with(fs, &IMPLICIT_EXTENSIONS, path, trailing_slash)
}

/// Ordem de extensões do campo `main` do `package.json`, medida no bun 1.4.2 removendo o vencedor a cada
/// rodada: js, cjs, cts, tsx, ts, jsx, json. Difere da implícita: `mjs` e `mts` nunca são sondados (nem no
/// `index.*` do diretório apontado por `main`), só valem por nome exato ou pela reescrita `.mjs` para `.mts`.
pub const MAIN_EXTENSIONS: [&str; 7] = ["js", "cjs", "cts", "tsx", "ts", "jsx", "json"];

fn probe_with(fs: &dyn ModuleFs, exts: &[&str], path: &str, trailing_slash: bool) -> Option<String> {
  let base = path.trim_end_matches('/');
  if !trailing_slash {
    if let Some(found) = probe_file(fs, exts, base) {
      return Some(found);
    }
  }
  exts.iter().map(|e| format!("{base}/index.{e}")).find(|c| fs.is_file(c))
}

/// A parte "arquivo" da sonda: o caminho exato, a reescrita para TypeScript e as extensões implícitas.
fn probe_file(fs: &dyn ModuleFs, exts: &[&str], base: &str) -> Option<String> {
  if fs.is_file(base) {
    return Some(base.to_string());
  }
  let name = base.rsplit('/').next().unwrap_or(base);
  if let Some((stem, ext)) = name.rsplit_once('.') {
    let dir = &base[..base.len() - name.len()];
    for e in rewrite_extensions(ext) {
      let candidate = format!("{dir}{stem}.{e}");
      if fs.is_file(&candidate) {
        return Some(candidate);
      }
    }
  }
  exts.iter().map(|e| format!("{base}.{e}")).find(|c| fs.is_file(c))
}

/// Resolve um especificador nu (`pkg`, `pkg/sub`, `@scope/pkg`, `@scope/pkg/sub`) como o `require` e o
/// `import` do Bun: sobe a partir de `importer_dir` (absoluto, sem barra final) e, em cada diretório que
/// não se chama `node_modules`, junta `node_modules/<especificador>` (normalizando `.`, `..` e `\`, que
/// vale como `/`) e sonda esse caminho como um import relativo: arquivo exato, reescrita para
/// TypeScript, extensões implícitas e só então diretório (`package.json` `main` antes do `index.*`; com
/// barra final, direto ao diretório). Não há noção de pacote: `node_modules/pkg.js` atende `pkg`, um
/// subdiretório com `package.json` usa o `main` dele, e quem falha num nível segue para o pai (um
/// diretório vazio ou sem `index` não bloqueia). O resultado é o caminho canônico (`realpath`), com os
/// links simbólicos resolvidos. Um pacote com `exports` resolve só por ele (`esm` escolhe a condição `import` em vez de
/// `require`; ver [`resolve_package_from`]).
pub fn resolve_node_modules(fs: &dyn ModuleFs, importer_dir: &str, specifier: &str, esm: bool) -> Option<String> {
  resolve_package_from(fs, importer_dir, specifier, esm, "").ok()
}

/// Um nível de [`resolve_node_modules`]: sonda `dir/node_modules/<spec>` (`spec` já com `/` no lugar de `\`)
/// sem subir. `dir` que é `node_modules` não conta. O resultado já passou por `realpath`.
pub(super) fn resolve_node_modules_at(fs: &dyn ModuleFs, dir: &str, spec: &str) -> Option<String> {
  if dir.ends_with("/node_modules") || dir == "node_modules" {
    return None;
  }
  let found = probe_package_path(fs, &join_normalized(&format!("{dir}/node_modules"), spec), spec.ends_with('/'))?;
  Some(fs.realpath(&found).unwrap_or(found))
}

/// `require(specifier)` visto de `importer_dir` (absoluto, sem barra final): caminho relativo (`./`, `../`, `.`,
/// `..`) ou absoluto vai pela sonda de arquivo e diretório e termina em `realpath`; o resto é nome de pacote
/// ([`resolve_node_modules`]). `file://` vira o caminho ([`file_url_path`]). `None` é "Cannot find module".
pub fn resolve_require(fs: &dyn ModuleFs, importer_dir: &str, specifier: &str, esm: bool) -> Option<String> {
  let spec = file_url_path(specifier).unwrap_or_else(|| specifier.to_owned()).replace('\\', "/");
  let path_like = spec.starts_with('/') || spec == "." || spec == ".." || spec.starts_with("./") || spec.starts_with("../");
  if !path_like {
    return resolve_node_modules(fs, importer_dir, &spec, esm);
  }
  let target = join_normalized(importer_dir, &spec);
  let found = probe_package_path(fs, &target, spec.ends_with('/'))?;
  Some(fs.realpath(&found).unwrap_or(found))
}

fn probe_package_path(fs: &dyn ModuleFs, target: &str, slash: bool) -> Option<String> {
  if !slash {
    if let Some(found) = probe_file(fs, &IMPLICIT_EXTENSIONS, target) {
      return Some(found);
    }
  }
  // Com barra final o diretório vem primeiro; se não houver, o bun ainda acha o arquivo (medido no bun 1.4.2:
  // `import('./a.mjs/')` e `import('file:///.../a.mjs/')` carregam `a.mjs`).
  probe_directory(fs, target).or_else(|| if slash { probe_file(fs, &IMPLICIT_EXTENSIONS, target) } else { None })
}

/// Mensagem do `require("pkg")` que não acha o pacote (`importer` é o arquivo que chamou).
pub fn require_not_found_message(specifier: &str, importer: &str) -> String {
  format!("Cannot find module '{specifier}'\nRequire stack:\n- {importer}")
}

/// Mensagem do `import "pkg"` e do `import.meta.resolve("pkg")` que não acham o pacote. Ela cita só o
/// nome do pacote: o primeiro segmento, ou os dois de um pacote com escopo (`@s/p/zzz` vira `@s/p`).
pub fn import_not_found_message(specifier: &str, importer: &str) -> String {
  let segments = if specifier.starts_with('@') { 2 } else { 1 };
  let name = specifier.splitn(segments + 1, '/').take(segments).collect::<Vec<_>>().join("/");
  format!("Cannot find package '{name}' imported from {importer}")
}

/// Resolve um diretório `dir` (absoluto, sem barra final) para um arquivo, como o `import "./d"` do Bun.
///
/// Lê `dir/package.json`; se o campo `main` for uma string não vazia, sonda o caminho dela (relativo a `dir`,
/// ou absoluto) com [`MAIN_EXTENSIONS`], arquivo antes de diretório. Se `main` não achar nada (ausente,
/// vazio, não string, inexistente, `.`, `./`, JSON inválido) cai no `index.*` do próprio `dir` na ordem
/// implícita. `exports`, `module` e qualquer outro campo são ignorados em import relativo, e um
/// `package.json` dentro do diretório apontado por `main` não é consultado.
pub fn probe_directory(fs: &dyn ModuleFs, dir: &str) -> Option<String> {
  if let Some(main) = fs.read_file(&format!("{dir}/package.json")).as_deref().and_then(main_field) {
    let slash = main.ends_with('/');
    let target = join_normalized(dir, &main);
    if target != dir {
      // `main` com barra final pula as extensões, mas o arquivo exato ainda vale (`lib/x.js/` acha `x.js`).
      if slash && fs.is_file(&target) {
        return Some(target);
      }
      if let Some(found) = probe_with(fs, &MAIN_EXTENSIONS, &target, slash) {
        return Some(found);
      }
    }
  }
  probe(fs, dir, true)
}

/// Junta `rel` a `dir` resolvendo `.` e `..`; `rel` que começa com `/` é absoluto. Sem barra final.
fn join_normalized(dir: &str, rel: &str) -> String {
  let joined = if rel.starts_with('/') { rel.to_string() } else { format!("{dir}/{rel}") };
  let mut parts: Vec<&str> = Vec::new();
  for seg in joined.split('/') {
    match seg {
      "" | "." => {}
      ".." => {
        parts.pop();
      }
      s => parts.push(s),
    }
  }
  format!("/{}", parts.join("/"))
}

/// Valor do campo `main` de topo se for string não vazia. O Bun aceita BOM, comentários e vírgula final;
/// JSON inválido vale como "sem `main`" (sem erro). Chave repetida: vale a primeira (não medido além de um
/// caso, em que a primeira era inexistente e o resultado foi o `index`).
fn main_field(text: &str) -> Option<String> {
  match super::package_json::parse(text)?.get("main") {
    Some(super::package_json::Json::Str(main)) if !main.is_empty() => Some(main.clone()),
    _ => None,
  }
}

/// Sistema de arquivos em memória: arquivos com conteúdo, diretórios deduzidos dos ancestrais dos
/// arquivos e links simbólicos (`link -> alvo`). Serve de dublê do VFS nos testes do carregador.
#[derive(Default, Debug)]
pub struct MemoryFs {
  files: std::collections::HashMap<String, String>,
  links: std::collections::HashMap<String, String>,
}

impl MemoryFs {
  pub fn new(files: &[(&str, &str)]) -> Self {
    MemoryFs { files: files.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect(), links: Default::default() }
  }

  pub fn with_link(mut self, link: &str, target: &str) -> Self {
    self.links.insert(link.to_string(), target.to_string());
    self
  }

  /// Resolução componente a componente, como o path_resolution(7): cada componente que é link
  /// vira o alvo, resolvido a partir do diretório já resolvido; `..` sobe desse diretório real
  /// (a raiz sobe para ela mesma). Mais de 40 links seguidos dão `ELOOP` (`None`).
  fn resolve_links(&self, path: &str) -> Option<String> {
    let mut pending: Vec<&str> = path.rsplit('/').collect();
    let mut dir: Vec<&str> = Vec::new();
    let mut followed = 0;
    while let Some(comp) = pending.pop() {
      match comp {
        "" | "." => {}
        ".." => {
          dir.pop();
        }
        name => {
          dir.push(name);
          let Some(target) = self.links.get(&format!("/{}", dir.join("/"))) else { continue };
          followed += 1;
          if followed > 40 {
            return None;
          }
          dir.pop();
          if target.starts_with('/') {
            dir.clear();
          }
          pending.extend(target.rsplit('/'));
        }
      }
    }
    Some(format!("/{}", dir.join("/")))
  }
}

impl ModuleFs for MemoryFs {
  fn read_file(&self, path: &str) -> Option<String> {
    self.files.get(&self.resolve_links(path)?).cloned()
  }

  fn stat(&self, path: &str) -> Option<EntryKind> {
    let resolved = self.resolve_links(path)?;
    if self.files.contains_key(&resolved) {
      return Some(EntryKind::File);
    }
    let prefix = format!("{}/", resolved.trim_end_matches('/'));
    (resolved == "/" || self.files.keys().any(|k| k.starts_with(&prefix))).then_some(EntryKind::Directory)
  }

  fn realpath(&self, path: &str) -> Option<String> {
    let resolved = self.resolve_links(path)?;
    self.stat(&resolved).map(|_| resolved)
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  fn run(files: &[&str], path: &str, slash: bool) -> Option<String> {
    let pairs: Vec<(&str, &str)> = files.iter().map(|f| (*f, "")).collect();
    probe(&MemoryFs::new(&pairs), path, slash)
  }

  #[test]
  fn memory_fs_stat_read_and_realpath() {
    let fs = MemoryFs::new(&[("/d/a.js", "x"), ("/d/lib/b.js", "y")]).with_link("/l", "/d/lib").with_link("/f", "/d/a.js");
    assert_eq!(fs.stat("/d/a.js"), Some(EntryKind::File));
    assert_eq!(fs.stat("/d"), Some(EntryKind::Directory));
    assert_eq!(fs.stat("/"), Some(EntryKind::Directory));
    assert_eq!(fs.stat("/d/nope"), None);
    assert_eq!(fs.read_file("/d/a.js").as_deref(), Some("x"));
    assert_eq!(fs.read_file("/d"), None);
    assert_eq!(fs.read_file("/l/b.js").as_deref(), Some("y"));
    assert_eq!(fs.stat("/l"), Some(EntryKind::Directory));
    assert_eq!(fs.realpath("/l/b.js").as_deref(), Some("/d/lib/b.js"));
    assert_eq!(fs.realpath("/f").as_deref(), Some("/d/a.js"));
    assert_eq!(fs.realpath("/d/./lib/../a.js").as_deref(), Some("/d/a.js"));
    assert_eq!(fs.realpath("/d/nope"), None);
    let looped = MemoryFs::new(&[]).with_link("/x", "/y").with_link("/y", "/x");
    assert_eq!(looped.realpath("/x"), None);
  }

  #[test]
  fn memory_fs_relative_link_targets_resolve_from_the_link_directory() {
    // Como o readlink do Linux: o alvo relativo vale a partir do diretório que contém o link.
    let fs = MemoryFs::new(&[("/real/index.js", "r"), ("/x/y/f.js", "f")])
      .with_link("/app/node_modules/lnk", "../../real")
      .with_link("/app/a/l1", "../b/l2")
      .with_link("/app/b/l2", "../../real")
      .with_link("/top", "real")
      .with_link("/x/z", "./y/");
    assert_eq!(fs.realpath("/app/node_modules/lnk").as_deref(), Some("/real"));
    assert_eq!(fs.realpath("/app/node_modules/lnk/index.js").as_deref(), Some("/real/index.js"));
    assert_eq!(fs.read_file("/app/node_modules/lnk/index.js").as_deref(), Some("r"));
    assert_eq!(fs.stat("/app/node_modules/lnk"), Some(EntryKind::Directory));
    // Encadeado: l1 -> ../b/l2 (em /app/b), l2 -> ../../real (em /app/b).
    assert_eq!(fs.realpath("/app/a/l1").as_deref(), Some("/real"));
    assert_eq!(fs.realpath("/app/a/l1/index.js").as_deref(), Some("/real/index.js"));
    // Link na raiz com alvo relativo simples e com `./` e barra final.
    assert_eq!(fs.realpath("/top/index.js").as_deref(), Some("/real/index.js"));
    assert_eq!(fs.realpath("/x/z/f.js").as_deref(), Some("/x/y/f.js"));
    // Ciclo relativo cai no limite de voltas.
    let looped = MemoryFs::new(&[]).with_link("/d/a", "b").with_link("/d/b", "a");
    assert_eq!(looped.realpath("/d/a"), None);
  }

  #[test]
  fn memory_fs_dotdot_climbs_from_the_resolved_directory() {
    // `link/../x` sobe a partir do alvo de `link`, não do pai textual.
    let fs = MemoryFs::new(&[("/other/branch/x", "alvo"), ("/other/x", "irmao"), ("/d/x", "texto"), ("/x", "raiz")])
      .with_link("/d/link", "/other/branch");
    assert_eq!(fs.realpath("/d/link/../x").as_deref(), Some("/other/x"));
    assert_eq!(fs.read_file("/d/link/../x").as_deref(), Some("irmao"));
    assert_eq!(fs.read_file("/d/link/x").as_deref(), Some("alvo"));
    // `..` a partir da raiz fica na raiz.
    assert_eq!(fs.realpath("/../x").as_deref(), Some("/x"));
    assert_eq!(fs.realpath("/d/../../../x").as_deref(), Some("/x"));
    assert_eq!(fs.realpath("/..").as_deref(), Some("/"));
    // Alvo com `link/..` encadeado: l2 -> /d/link/.. vale /other, e l3 -> l2/branch vale /other/branch.
    let chained = fs.with_link("/d/l2", "/d/link/..").with_link("/d/l3", "l2/branch");
    assert_eq!(chained.realpath("/d/l2").as_deref(), Some("/other"));
    assert_eq!(chained.realpath("/d/l2/x").as_deref(), Some("/other/x"));
    assert_eq!(chained.realpath("/d/l3/x").as_deref(), Some("/other/branch/x"));
    assert_eq!(chained.realpath("/d/l3/../x").as_deref(), Some("/other/x"));
    // Link cujo alvo passa por outro link: `a -> b/..` com `b -> /d/e` sobe de /d/e e dá /d.
    let selfref = MemoryFs::new(&[("/d/f", ""), ("/d/e/g", "")]).with_link("/d/b", "/d/e").with_link("/d/a", "b/..");
    assert_eq!(selfref.realpath("/d/a").as_deref(), Some("/d"));
    assert_eq!(selfref.realpath("/d/a/f").as_deref(), Some("/d/f"));
  }

  #[test]
  fn probe_follows_links_and_ignores_directories_named_like_files() {
    let fs = MemoryFs::new(&[("/d/real/index.js", "")]).with_link("/d/pkg", "/d/real");
    assert_eq!(probe(&fs, "/d/pkg", false).as_deref(), Some("/d/pkg/index.js"));
    let dir_named_js = MemoryFs::new(&[("/d/a.js/x", ""), ("/d/a.ts", "")]);
    assert_eq!(probe(&dir_named_js, "/d/a.js", false).as_deref(), Some("/d/a.ts"));
  }

  #[test]
  fn implicit_extension_order_matches_bun() {
    let all: Vec<String> = IMPLICIT_EXTENSIONS.iter().map(|e| format!("/d/a.{e}")).collect();
    let mut left: Vec<&str> = all.iter().map(String::as_str).collect();
    let mut order = Vec::new();
    while let Some(found) = run(&left, "/d/a", false) {
      left.retain(|f| *f != found);
      order.push(found);
    }
    let expected: Vec<String> = IMPLICIT_EXTENSIONS.iter().map(|e| format!("/d/a.{e}")).collect();
    assert_eq!(order, expected);
  }

  #[test]
  fn index_order_and_file_beats_directory() {
    assert_eq!(run(&["/d/c.js", "/d/c/index.js"], "/d/c", false).as_deref(), Some("/d/c.js"));
    assert_eq!(run(&["/d/c.js", "/d/c/index.js"], "/d/c", true).as_deref(), Some("/d/c/index.js"));
    assert_eq!(run(&["/d/a/index.json", "/d/a/index.mts"], "/d/a", false).as_deref(), Some("/d/a/index.mts"));
    assert_eq!(run(&[], "/d/a", false), None);
  }

  #[test]
  fn js_specifier_rewrites_to_typescript_only() {
    assert_eq!(run(&["/d/b.js", "/d/b.ts"], "/d/b.js", false).as_deref(), Some("/d/b.js"));
    assert_eq!(run(&["/d/b.tsx", "/d/b.ts"], "/d/b.js", false).as_deref(), Some("/d/b.ts"));
    assert_eq!(run(&["/d/b.mts"], "/d/b.mjs", false).as_deref(), Some("/d/b.mts"));
    assert_eq!(run(&["/d/b.cts"], "/d/b.cjs", false).as_deref(), Some("/d/b.cts"));
    assert_eq!(run(&["/d/b.jsx", "/d/b.mjs", "/d/b.cts"], "/d/b.js", false), None);
    assert_eq!(run(&["/d/b.js"], "/d/b.ts", false), None);
  }

  fn dir(files: &[(&str, &str)]) -> Option<String> {
    probe_directory(&MemoryFs::new(files), "/d")
  }

  #[test]
  fn main_uses_its_own_extension_order() {
    let all: Vec<(String, String)> = MAIN_EXTENSIONS.iter().map(|e| (format!("/d/lib/x.{e}"), String::new())).collect();
    let mut files: Vec<(&str, &str)> = all.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
    files.push(("/d/package.json", r#"{"main":"lib/x"}"#));
    files.push(("/d/index.js", ""));
    let mut order = Vec::new();
    while let Some(found) = dir(&files) {
      if found == "/d/index.js" {
        break;
      }
      files.retain(|(k, _)| *k != found);
      order.push(found);
    }
    let expected: Vec<String> = MAIN_EXTENSIONS.iter().map(|e| format!("/d/lib/x.{e}")).collect();
    assert_eq!(order, expected);
    // mjs e mts não são sondados por `main`, só por nome exato.
    let only_mjs = [("/d/package.json", r#"{"main":"lib/x"}"#), ("/d/lib/x.mjs", ""), ("/d/index.js", "")];
    assert_eq!(dir(&only_mjs).as_deref(), Some("/d/index.js"));
    let exact = [("/d/package.json", r#"{"main":"lib/x.mjs"}"#), ("/d/lib/x.mjs", "")];
    assert_eq!(dir(&exact).as_deref(), Some("/d/lib/x.mjs"));
    let rewrite = [("/d/package.json", r#"{"main":"lib/x.mjs"}"#), ("/d/lib/x.mts", "")];
    assert_eq!(dir(&rewrite).as_deref(), Some("/d/lib/x.mts"));
  }

  #[test]
  fn main_falls_back_to_the_directory_index() {
    let idx = ("/d/index.js", "");
    for pkg in [r#"{"main":"nope"}"#, r#"{"main":""}"#, r#"{"main":5}"#, r#"{"main":null}"#, r#"{"main":"."}"#, r#"{"main":"./"}"#,
      r#"{"main":" lib/x.js "}"#, "{main:", "", "[1]", r#"{"Main":"lib/x.js"}"#, r#"{"main":"nope","main":"lib/x.js"}"#,
      r#"{"exports":"./lib/x.js"}"#, r#"{"module":"lib/x.js"}"#] {
      assert_eq!(dir(&[("/d/package.json", pkg), idx, ("/d/lib/x.js", "")]).as_deref(), Some("/d/index.js"), "{pkg}");
    }
    assert_eq!(dir(&[("/d/package.json", "{}")]), None);
    assert_eq!(dir(&[("/d/index.mjs", "")]).as_deref(), Some("/d/index.mjs"));
    // `exports` presente não vence `main` em import relativo.
    let both = [("/d/package.json", r#"{"exports":"./lib/e.js","main":"lib/m.js"}"#), ("/d/lib/e.js", ""), ("/d/lib/m.js", "")];
    assert_eq!(dir(&both).as_deref(), Some("/d/lib/m.js"));
  }

  #[test]
  fn main_directories_slashes_and_paths() {
    let pkg = |m: &str| format!(r#"{{"main":"{m}"}}"#);
    let lib = pkg("lib");
    let files = [("/d/package.json", lib.as_str()), ("/d/lib.js", ""), ("/d/lib/index.js", "")];
    assert_eq!(dir(&files).as_deref(), Some("/d/lib.js"));
    let slash = pkg("lib/");
    let files = [("/d/package.json", slash.as_str()), ("/d/lib.js", ""), ("/d/lib/index.js", "")];
    assert_eq!(dir(&files).as_deref(), Some("/d/lib/index.js"));
    let files = [("/d/package.json", slash.as_str()), ("/d/lib.js", ""), ("/d/index.js", "")];
    assert_eq!(dir(&files).as_deref(), Some("/d/index.js"));
    // `package.json` aninhado no diretório de `main` não é consultado.
    let nested = [("/d/package.json", lib.as_str()), ("/d/lib/package.json", r#"{"main":"n.js"}"#), ("/d/lib/n.js", ""), ("/d/index.js", "")];
    assert_eq!(dir(&nested).as_deref(), Some("/d/index.js"));
    let file_slash = pkg("lib/x.js/");
    assert_eq!(dir(&[("/d/package.json", file_slash.as_str()), ("/d/lib/x.js", ""), ("/d/index.js", "")]).as_deref(), Some("/d/lib/x.js"));
    let rel = pkg("./lib/x.js");
    assert_eq!(dir(&[("/d/package.json", rel.as_str()), ("/d/lib/x.js", "")]).as_deref(), Some("/d/lib/x.js"));
    let up = pkg("../out.js");
    assert_eq!(dir(&[("/d/package.json", up.as_str()), ("/out.js", ""), ("/d/index.js", "")]).as_deref(), Some("/out.js"));
    let abs = pkg("/o/y.js");
    assert_eq!(dir(&[("/d/package.json", abs.as_str()), ("/o/y.js", "")]).as_deref(), Some("/o/y.js"));
    let json = pkg("lib/c.json");
    assert_eq!(dir(&[("/d/package.json", json.as_str()), ("/d/lib/c.json", "")]).as_deref(), Some("/d/lib/c.json"));
  }

  /// A árvore de `scripts/gen-node-modules-resolve-golden.js`, com os mesmos casos medidos no bun 1.4.2.
  fn nm_tree() -> MemoryFs {
    MemoryFs::new(&[
      ("/app/node_modules/a/package.json", r#"{"main":"lib/i.js"}"#),
      ("/app/node_modules/a/index.js", ""),
      ("/app/node_modules/a/sub.js", ""),
      ("/app/node_modules/@s/p/index.js", ""),
      ("/app/node_modules/@s/p/lib/x.js", ""),
      ("/app/node_modules/b/package.json", r#"{"main":"lib/i.js"}"#),
      ("/app/node_modules/b/lib/i.js", ""),
      ("/app/node_modules/b/lib/y.js", ""),
      ("/node_modules/up/index.js", ""),
      ("/node_modules/dirmain/package.json", r#"{"main":"lib/m"}"#),
      ("/node_modules/dirmain/lib/m.js", ""),
      ("/app/node_modules/withexp/package.json", r#"{"exports":"./e.js","main":"m.js"}"#),
      ("/app/node_modules/withexp/e.js", ""),
      ("/app/node_modules/withexp/m.js", ""),
      ("/app/node_modules/sd/lib/package.json", r#"{"main":"x.js"}"#),
      ("/app/node_modules/sd/lib/x.js", ""),
      ("/app/node_modules/sd/lib/index.js", ""),
      ("/app/node_modules/sd/dir/index.js", ""),
      ("/app/node_modules/sd/dir.js", ""),
      ("/node_modules/sd/lib/only_up.js", ""),
      ("/app/node_modules/empty/readme.txt", ""),
      ("/node_modules/empty/index.js", ""),
      ("/app/node_modules/inner/node_modules/deep/index.js", ""),
      ("/app/node_modules/inner/src/i.js", ""),
      ("/node_modules/deep.js", ""),
      ("/node_modules/f.js", ""),
      ("/app/node_modules/@t/q.js", ""),
      ("/app/node_modules/@t/q/index.js", ""),
      ("/app/node_modules/node_modules/nn/index.js", ""),
      ("/app/node_modules/pk/index.js", ""),
      ("/app/node_modules/pk/sub/index.js", ""),
      ("/app/node_modules/pk/node_modules/in/index.js", ""),
      ("/app/node_modules/file.json", ""),
      ("/app/node_modules/sp ace.js", ""),
      ("/real/index.js", ""),
      ("/real/lib/z.js", ""),
    ])
    .with_link("/app/node_modules/lnk", "/real")
  }

  fn nm(importer_dir: &str, spec: &str) -> Option<String> {
    resolve_node_modules(&nm_tree(), importer_dir, spec, true)
  }

  #[test]
  fn node_modules_walks_up_and_resolves_packages() {
    let app = "/app/src";
    for (spec, want) in [
      ("a", "/app/node_modules/a/index.js"), // `main` inexistente cai no index
      ("a/sub", "/app/node_modules/a/sub.js"),
      ("a/sub.js", "/app/node_modules/a/sub.js"),
      ("a/", "/app/node_modules/a/index.js"),
      ("a//sub", "/app/node_modules/a/sub.js"),
      ("a/../b", "/app/node_modules/b/lib/i.js"),
      ("@s/p", "/app/node_modules/@s/p/index.js"),
      ("@s/p/lib/x", "/app/node_modules/@s/p/lib/x.js"),
      ("b", "/app/node_modules/b/lib/i.js"),
      ("b/lib/y", "/app/node_modules/b/lib/y.js"),
      ("up", "/node_modules/up/index.js"),
      ("up/", "/node_modules/up/index.js"),
      ("dirmain", "/node_modules/dirmain/lib/m.js"),
      // `withexp` (golden: `exports` vence `main` em especificador nu) fica para a fatia do campo `exports`.
      ("empty", "/node_modules/empty/index.js"), // diretório sem index não bloqueia
      ("sd/lib", "/app/node_modules/sd/lib/x.js"), // `main` do subdiretório vence o index
      ("sd/lib/", "/app/node_modules/sd/lib/x.js"),
      ("sd/dir", "/app/node_modules/sd/dir.js"), // arquivo vence diretório
      ("sd/dir/", "/app/node_modules/sd/dir/index.js"),
      ("sd/lib/only_up", "/node_modules/sd/lib/only_up.js"), // o subcaminho também sobe
      ("f", "/node_modules/f.js"), // sem noção de pacote: `node_modules/f.js` atende `f`
      ("deep", "/node_modules/deep.js"),
      ("@t/q", "/app/node_modules/@t/q.js"),
      ("pk/.", "/app/node_modules/pk/index.js"),
      ("pk/sub/..", "/app/node_modules/pk/index.js"),
      ("pk\\sub", "/app/node_modules/pk/sub/index.js"),
      ("pk/sub\\", "/app/node_modules/pk/sub/index.js"),
      ("file", "/app/node_modules/file.json"),
      ("sp ace", "/app/node_modules/sp ace.js"),
      ("lnk", "/real/index.js"), // o resultado é o caminho canônico
      ("lnk/lib/z", "/real/lib/z.js"),
    ] {
      assert_eq!(nm(app, spec).as_deref(), Some(want), "{spec}");
    }
    for spec in ["a/nope", "@s", "@s/", "@s/nope", "@nope/x", "nope", "f/x", "nn", "PK", "a/nope/deeper", "nope/sub", "@s/p/zzz"] {
      assert_eq!(nm(app, spec), None, "{spec}");
    }
  }

  #[test]
  fn node_modules_skips_node_modules_directories_in_the_walk() {
    assert_eq!(nm("/app/node_modules/inner/src", "deep").as_deref(), Some("/app/node_modules/inner/node_modules/deep/index.js"));
    assert_eq!(nm("/app/node_modules/inner/src", "inner"), None);
    assert_eq!(nm("/app/node_modules/inner/src", "sd"), None);
    assert_eq!(nm("/app/node_modules/inner/src", "f").as_deref(), Some("/node_modules/f.js"));
    // `app/node_modules/node_modules` não é consultado, e `in` só existe dentro do pacote.
    assert_eq!(nm("/app/node_modules/pk/sub", "nn"), None);
    assert_eq!(nm("/app/node_modules/pk/sub", "in").as_deref(), Some("/app/node_modules/pk/node_modules/in/index.js"));
    assert_eq!(nm("/app/node_modules/pk/sub", "pk").as_deref(), Some("/app/node_modules/pk/index.js"));
  }

  #[test]
  fn node_modules_not_found_messages_match_bun() {
    let imp = "/app/src/t.js";
    assert_eq!(require_not_found_message("a/nope", imp), "Cannot find module 'a/nope'\nRequire stack:\n- /app/src/t.js");
    assert_eq!(import_not_found_message("a/nope", imp), "Cannot find package 'a' imported from /app/src/t.js");
    assert_eq!(import_not_found_message("nope", imp), "Cannot find package 'nope' imported from /app/src/t.js");
    assert_eq!(import_not_found_message("@s/p/lib/zzz", imp), "Cannot find package '@s/p' imported from /app/src/t.js");
    assert_eq!(import_not_found_message("@s/nope", imp), "Cannot find package '@s/nope' imported from /app/src/t.js");
    assert_eq!(import_not_found_message("@s", imp), "Cannot find package '@s' imported from /app/src/t.js");
    assert_eq!(import_not_found_message("@s/", imp), "Cannot find package '@s/' imported from /app/src/t.js");
  }

  #[test]
  fn package_json_is_lenient_like_bun() {
    for pkg in ["\u{feff}{\"main\":\"lib/x.js\"}", r#"{"main":"lib/x.js",}"#, "{\"main\":\"lib/x.js\" // c\n}", r#"{"a":[1,{"b":"}"}],"main":"lib/x.js"}"#] {
      assert_eq!(dir(&[("/d/package.json", pkg), ("/d/lib/x.js", "")]).as_deref(), Some("/d/lib/x.js"), "{pkg}");
    }
  }

  // Medição: `bun x.mjs` com `import 'file://...'` de arquivos reais em /tmp/m/t (o carregador, que é o que
  // `file_url_path` imita), e `Bun.fileURLToPath` onde o texto diz. Valor "" = "Cannot find module ''".
  #[test]
  fn file_url_path_cases() {
    // Bun.fileURLToPath('file:///tmp/a%20b.js') => '/tmp/a b.js'
    assert_eq!(file_url_path("file:///tmp/a%20b.js").as_deref(), Some("/tmp/a b.js"));
    // Bun.fileURLToPath('file://localhost/x/y.js?q=1#h') => '/x/y.js'
    assert_eq!(file_url_path("file://localhost/x/y.js?q=1#h").as_deref(), Some("/x/y.js"));
    // Bun.fileURLToPath('file://') => '/' ; 'file://localhost' => '/'
    assert_eq!(file_url_path("file://").as_deref(), Some("/"));
    assert_eq!(file_url_path("file://localhost").as_deref(), Some("/"));
    // import 'file://host/tmp/m/t/real.js' carrega (Bun.fileURLToPath lançaria ERR_INVALID_FILE_URL_HOST)
    assert_eq!(file_url_path("file://host/tmp/m/t/real.js").as_deref(), Some("/tmp/m/t/real.js"));
    // import 'file:/x' e 'FILE://x' são nome de pacote; 'pkg' idem
    assert_eq!(file_url_path("file:/x"), None);
    assert_eq!(file_url_path("FILE://x"), None);
    assert_eq!(file_url_path("pkg"), None);
  }

  #[test]
  fn file_url_path_edge_cases() {
    // import 'file:///tmp/m/t/a%2Fb.js' => Cannot find module '/tmp/m/t/a/b.js' (o carregador decodifica %2F);
    // Bun.fileURLToPath('file:///a%2Fb') lançaria ERR_INVALID_FILE_URL_PATH, mas o carregador não passa por ela
    assert_eq!(file_url_path("file:///a%2Fb").as_deref(), Some("/a/b"));
    // import 'file:///tmp/m/t/a%252Fb.js' => carrega o arquivo 'a%2Fb.js'
    assert_eq!(file_url_path("file:///a%252Fb").as_deref(), Some("/a%2Fb"));
    // import 'file:///tmp/m/t/m%00n.js' => Cannot find module '/tmp/m/t/m\0n.js'; Bun.fileURLToPath('file:///a%00b') => '/a\0b'
    assert_eq!(file_url_path("file:///a%00b").as_deref(), Some("/a\0b"));
    // Bun.fileURLToPath('file:///a%zzb') => '/a%zzb'; '%2' => '/a%2'; '%' => '/a%'; import '.../o%zz.js' carrega 'o%zz.js'
    assert_eq!(file_url_path("file:///a%zzb").as_deref(), Some("/a%zzb"));
    assert_eq!(file_url_path("file:///a%2").as_deref(), Some("/a%2"));
    assert_eq!(file_url_path("file:///a%").as_deref(), Some("/a%"));
    // Bun.fileURLToPath('file:////a//b') => '//a//b'
    assert_eq!(file_url_path("file:////a//b").as_deref(), Some("//a//b"));
    // import 'file:///tmp/m/t/nonexist/../real.js' => 'real' (resolve `..` sem o diretório existir)
    assert_eq!(file_url_path("file:///t/nonexist/../real.js").as_deref(), Some("/t/real.js"));
    // Bun.fileURLToPath('file:///a/./b') => '/a/b'
    assert_eq!(file_url_path("file:///a/./b").as_deref(), Some("/a/b"));
    // import 'file:///tmp/m/t/nonexist/%2e%2e/real.js' => 'real'
    assert_eq!(file_url_path("file:///t/nonexist/%2e%2E/real.js").as_deref(), Some("/t/real.js"));
    // import 'file:///tmp/m/t/sub/..' => Cannot find module '/tmp/m/t/'
    assert_eq!(file_url_path("file:///t/sub/..").as_deref(), Some("/t/"));
    // `..` não sobe da raiz: Bun.fileURLToPath('file:///../a') => '/a'
    assert_eq!(file_url_path("file:///../a").as_deref(), Some("/a"));
    // Bun.fileURLToPath('file:///%F0%9F%98%80') => '/😀' ; Bun.fileURLToPath('file:///😀') => '/😀'
    assert_eq!(file_url_path("file:///%F0%9F%98%80").as_deref(), Some("/\u{1F600}"));
    assert_eq!(file_url_path("file:///\u{1F600}").as_deref(), Some("/\u{1F600}"));
    // Bun.fileURLToPath('file:///a\\b') => '/a/b' ; 'file:///a%5Cb' => '/a\\b'
    assert_eq!(file_url_path("file:///a\\b").as_deref(), Some("/a/b"));
    assert_eq!(file_url_path("file:///a%5Cb").as_deref(), Some("/a\\b"));
    // Bun.fileURLToPath('file:///a%C3') => '' (UTF-8 inválido) ; import 'file:///tmp/m/t/%C3.js' => Cannot find module ''
    assert_eq!(file_url_path("file:///a%C3").as_deref(), Some(""));
  }

  // Medição: `require('url').pathToFileURL(p).href` e o `import.meta.url` de arquivos reais de mesmo nome
  // (import '/tmp/m/t/<nome>' => export default import.meta.url).
  #[test]
  fn file_url_from_path_cases() {
    // pathToFileURL('/tmp/a b.js').href => 'file:///tmp/a%20b.js'
    assert_eq!(file_url_from_path("/tmp/a b.js"), "file:///tmp/a%20b.js");
    // chave sem barra: módulo em memória, sem medição direta (pathToFileURL('lib/a.js') enraíza no cwd)
    assert_eq!(file_url_from_path("lib/a.js"), "file:///lib/a.js");
    // pathToFileURL('/a#b?c%d').href => 'file:///a%23b%3Fc%25d'
    assert_eq!(file_url_from_path("/a#b?c%d"), "file:///a%23b%3Fc%25d");
    // pathToFileURL('/é').href => 'file:///%C3%A9'
    assert_eq!(file_url_from_path("/é"), "file:///%C3%A9");
  }

  #[test]
  fn file_url_from_path_edge_cases() {
    // pathToFileURL('/a%2Fb').href => 'file:///a%252Fb' ; '/a%00b' => 'file:///a%2500b' ; '/a%zz' => 'file:///a%25zz'
    assert_eq!(file_url_from_path("/a%2Fb"), "file:///a%252Fb");
    assert_eq!(file_url_from_path("/a%00b"), "file:///a%2500b");
    assert_eq!(file_url_from_path("/a%zz"), "file:///a%25zz");
    // pathToFileURL('/\u{1F600}').href => 'file:///%F0%9F%98%80' ; import.meta.url de '😀.js' idem
    assert_eq!(file_url_from_path("/\u{1F600}"), "file:///%F0%9F%98%80");
    // pathToFileURL('/a\\b').href => 'file:///a%5Cb' ; import.meta.url de 'i\\j.js' idem
    assert_eq!(file_url_from_path("/a\\b"), "file:///a%5Cb");
    // pathToFileURL("/x\"<>[]^`{|}~'").href => "file:///x%22%3C%3E%5B%5D%5E%60%7B%7C%7D%7E'" (til escapa, apóstrofo não)
    assert_eq!(file_url_from_path("/x\"<>[]^`{|}~'"), "file:///x%22%3C%3E%5B%5D%5E%60%7B%7C%7D%7E'");
    // pathToFileURL('/a\tb') => 'file:///a%09b' ; '/a\nb' => '%0A' ; '/a\x7fb' => '%7F'
    assert_eq!(file_url_from_path("/a\tb\nc\x7fd"), "file:///a%09b%0Ac%7Fd");
    // pathToFileURL('/a//b') => 'file:///a/b' e '/a/../b' => 'file:///b': o pathToFileURL resolve, a função não;
    // o import.meta.url usa a chave já resolvida (import '/tmp/m/t//a b.js' => 'file:///tmp/m/t/a%20b.js')
    assert_eq!(file_url_from_path("/a//b"), "file:///a//b");
  }

  /// `file_url_path` medido no bun 1.4.2 (`require(spec)` e `import(spec)` mostram o caminho em "Cannot find module").
  #[test]
  fn file_url_path_matches_bun() {
    let cases: [(&str, Option<&str>); 31] = [
      // Só o prefixo `file://` em minúsculas conta; o resto é nome de pacote.
      ("file:/x", None),
      ("FILE://x", None),
      ("File:///x", None),
      ("file:x", None),
      ("/x", None),
      ("file://localhost/x", Some("/x")),
      ("file://LOCALHOST/x", Some("/x")),
      ("file://localhost", Some("/")),
      ("file://", Some("/")),
      ("file://x", Some("/")),
      ("file://x/y", Some("/y")),
      ("file://h%41/y", Some("/y")),
      ("file://[::1]/y", Some("/y")),
      ("file://./y", Some("/y")),
      ("file://x:80/y", Some("")),
      ("file://u:p@h/y", Some("")),
      ("file:///", Some("/")),
      ("file:///x", Some("/x")),
      // Escapes, `.`/`..`, barra invertida, consulta e fragmento.
      ("file:///tmp/a%20b.js", Some("/tmp/a b.js")),
      ("file:///t/nonexist/%2e%2E/real.js", Some("/t/real.js")),
      ("file:///a%2e%2E/b", Some("/a../b")),
      ("file:///a/../b", Some("/b")),
      ("file:///a/./b", Some("/a/b")),
      ("file:///a\\b", Some("/a/b")),
      ("file:///a%2fb", Some("/a/b")),
      ("file:///a%zz", Some("/a%zz")),
      ("file:///%C3%A9", Some("/\u{e9}")),
      ("file:///a?b#c", Some("/a")),
      ("file:///a%3Fb%23c", Some("/a?b#c")),
      ("file:///a//b", Some("/a//b")),
      ("file:///a b", Some("/a b")),
    ];
    for (spec, expected) in cases {
      assert_eq!(file_url_path(spec).as_deref(), expected, "{spec}");
    }
    assert_eq!(file_url_path("file:///a<b>").as_deref(), Some("/a<b>"));
    assert_eq!(file_url_path("file:///a`b{c}").as_deref(), Some("/a`b{c}"));
    // Porta vazia: o bun entrega a autoridade inteira ao caminho (medido).
    assert_eq!(file_url_path("file://x:/y").as_deref(), Some("/x:/y"));
    // Bytes que não formam UTF-8 dão o caminho vazio.
    assert_eq!(file_url_path("file:///a%ffb").as_deref(), Some(""));
    assert_eq!(file_url_path("file:///a%C3").as_deref(), Some(""));
    // Quatro barras: o caminho `//x`.
    assert_eq!(file_url_path("file:////x").as_deref(), Some("//x"));
  }

  /// `file_url_from_path` medido no bun 1.4.2 (`import.meta.url`, `pathToFileURL`, `Bun.pathToFileURL`).
  #[test]
  fn file_url_from_path_matches_bun() {
    let cases = [
      ("/tmp/a b.js", "file:///tmp/a%20b.js"),
      ("lib/a.js", "file:///lib/a.js"),
      ("/a#b?c%d", "file:///a%23b%3Fc%25d"),
      ("/\u{e9}", "file:///%C3%A9"),
      ("/a<b>c", "file:///a%3Cb%3Ec"),
      ("/a`b", "file:///a%60b"),
      ("/a{b}c", "file:///a%7Bb%7Dc"),
      ("/a b", "file:///a%20b"),
      ("/a%b", "file:///a%25b"),
      ("/a#b", "file:///a%23b"),
      ("/a?b", "file:///a%3Fb"),
      ("/a<>`{} %#?z", "file:///a%3C%3E%60%7B%7D%20%25%23%3Fz"),
      ("/a\"b", "file:///a%22b"),
      ("/a[b]^|~\\c", "file:///a%5Bb%5D%5E%7C%7E%5Cc"),
      ("/tab\there", "file:///tab%09here"),
      ("/a%20b", "file:///a%2520b"),
      ("/a'b", "file:///a'b"),
      ("/a;b=c,d&e+f$g@h:i!j(k)*", "file:///a;b=c,d&e+f$g@h:i!j(k)*"),
      ("/tmp/zjsc-probe/sp ace<>`{}%/m.mjs", "file:///tmp/zjsc-probe/sp%20ace%3C%3E%60%7B%7D%25/m.mjs"),
      ("/tmp/zjsc-probe/h#q/m.mjs", "file:///tmp/zjsc-probe/h%23q/m.mjs"),
    ];
    for (path, expected) in cases {
      assert_eq!(file_url_from_path(path), expected, "{path}");
    }
  }
}
