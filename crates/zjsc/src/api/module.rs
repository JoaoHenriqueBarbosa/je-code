//! `evaluate_module`: módulos ES de ponta a ponta (o que `jsc.cpp` faz com `loadAndEvaluateModule`):
//! o ponto de entrada e todo o grafo de `import` passam por um [`ModuleHost`].

use std::collections::HashMap;
use std::rc::Rc;

use crate::api::eval::{describe_exception, evaluate, new_global_object, program_source, VmFinalizer};
use crate::api::fs_module_host::FsModuleHost;
use crate::api::module_probe::{file_url_from_path, ModuleFs};
use crate::parser::source_provider::SourceProviderSourceType;
use crate::runtime::script_fetch_parameters::ScriptFetchParametersType;
use crate::runtime::js_module_loader::{install_module_loader, load_and_evaluate_module, ModuleHost};
use crate::runtime::js_promise::Status as PromiseStatus;
use crate::runtime::js_value::JSValue;
use crate::wtf::text::conversion_mode::ConversionMode;

/// O host do ponto de entrada: serve `source` para a chave `specifier` e delega o resto a `inner`.
struct EntryHost {
    specifier: String,
    source: String,
    /// O mapa do `source`, quando ele é o texto que o transpilador de fonte reimprimiu.
    position_map: Option<Rc<crate::parser::position_map::PositionMap>>,
    inner: Rc<dyn ModuleHost>,
}

impl ModuleHost for EntryHost {
    fn resolve(&self, specifier: &str, referrer: Option<&str>) -> Result<String, String> {
        if referrer.is_none() && specifier == self.specifier {
            return Ok(self.specifier.clone());
        }
        self.inner.resolve(specifier, referrer)
    }

    fn builtin_exports(
        &self,
        global_object: &crate::runtime::js_global_object::JSGlobalObject,
        key: &str,
    ) -> Option<Result<(Vec<crate::runtime::identifier::Identifier>, Vec<JSValue>), crate::runtime::host_call::Thrown>> {
        self.inner.builtin_exports(global_object, key)
    }

    fn fetch(&self, key: &str) -> Result<String, String> {
        if key == self.specifier {
            return Ok(self.source.clone());
        }
        self.inner.fetch(key)
    }

    fn position_map(&self, key: &str) -> Option<Rc<crate::parser::position_map::PositionMap>> {
        if key == self.specifier {
            return self.position_map.clone();
        }
        self.inner.position_map(key)
    }

    fn build_errors(&self, key: &str) -> Option<Vec<crate::api::source_transpiler::BuildDiagnostic>> {
        self.inner.build_errors(key)
    }

    fn commonjs_module(&self, key: &str) -> Option<Rc<crate::api::source_transpiler::Transpiled>> {
        self.inner.commonjs_module(key)
    }

    fn import_meta_url(&self, key: &str) -> String {
        self.inner.import_meta_url(key)
    }

    fn import_meta_resolve(&self, specifier: &str, referrer: &str, sync: bool) -> Result<String, String> {
        self.inner.import_meta_resolve(specifier, referrer, sync)
    }

    fn is_main_module(&self, key: &str) -> bool {
        key == self.specifier
    }

    fn source_type(&self, key: &str, request_type: ScriptFetchParametersType, host_defined_type: &str) -> SourceProviderSourceType {
        self.inner.source_type(key, request_type, host_defined_type)
    }
}

/// Um host de módulos em memória, para o embedder que não tem sistema de arquivos: as chaves são caminhos
/// normalizados (`lib/a.js`, ou `/lib/a.js` quando o referrer é absoluto) e a resolução de especificador
/// relativo (`./`, `../`, `/`) é a do Bun, com as mesmas mensagens de falha: `Cannot find module './x.js'
/// imported from main.js` para caminho e `Cannot find package 'x' imported from main.js` para especificador "nu".
/// Fonte com chave `.json` é JSON mesmo sem o atributo `type`, como no Bun.
pub struct MemoryModuleHost {
    files: HashMap<String, String>,
    reprinted: crate::api::source_transpiler::ReprintedModules,
}

impl MemoryModuleHost {
    pub fn new<K: Into<String>, V: Into<String>>(files: impl IntoIterator<Item = (K, V)>) -> Self {
        Self { files: files.into_iter().map(|(key, source)| (key.into(), source.into())).collect(), reprinted: Default::default() }
    }
}

/// Os segmentos de `path` sem a barra da raiz; `keep_empty` conserva os vazios (`a//b` dá `a`, vazio, `b`),
/// como o `import.meta.resolve` do bun, que nunca colapsa barras duplas. Caminho vazio não tem segmento.
fn path_segments(path: &str, keep_empty: bool) -> impl Iterator<Item = &str> {
    let body = path.strip_prefix('/').unwrap_or(path);
    body.split('/').filter(move |part| !path.is_empty() && (keep_empty || !part.is_empty()))
}

/// Junta `specifier` ao diretório de `referrer` e normaliza os segmentos `.` e `..`. Com `keep_empty` as barras
/// duplas sobrevivem (e `..` também consome um segmento vazio) e o resultado é sempre enraizado em `/`.
fn join_relative(referrer: &str, specifier: &str, keep_empty: bool) -> String {
    let rooted = keep_empty || specifier.starts_with('/') || referrer.starts_with('/');
    let mut segments: Vec<&str> = Vec::new();
    let directory = referrer.rsplit_once('/').map_or("", |(directory, _)| directory);
    let from_directory = if specifier.starts_with('/') { "" } else { directory };
    for part in path_segments(from_directory, keep_empty).chain(path_segments(specifier, keep_empty)) {
        match part {
            "." => {}
            ".." => {
                segments.pop();
            }
            part => segments.push(part),
        }
    }
    // Um `.` ou `..` no fim aponta para um diretório: o bun devolve a barra final (`./s/..` dá `<raiz>/`).
    if keep_empty && matches!(specifier.rsplit('/').next(), Some("." | "..")) {
        segments.push("");
    }
    let joined = segments.join("/");
    if rooted { format!("/{joined}") } else { joined }
}

/// Separa o `?consulta` do especificador (o `#fragmento` fica no nome do arquivo, medido no bun): a chave do módulo os conserva (`./a.mjs?x` é outra instância),
/// o arquivo é o de antes deles.
fn split_suffix(specifier: &str) -> (&str, &str) {
    specifier.find('?').map_or((specifier, ""), |at| specifier.split_at(at))
}

/// Extensões e índices que o bun tenta quando o caminho não existe como está (medido: `./a` acha `a.mjs`, `./sub` acha
/// `sub/index.mjs`, `./A.mjs` acha `a.mjs`).
const PROBE_EXTENSIONS: [&str; 8] = [".tsx", ".jsx", ".ts", ".mjs", ".js", ".cjs", ".json", ".mts"];

impl MemoryModuleHost {
    /// O especificador nu `specifier` achado em `node_modules` a partir do diretório de `from`, como o bun resolve
    /// (`react/jsx-dev-runtime` com `node_modules/react/jsx-dev-runtime.js`; o `exports` do pacote vale): a chave do
    /// arquivo no mapa, ou `None`.
    fn find_in_node_modules(&self, from: &str, specifier: &str) -> Option<String> {
        let rooted = |key: &str| format!("/{}", key.trim_start_matches('/'));
        let files: Vec<(String, &str)> = self.files.iter().map(|(key, text)| (rooted(key), text.as_str())).collect();
        let borrowed: Vec<(&str, &str)> = files.iter().map(|(key, text)| (key.as_str(), *text)).collect();
        let fs = crate::api::module_probe::MemoryFs::new(&borrowed);
        let directory = rooted(from.rsplit_once('/').map_or("", |(directory, _)| directory));
        let found = crate::api::module_probe::resolve_node_modules(&fs, directory.trim_end_matches('/'), specifier, true)?;
        let bare = found.trim_start_matches('/');
        self.files.contains_key(&found).then_some(found.clone()).or_else(|| self.files.contains_key(bare).then(|| bare.to_owned()))
    }

    fn find_file(&self, key: &str) -> Option<String> {
        if self.files.contains_key(key) {
            return Some(key.to_owned());
        }
        let candidates = PROBE_EXTENSIONS
            .iter()
            .map(|extension| format!("{key}{extension}"))
            .chain(PROBE_EXTENSIONS.iter().map(|extension| format!("{key}/index{extension}")));
        for candidate in candidates {
            if self.files.contains_key(&candidate) {
                return Some(candidate);
            }
        }
        let lowered = key.to_lowercase();
        self.files.keys().find(|known| known.to_lowercase() == lowered).cloned()
    }
}

/// `data:[<tipo>][;base64],<dados>` de um módulo: o texto depois da vírgula, sem o escape de porcentagem.
fn data_url_source(specifier: &str) -> Option<String> {
    let (head, body) = specifier.strip_prefix("data:")?.split_once(',')?;
    if head.ends_with(";base64") {
        return None;
    }
    let bytes = body.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        let decoded = (bytes[index] == b'%' && index + 2 < bytes.len() + 0)
            .then(|| u8::from_str_radix(&body[index + 1..index + 3], 16).ok())
            .flatten();
        match decoded {
            Some(byte) => {
                out.push(byte);
                index += 3;
            }
            None => {
                out.push(bytes[index]);
                index += 1;
            }
        }
    }
    String::from_utf8(out).ok()
}

impl ModuleHost for MemoryModuleHost {
    fn resolve(&self, specifier: &str, referrer: Option<&str>) -> Result<String, String> {
        let from = referrer.unwrap_or("");
        if specifier.starts_with("data:") && data_url_source(specifier).is_some() {
            return Ok(specifier.to_owned());
        }
        if let Some(path) = specifier.strip_prefix("file://") {
            let (path, suffix) = split_suffix(path);
            return self.find_file(path).map(|key| format!("{key}{suffix}")).ok_or_else(|| format!("Cannot find module '{path}' imported from {from}"));
        }
        let (path_part, suffix) = split_suffix(specifier);
        let is_path = specifier.starts_with("./") || specifier.starts_with("../") || specifier.starts_with('/') || specifier.starts_with('\\');
        if is_path {
            // `//x` é caminho absoluto do disco no bun (nunca uma chave do mapa) e `\x` nunca existe.
            // O referrer de um script é a URL `file:///caminho`; a chave do mapa é o caminho.
            let key = join_relative(from.strip_prefix("file://").unwrap_or(from), path_part, false);
            if !specifier.starts_with("//") && !specifier.starts_with('\\') {
                if let Some(found) = self.find_file(&key) {
                    return Ok(format!("{found}{suffix}"));
                }
            }
            return Err(format!("Cannot find module '{specifier}' imported from {from}"));
        }
        if self.files.contains_key(specifier) {
            return Ok(specifier.to_owned());
        }
        if specifier.is_empty() {
            return Err(format!("Cannot find module '' imported from {from}"));
        }
        if let Some(key) = self.find_in_node_modules(from, specifier) {
            return Ok(key);
        }
        // O pacote é o primeiro segmento (`@escopo/nome` tem dois): `nopkg/sub` falha em `nopkg`.
        let mut parts = specifier.split('/');
        let first = parts.next().unwrap_or("");
        let package = if first.starts_with('@') { format!("{first}/{}", parts.next().unwrap_or("")) } else { first.to_owned() };
        Err(format!("Cannot find package '{package}' imported from {from}"))
    }

    fn fetch(&self, key: &str) -> Result<String, String> {
        if let Some(source) = data_url_source(key) {
            return Ok(source);
        }
        let (path, _) = split_suffix(key);
        let text = self.files.get(path).cloned().ok_or_else(|| format!("Could not open the module '{key}'."))?;
        Ok(self.reprinted.reprint(key, text))
    }

    fn position_map(&self, key: &str) -> Option<Rc<crate::parser::position_map::PositionMap>> {
        self.reprinted.position_map(key)
    }

    fn build_errors(&self, key: &str) -> Option<Vec<crate::api::source_transpiler::BuildDiagnostic>> {
        self.reprinted.build_errors(key)
    }

    fn commonjs_module(&self, key: &str) -> Option<Rc<crate::api::source_transpiler::Transpiled>> {
        self.reprinted.commonjs_module(key)
    }
}

/// Instala o carregador de módulos do arquivo principal `url` (fonte `source`) no global já montado, como o bun faz
/// com `bun main.js`: os `import` resolvem pelo sistema de arquivos do global, quando há um, e pelos embutidos. Serve
/// ao principal módulo ES e ao `import()` do principal CommonJS, que no bun passa pelo mesmo carregador.
pub(crate) fn install_main_module_loader(
    global_object: &crate::runtime::js_global_object::JSGlobalObjectRef,
    source: &str,
    url: &str,
    position_map: Option<Rc<crate::parser::position_map::PositionMap>>,
) {
    let inner: Rc<dyn ModuleHost> = match global_object.module_fs() {
        Some(fs) => Rc::new(FsModuleHost::new(fs)),
        None => Rc::new(FileUrlHost { inner: MemoryModuleHost::new(Vec::<(String, String)>::new()) }),
    };
    install_module_loader(global_object, Rc::new(EntryHost { specifier: url.to_owned(), source: source.to_owned(), position_map, inner }));
}

/// Começa a avaliar o arquivo principal `url` (fonte `source`) como módulo ES no global já montado, como o bun faz com
/// `bun main.js` quando o arquivo não tem marcador de CommonJS. Devolve a promessa da avaliação; quem chama esvazia as
/// microtarefas.
pub(crate) fn start_main_module(
    global_object: &crate::runtime::js_global_object::JSGlobalObjectRef,
    source: &str,
    url: &str,
    position_map: Option<Rc<crate::parser::position_map::PositionMap>>,
) -> crate::runtime::js_promise::JSPromiseRef {
    install_main_module_loader(global_object, source, url, position_map);
    load_and_evaluate_module(global_object, url)
}

/// Carrega, liga e avalia o módulo `specifier` (cujo fonte é `source`; os módulos que ele importa vêm de
/// `host`), num `VM` e `JSGlobalObject` recém-criados. Devolve o namespace do módulo, ou a exceção como
/// `Err` (`SyntaxError` de parser, de ligação, ou o que o corpo lançou, inclusive depois de um `await`
/// de topo). O `drainMicrotasks` roda antes de olhar o resultado, como em `jsc.cpp`; um módulo cujo
/// top-level await nunca se resolve devolve `Err(undefined)` (não há valor de erro a devolver).
pub fn evaluate_module(source: &str, specifier: &str, host: Rc<dyn ModuleHost>) -> Result<JSValue, JSValue> {
    crate::runtime::cell_registry::run_program(|| {
        let (vm, global_object) = new_global_object();
        let _finalizer = VmFinalizer::new(&vm);
        install_module_loader(
            &global_object,
            Rc::new(EntryHost { specifier: specifier.to_owned(), source: source.to_owned(), position_map: None, inner: host }),
        );
        let promise = load_and_evaluate_module(&global_object, specifier);
        vm.drain_microtasks();
        match promise.status() {
            PromiseStatus::Fulfilled => Ok(promise.result()),
            PromiseStatus::Rejected => {
                promise.mark_as_handled();
                Err(promise.result())
            }
            PromiseStatus::Pending => Err(JSValue::undefined()),
        }
    })
}

/// O resultado de [`evaluate_module_map`]: o `JSON.stringify` do array global `log` e a mensagem do erro
/// que rejeitou a avaliação (`Nome: mensagem`), ou `None` se o módulo de entrada terminou sem falha.
pub struct ModuleMapOutcome {
    pub log_json: String,
    pub error: Option<String>,
}

/// O host de `evaluate_module_map`: arquivos em memória, com `import.meta.url` no formato `file:///chave`
/// (o que os goldens normalizam a partir do caminho do diretório temporário do bun).
struct FileUrlHost {
    inner: MemoryModuleHost,
}

impl ModuleHost for FileUrlHost {
    /// Os embutidos (`node:path`, `path`) resolvem sempre, como no bun, mesmo sem sistema de arquivos.
    fn resolve(&self, specifier: &str, referrer: Option<&str>) -> Result<String, String> {
        if let Some(resolved) = crate::api::builtin_modules::resolve_import(specifier) {
            return resolved;
        }
        self.inner.resolve(specifier, referrer)
    }

    fn builtin_exports(
        &self,
        global_object: &crate::runtime::js_global_object::JSGlobalObject,
        key: &str,
    ) -> Option<Result<(Vec<crate::runtime::identifier::Identifier>, Vec<JSValue>), crate::runtime::host_call::Thrown>> {
        crate::api::builtin_modules::import_exports(global_object, key)
    }

    fn fetch(&self, key: &str) -> Result<String, String> {
        self.inner.fetch(key)
    }

    fn position_map(&self, key: &str) -> Option<Rc<crate::parser::position_map::PositionMap>> {
        self.inner.position_map(key)
    }

    fn build_errors(&self, key: &str) -> Option<Vec<crate::api::source_transpiler::BuildDiagnostic>> {
        self.inner.build_errors(key)
    }

    fn commonjs_module(&self, key: &str) -> Option<Rc<crate::api::source_transpiler::Transpiled>> {
        self.inner.commonjs_module(key)
    }

    fn import_meta_url(&self, key: &str) -> String {
        file_url_from_path(key)
    }

    /// Como o Bun: `node:x` e `file:` ficam como estão, e caminho (`./`, `../`, `/`) vira URL sem conferir
    /// a existência do arquivo; `resolveSync` confere (e devolve o caminho absoluto), e devolve o módulo embutido que
    /// o resolvedor do bun conhece (`known_builtin_specifier`).
    fn import_meta_resolve(&self, specifier: &str, referrer: &str, sync: bool) -> Result<String, String> {
        if !sync && (specifier.starts_with("node:") || specifier.starts_with("file:")) {
            return Ok(specifier.to_owned());
        }
        if let Some(resolved) = crate::api::builtin_modules::known_builtin_specifier(specifier).filter(|_| sync) {
            return Ok(resolved);
        }
        // `//x/p`: o bun lê `x` como host. `resolve` dá `file://x/p` (o caminho normalizado como absoluto, sem
        // `..` que consuma o host); `resolveSync` devolve o especificador como veio, sem conferir existência.
        if let Some(after) = specifier.strip_prefix("//") {
            if sync {
                return Ok(specifier.to_owned());
            }
            let (host, remainder) = after.split_once('/').unwrap_or((after, ""));
            return Ok(format!("file://{host}{}", join_relative("", &format!("/{remainder}"), true)));
        }
        let is_path = specifier.starts_with("./") || specifier.starts_with("../") || specifier.starts_with('/');
        if sync || !is_path {
            let key = self.resolve(specifier, Some(referrer))?;
            let path = format!("/{}", key.trim_start_matches('/'));
            return Ok(if sync { path } else { format!("file://{path}") });
        }
        let path = join_relative(referrer, specifier, true);
        Ok(format!("file://{path}"))
    }

    fn source_type(&self, key: &str, request_type: ScriptFetchParametersType, host_defined_type: &str) -> SourceProviderSourceType {
        self.inner.source_type(key, request_type, host_defined_type)
    }
}

/// Instala em `global_object` o carregador de módulos sobre os arquivos em memória `files` (chave, fonte): o `import()` de um script
/// e o `import` de um módulo resolvem contra eles, como em [`evaluate_module_map`].
pub(crate) fn install_memory_modules(global_object: &crate::runtime::js_global_object::JSGlobalObjectRef, files: &[(String, String)]) {
    install_module_loader(global_object, Rc::new(FileUrlHost { inner: MemoryModuleHost::new(files.iter().cloned()) }));
}

/// Quando o `log` é lido, como em cada gerador de golden: na continuação do `await import(main)`
/// (`gen-module-more-golden.js`, `gen-module-edge-golden.js`) ou depois de um `setTimeout(0)` a mais
/// (`gen-module-golden.js`). O que o laço de eventos rodar depois disso não entra no `log`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LogPoint {
    EntrySettled,
    NextTimeout,
}

/// Avalia o mapa de arquivos `files` (chave, fonte) a partir do módulo `entry` (por exemplo `main.mjs`) num
/// realm novo, depois de definir os globais `log` (array) e `L(x)` (que empurra `String(x)` em `log`).
/// Roda o laço de eventos até o ponto de leitura `log_point` e devolve o `log` serializado em JSON e o erro que
/// rejeitou a avaliação, se houve. Um módulo que nunca termina (top-level await pendente) devolve o erro `pending`.
pub fn evaluate_module_map_until(files: &[(String, String)], entry: &str, log_point: LogPoint) -> ModuleMapOutcome {
    let host = FileUrlHost { inner: MemoryModuleHost::new(files.iter().cloned()) };
    evaluate_logged_module(Rc::new(host), &format!("./{entry}"), log_point, None, "")
}

/// [`evaluate_module_map`] com um script global que roda antes do grafo, no mesmo realm (`global_script` vê `log` e `L`).
pub fn evaluate_module_map_after_script(files: &[(String, String)], entry: &str, global_script: &str) -> ModuleMapOutcome {
    let host = FileUrlHost { inner: MemoryModuleHost::new(files.iter().cloned()) };
    evaluate_logged_module(Rc::new(host), &format!("./{entry}"), LogPoint::EntrySettled, None, global_script)
}

/// [`evaluate_module_map_until`] com o `log` lido quando a entrada assenta.
pub fn evaluate_module_map(files: &[(String, String)], entry: &str) -> ModuleMapOutcome {
    evaluate_module_map_until(files, entry, LogPoint::EntrySettled)
}

/// Como [`evaluate_module_map`], mas o grafo vem de um [`ModuleFs`] (`import` estático e `import()` dinâmico
/// resolvem como o Bun, com a sonda de arquivo e `node_modules`). `source` é o fonte do módulo de entrada,
/// de chave `entry_key` (caminho absoluto, ex. `/app/main.mjs`).
pub fn evaluate_module_with_fs(fs: Rc<dyn ModuleFs>, source: &str, entry_key: &str) -> ModuleMapOutcome {
    let host = EntryHost { specifier: entry_key.to_owned(), source: source.to_owned(), position_map: None, inner: Rc::new(FsModuleHost::new(fs.clone())) };
    evaluate_logged_module(Rc::new(host), entry_key, LogPoint::EntrySettled, Some(fs), "")
}

/// O erro como os geradores dos goldens de módulo o escrevem: `Nome: mensagem` para um `Error` (inclusive o
/// `ResolveMessage` e o `BuildMessage`, que o bun faz herdar de `Error`), `throw <String(valor)>` para o resto.
fn describe_rejection(global_object: &crate::runtime::js_global_object::JSGlobalObjectRef, reason: JSValue) -> String {
    let formatter = "(function (e) { return e instanceof Error ? e.name + ': ' + e.message : 'throw ' + String(e); })";
    let text = evaluate(global_object, &program_source(formatter)).ok().and_then(|function| {
        crate::runtime::call_data::call_with_error_message(global_object, function, JSValue::undefined(), &[reason], "formatador do erro").ok()
    });
    match text {
        Some(value) if value.is_string() => {
            String::from_utf8_lossy(&value.as_js_string().value().utf8(ConversionMode::LenientConversion)).into_owned()
        }
        _ => {
            global_object.vm().clear_exception();
            describe_exception(&reason)
        }
    }
}

/// O `log` serializado em JSON, ou `"null"`.
fn logged(global_object: &crate::runtime::js_global_object::JSGlobalObjectRef) -> String {
    match evaluate(global_object, &program_source("JSON.stringify(log)")) {
        Ok(value) if value.is_string() => {
            let bytes = value.as_js_string().value().utf8(ConversionMode::LenientConversion);
            String::from_utf8_lossy(&bytes).into_owned()
        }
        _ => "null".to_owned(),
    }
}

thread_local! {
    /// O global do programa em curso, o ponto de leitura do `log` e o `log` lido nele.
    static SETTLED_LOG: std::cell::RefCell<(Option<crate::runtime::js_global_object::JSGlobalObjectRef>, LogPoint, Option<String>)> =
        const { std::cell::RefCell::new((None, LogPoint::EntrySettled, None)) };
}

/// Lê o `log` e para o laço de eventos: o que ainda estiver na fila não entra mais no `log`.
fn take_log_and_halt() {
    let global_object = SETTLED_LOG.with(|slot| slot.borrow().0.clone());
    if let Some(global_object) = global_object {
        let text = logged(&global_object);
        SETTLED_LOG.with(|slot| slot.borrow_mut().2 = Some(text));
    }
    crate::runtime::timers::halt_event_loop();
}

/// A avaliação do módulo de entrada assentou: lê o `log` agora, ou depois de um `setTimeout(0)`.
fn entry_settled(global_object: &crate::runtime::js_global_object::JSGlobalObject) {
    match SETTLED_LOG.with(|slot| slot.borrow().1) {
        LogPoint::EntrySettled => take_log_and_halt(),
        LogPoint::NextTimeout => crate::runtime::timers::schedule_native(global_object, 0.0, Vec::new(), Rc::new(|_| take_log_and_halt())),
    }
}

fn entry_settled_reaction_body(global_object: &crate::runtime::js_global_object::JSGlobalObject, _call: &crate::runtime::host_call::HostCall) -> crate::runtime::host_call::HostResult {
    entry_settled(global_object);
    Ok(JSValue::undefined())
}
crate::host_function!(entry_settled_reaction, entry_settled_reaction_body);

fn evaluate_logged_module(host: Rc<dyn ModuleHost>, entry_specifier: &str, log_point: LogPoint, fs: Option<Rc<dyn ModuleFs>>, global_script: &str) -> ModuleMapOutcome {
    crate::runtime::cell_registry::run_program(|| {
        let (vm, global_object) = new_global_object();
        // O `require` de um CommonJS que o grafo importa lê por este sistema de arquivos.
        global_object.set_module_fs(fs);
        let _finalizer = VmFinalizer::new(&vm);
        install_module_loader(&global_object, host);
        // `evaluate` devolve `Err` se o prelúdio falhar, o que seria bug do porte, não do caso.
        let prelude = "globalThis.log = []; globalThis.L = (x) => { log.push(String(x)); };";
        let _ = evaluate(&global_object, &program_source(prelude));
        // O script global que roda antes do grafo (o harness do test262, que os módulos importados também enxergam).
        if !global_script.is_empty() {
            let _ = evaluate(&global_object, &program_source(global_script));
        }
        // Como no bun, o `import()` de um módulo novo é uma leitura entregue pelo laço de eventos (uma macrotask):
        // o laço roda até o ponto de leitura do `log`.
        crate::runtime::timers::enable_event_loop();
        SETTLED_LOG.with(|slot| *slot.borrow_mut() = (Some(global_object.clone()), log_point, None));
        let promise = load_and_evaluate_module(&global_object, entry_specifier);
        vm.drain_microtasks();
        if promise.status() == PromiseStatus::Pending {
            let reaction = crate::runtime::timers::native(&global_object, "", 1, entry_settled_reaction);
            promise.perform_promise_then(&*global_object, reaction, reaction, JSValue::undefined());
            crate::runtime::timers::run_event_loop_until_held(&global_object);
        } else if log_point == LogPoint::NextTimeout {
            entry_settled(&global_object);
            crate::runtime::timers::run_event_loop_until_held(&global_object);
        }
        let error = match promise.status() {
            PromiseStatus::Fulfilled => None,
            PromiseStatus::Rejected => {
                promise.mark_as_handled();
                Some(describe_rejection(&global_object, promise.result()))
            }
            PromiseStatus::Pending => Some("pending".to_owned()),
        };
        let settled = SETTLED_LOG.with(|slot| std::mem::take(&mut slot.borrow_mut().2));
        SETTLED_LOG.with(|slot| slot.borrow_mut().0 = None);
        let log_json = settled.unwrap_or_else(|| logged(&global_object));
        ModuleMapOutcome { log_json, error }
    })
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::collections::HashMap;

    use super::*;

    /// Um host de arquivos em memória; `resolve` devolve o especificador e anota o referrer.
    struct MapHost {
        files: HashMap<&'static str, &'static str>,
        referrers: RefCell<Vec<(String, Option<String>)>>,
    }

    impl ModuleHost for MapHost {
        fn resolve(&self, specifier: &str, referrer: Option<&str>) -> Result<String, String> {
            self.referrers.borrow_mut().push((specifier.to_owned(), referrer.map(str::to_owned)));
            Ok(specifier.to_owned())
        }

        fn fetch(&self, key: &str) -> Result<String, String> {
            self.files.get(key).map(|source| (*source).to_owned()).ok_or_else(|| format!("Could not open the module '{key}'."))
        }
    }

    fn run(entry: &str, files: &[(&'static str, &'static str)]) -> (Result<JSValue, JSValue>, Rc<MapHost>) {
        let host = Rc::new(MapHost { files: files.iter().copied().collect(), referrers: RefCell::new(Vec::new()) });
        let result = evaluate_module(entry, "main.js", Rc::clone(&host) as Rc<dyn ModuleHost>);
        (result, host)
    }

    fn assert_thrown_int(result: Result<JSValue, JSValue>, expected: i32) {
        match result {
            Err(value) => assert!(value.is_int32() && value.as_int32() == expected, "lançou outro valor"),
            Ok(_) => panic!("o módulo devia lançar {expected}"),
        }
    }

    #[test]
    fn top_level_await_resumes_the_module_body() {
        let (result, _) = run("await Promise.resolve(0); throw 7;", &[]);
        assert_thrown_int(result, 7);
    }

    #[test]
    fn top_level_await_before_an_export_const_keeps_the_export_readable() {
        let (result, _) = run(
            "import { x } from './dep.js'; if (x !== 2) throw 1;",
            &[("./dep.js", "await 1; export const x = 2;")],
        );
        assert!(result.is_ok());
        let (result, _) = run("await 1; export const x = 2;", &[]);
        assert!(result.is_ok());
    }

    #[test]
    fn async_dependency_runs_before_its_importer() {
        let (result, _) = run(
            "import './dep.js'; if (globalThis.order !== 1) throw 1;",
            &[("./dep.js", "await null; globalThis.order = 1;")],
        );
        assert!(result.is_ok());
    }

    #[test]
    fn async_dependency_error_rejects_the_importer() {
        let (result, _) = run("import './dep.js'; throw 1;", &[("./dep.js", "await null; throw 9;")]);
        assert_thrown_int(result, 9);
    }

    #[test]
    fn json_module_exports_the_parsed_value_as_default() {
        let (result, _) = run(
            "import data from './a.json' with { type: 'json' }; if (data.x !== 1) throw 2;",
            &[("./a.json", "{\"x\":1}")],
        );
        assert!(result.is_ok());
    }

    #[test]
    fn text_module_exports_the_source_as_default() {
        let (result, _) = run(
            "import text from './a.txt' with { type: 'text' }; if (text !== 'hi') throw 3;",
            &[("./a.txt", "hi")],
        );
        assert!(result.is_ok());
    }

    #[test]
    fn dynamic_import_inside_a_function_and_after_await_resolves_against_the_module() {
        let (result, host) = run(
            "function load() { return import('./dep.js'); } await load(); await null; await import('./dep.js');",
            &[("./dep.js", "export const x = 1;")],
        );
        assert!(result.is_ok());
        let referrers = host.referrers.borrow();
        let dynamic: Vec<_> = referrers.iter().filter(|(specifier, _)| specifier == "./dep.js").collect();
        assert!(dynamic.iter().all(|(_, referrer)| referrer.as_deref() == Some("main.js")));
    }

    #[test]
    fn dynamic_import_with_json_type_attribute() {
        let (result, _) = run(
            "const ns = await import('./a.json', { with: { type: 'json' } }); if (ns.default.x !== 1) throw 2;",
            &[("./a.json", "{\"x\":1}")],
        );
        assert!(result.is_ok());
    }

    #[test]
    fn import_defer_evaluates_on_first_namespace_access() {
        let (result, _) = run(
            "import defer * as ns from './dep.js'; if (globalThis.ran) throw 1; ns.x; if (!globalThis.ran) throw 2;",
            &[("./dep.js", "globalThis.ran = true; export const x = 1;")],
        );
        assert!(result.is_ok());
    }

    #[test]
    fn export_with_an_index_name_is_readable_through_the_namespace() {
        let (result, _) = run(
            "import * as ns from './dep.js'; if (ns[0] !== 5 || ns['0'] !== 5 || !(0 in ns)) throw 1;",
            &[("./dep.js", "const a = 5; export { a as \"0\" };")],
        );
        assert!(result.is_ok());
    }

    /// Roda `entry` como `main.js` sobre um [`MemoryModuleHost`] com `files`.
    fn run_memory(entry: &str, files: &[(&str, &str)]) -> Result<JSValue, JSValue> {
        evaluate_module(entry, "main.js", Rc::new(MemoryModuleHost::new(files.iter().copied())))
    }

    #[test]
    fn default_and_named_imports_resolve_relative_to_each_importer() {
        let result = run_memory(
            "import x, { y } from './lib/a.js'; if (x !== 1 || y !== 2) throw 1;",
            &[
                ("lib/a.js", "import { y } from '../b.js'; export default 1; export { y };"),
                ("b.js", "export const y = 2;"),
            ],
        );
        assert!(result.is_ok());
    }

    #[test]
    fn dependencies_run_once_in_post_order() {
        let result = run_memory(
            "import './a.js'; import './b.js'; if (globalThis.log !== 'cab') throw 1;",
            &[
                ("a.js", "import './c.js'; globalThis.log += 'a';"),
                ("b.js", "import './c.js'; globalThis.log += 'b';"),
                ("c.js", "globalThis.log = 'c';"),
            ],
        );
        assert!(result.is_ok());
    }

    #[test]
    fn export_star_reexports_everything_but_default() {
        let result = run_memory(
            "import { a, b } from './re.js'; import * as ns from './re.js'; \
             if (a !== 1 || b !== 2 || 'default' in ns) throw 1; \
             if (Object.keys(ns).join() !== 'a,b,q') throw 2; if (ns.q.a !== 1) throw 3;",
            &[
                ("re.js", "export * from './dep.js'; export * as q from './dep.js';"),
                ("dep.js", "export const a = 1, b = 2; export default 3;"),
            ],
        );
        assert!(result.is_ok());
    }

    #[test]
    fn export_star_conflict_is_ambiguous_only_when_imported() {
        let files = [
            ("re.js", "export * from './one.js'; export * from './two.js'; export const only = 1;"),
            ("one.js", "export const x = 1; export const same = 0;"),
            ("two.js", "export const x = 2; export { same } from './one.js';"),
        ];
        let result = run_memory("import { only, same } from './re.js'; if (only !== 1 || same !== 0) throw 1;", &files);
        assert!(result.is_ok());
        let result = run_memory(
            r#"try { await import('./use.js'); throw 1; } catch (e) {
                 if (!(e instanceof SyntaxError)) throw 2;
                 if (e.message !== "Export named 'x' cannot be resolved due to ambiguous multiple bindings in module 're.js'.") throw 3;
               }"#,
            &[files[0], files[1], files[2], ("use.js", "import { x } from './re.js';")],
        );
        assert!(result.is_ok());
    }

    #[test]
    fn missing_export_is_a_syntax_error_with_the_bun_message() {
        let result = run_memory(
            r#"try { await import('./use.js'); throw 1; } catch (e) {
                 if (!(e instanceof SyntaxError)) throw 2;
                 if (e.message !== "Export named 'zz' not found in module 'dep.js'.") throw 3;
               }"#,
            &[("use.js", "import { zz } from './dep.js';"), ("dep.js", "export const a = 1;")],
        );
        assert!(result.is_ok());
    }

    #[test]
    fn cycle_shares_hoisted_functions_and_bindings() {
        let result = run_memory(
            "import { fa } from './a.js'; if (fa() !== 'b:a') throw 1; if (globalThis.seen !== 'function') throw 2;",
            &[
                ("a.js", "import { fb } from './b.js'; export function fa() { return fb() + ':a'; }"),
                ("b.js", "import { fa } from './a.js'; globalThis.seen = typeof fa; export function fb() { return 'b'; }"),
            ],
        );
        assert!(result.is_ok());
    }

    #[test]
    fn cycle_reading_an_uninitialized_binding_throws_reference_error() {
        let result = run_memory(
            "try { await import('./a.js'); throw 1; } catch (e) { if (!(e instanceof ReferenceError)) throw 2; }",
            &[("a.js", "import './b.js'; export const x = 1;"), ("b.js", "import { x } from './a.js'; x;")],
        );
        assert!(result.is_ok());
    }

    #[test]
    fn await_import_returns_the_namespace_and_reuses_the_instance() {
        let result = run_memory(
            "const ns = await import('./dep.js'); const again = await import('./dep.js'); \
             if (ns !== again || ns.default !== 4 || ns.n !== 1 || globalThis.count !== 1) throw 1; \
             if (Object.prototype.toString.call(ns) !== '[object Module]') throw 2;",
            &[("dep.js", "globalThis.count = (globalThis.count ?? 0) + 1; export default 4; export const n = 1;")],
        );
        assert!(result.is_ok());
    }

    #[test]
    fn dynamic_import_rejects_with_the_resolution_message() {
        let result = run_memory(
            r#"try { await import('./nope.js'); throw 1; } catch (e) {
                 if (e.message !== "Cannot find module './nope.js' imported from main.js") throw 2;
                 if (e.name !== "ResolveMessage" || e.constructor.name !== "ResolveMessage" || e.code !== "ERR_MODULE_NOT_FOUND") throw 4;
               }
               try { await import('left-pad'); throw 1; } catch (e) {
                 if (e.message !== "Cannot find package 'left-pad' imported from main.js") throw 3;
               }"#,
            &[],
        );
        assert!(result.is_ok());
    }

    #[test]
    fn static_import_of_a_missing_module_rejects_the_entry() {
        assert!(run_memory("import './nope.js';", &[]).is_err());
        assert!(run_memory("import x from 'left-pad';", &[]).is_err());
    }

    #[test]
    fn syntax_error_in_a_dependency_rejects_with_syntax_error() {
        let result = run_memory(
            "try { await import('./bad.js'); throw 1; } catch (e) { if (!(e instanceof SyntaxError)) throw 2; }",
            &[("bad.js", "export const = ;")],
        );
        assert!(result.is_ok());
    }

    #[test]
    fn import_meta_main_is_true_only_for_the_entry_and_the_extras_stay_hidden() {
        let result = run_memory(
            "import { main } from './dep.js'; if (import.meta.main !== true || main !== false) throw 1; \
             if (Object.keys(import.meta).length !== 0 || JSON.stringify(import.meta) !== '{}') throw 2;",
            &[("dep.js", "export const main = import.meta.main;")],
        );
        assert!(result.is_ok());
    }

    #[test]
    fn import_meta_url_is_the_module_key() {
        let result = run_memory(
            "import { url } from './dep.js'; if (import.meta.url !== 'main.js' || url !== 'dep.js') throw 1;",
            &[("dep.js", "export const url = import.meta.url;")],
        );
        assert!(result.is_ok());
    }

    #[test]
    fn file_url_host_escapes_like_the_fs_host() {
        let host = FileUrlHost { inner: MemoryModuleHost::new::<&str, &str>([]) };
        assert_eq!(host.import_meta_url("lib/a b%#.js"), "file:///lib/a%20b%25%23.js");
        assert_eq!(host.import_meta_url("/lib/ñ.js"), "file:///lib/%C3%B1.js");
    }

    #[test]
    fn json_extension_is_a_json_module_and_an_empty_type_attribute_is_a_type_error() {
        let result = run_memory(
            "import data from './a.json'; if (data.x !== 1) throw 1; \
             try { await import('./dep.js', { with: { type: '' } }); throw 2; } catch (e) { if (!(e instanceof TypeError)) throw 3; }",
            &[("a.json", "{\"x\":1}"), ("dep.js", "export {};")],
        );
        assert!(result.is_ok());
    }
}
