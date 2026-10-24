//! Módulos ES do navegador (HTML, "fetch a module script graph"): só URLs. O especificador resolve contra a URL do
//! módulo que importa (ou, no `import()` de um script clássico, contra a URL do documento); especificador nu sem
//! import map, rede que falha e cada URL como registro único (o mapa do carregador) seguem o Chromium. Nada da
//! resolução do Node ou do Bun (`node_modules`, extensões implícitas, `index.js`, `node:`).

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use crate::parser::source_provider::SourceProviderSourceType;
use crate::runtime::js_module_loader::{install_module_loader, load_and_evaluate_module, rust_string, ModuleHost};
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_promise::JSPromiseRef;
use crate::runtime::script_fetch_parameters::ScriptFetchParametersType;

/// O corpo já decodificado do recurso na URL absoluta, ou `None` (erro de rede).
pub type ModuleFetch = Rc<dyn Fn(&str) -> Option<String>>;

/// O host de módulos da página.
pub struct UrlModuleHost {
    fetch: ModuleFetch,
    /// A URL base do documento: referrer do `import()` feito por script clássico.
    document_url: String,
    /// Os fontes de `<script type=module>` inline, servidos pela chave (a URL do documento com um sufixo único).
    inline: RefCell<Vec<(String, String)>>,
    /// A ida e volta da rede do `import()` (ms de relógio virtual); `None` busca na hora.
    network_latency_ms: Cell<Option<f64>>,
}

impl UrlModuleHost {
    pub fn new(document_url: &str, fetch: ModuleFetch) -> Self {
        UrlModuleHost { fetch, document_url: document_url.to_owned(), inline: RefCell::new(Vec::new()), network_latency_ms: Cell::new(None) }
    }

    /// A latência da rede: o `import()` de módulo novo busca e liga numa tarefa de timer `milliseconds` depois.
    pub fn set_network_latency_ms(&self, milliseconds: f64) {
        self.network_latency_ms.set(Some(milliseconds));
    }

    /// O especificador que o Chromium resolve: absoluto (com esquema) ou começando por `/`, `./` ou `../`. A mensagem
    /// da falha depende de quem resolve (medido no Chromium 154).
    fn resolve_against(&self, specifier: &str, base: &str, by: Resolver) -> Result<String, String> {
        let units = |text: &str| text.encode_utf16().collect::<Vec<u16>>();
        let relative = specifier.starts_with('/') || specifier.starts_with("./") || specifier.starts_with("../");
        let parsed = if relative {
            crate::runtime::url::parse_with_base(&units(specifier), Some(&units(base)))
        } else {
            crate::runtime::url::parse_with_base(&units(specifier), None)
        };
        const RELATIVE: &str = "Relative references must start with either \"/\", \"./\", or \"../\".";
        parsed.map(|url| rust_string(url.string())).ok_or_else(|| match by {
            Resolver::Static => format!("Failed to resolve module specifier \"{specifier}\". {RELATIVE}"),
            Resolver::Dynamic => format!("Failed to resolve module specifier '{specifier}'"),
            Resolver::ImportMeta => format!("Failed to resolve module specifier {specifier}: {RELATIVE}"),
        })
    }

    fn resolve_with(&self, specifier: &str, referrer: Option<&str>, by: Resolver) -> Result<String, String> {
        if referrer.is_none() && self.inline.borrow().iter().any(|(key, _)| key == specifier) {
            return Ok(specifier.to_owned());
        }
        // A origem de um script clássico é `file:///<url do script>` (o nome do programa): a base é a URL dele. Sem
        // referrer (código sem origem), a do documento.
        let base = match referrer {
            Some(referrer) => referrer.strip_prefix("file:///").filter(|url| url.contains("://")).unwrap_or(referrer),
            None => &self.document_url,
        };
        self.resolve_against(specifier, shown_url(base), by)
    }
}

/// Quem pede a resolução: o `import` estático, o `import()` e o `import.meta.resolve` falham com textos diferentes.
#[derive(Clone, Copy)]
enum Resolver {
    Static,
    Dynamic,
    ImportMeta,
}

/// A chave de um módulo inline é `<url do documento>#__inline_module_<n>`: a parte antes do marcador é a URL que o
/// navegador mostra (`import.meta.url` do inline é a do documento).
const INLINE_MARK: &str = "#__inline_module_";

fn shown_url(key: &str) -> &str {
    key.split_once(INLINE_MARK).map_or(key, |(url, _)| url)
}

impl ModuleHost for UrlModuleHost {
    fn resolve(&self, specifier: &str, referrer: Option<&str>) -> Result<String, String> {
        self.resolve_with(specifier, referrer, Resolver::Static)
    }

    fn resolve_dynamic(&self, specifier: &str, referrer: Option<&str>) -> Result<String, String> {
        self.resolve_with(specifier, referrer, Resolver::Dynamic)
    }

    fn network_latency_ms(&self) -> Option<f64> {
        self.network_latency_ms.get()
    }

    fn fetch(&self, key: &str) -> Result<String, String> {
        if let Some((_, source)) = self.inline.borrow().iter().find(|(inline_key, _)| inline_key == key) {
            return Ok(source.clone());
        }
        (self.fetch)(key).ok_or_else(|| format!("Failed to fetch dynamically imported module: {key}"))
    }

    fn failures_are_type_errors(&self) -> bool {
        true
    }

    fn json_named_exports(&self) -> bool {
        false
    }

    fn html_import_meta(&self) -> bool {
        true
    }

    fn import_meta_url(&self, key: &str) -> String {
        shown_url(key).to_owned()
    }

    fn import_meta_resolve(&self, specifier: &str, referrer: &str, _sync: bool) -> Result<String, String> {
        self.resolve_against(specifier, shown_url(referrer), Resolver::ImportMeta)
    }

    /// No navegador o tipo vem do `type` do pedido (JSON com `with { type: 'json' }`), nunca da extensão.
    fn source_type(&self, _key: &str, request_type: ScriptFetchParametersType, _host_defined_type: &str) -> SourceProviderSourceType {
        match request_type {
            ScriptFetchParametersType::JSON => SourceProviderSourceType::JSON,
            _ => SourceProviderSourceType::Module,
        }
    }
}

/// Instala o carregador de URLs no global da página.
pub fn install_url_module_loader(global_object: &JSGlobalObject, host: Rc<UrlModuleHost>) {
    install_module_loader(global_object, host);
}

/// Avalia um `<script type=module>`: `url` é a URL do script externo, ou `None` para o inline (cujo fonte é
/// `source`). Devolve a promessa da avaliação; quem chama roda o laço de eventos e relata a rejeição.
pub fn evaluate_page_module(global_object: &JSGlobalObject, host: &UrlModuleHost, source: Option<&str>, url: Option<&str>) -> JSPromiseRef {
    let key = match (url, source) {
        (Some(url), _) => url.to_owned(),
        (None, source) => {
            let mut inline = host.inline.borrow_mut();
            let key = format!("{}{INLINE_MARK}{}", host.document_url.split('#').next().unwrap_or(""), inline.len());
            inline.push((key.clone(), source.unwrap_or_default().to_owned()));
            key
        }
    };
    load_and_evaluate_module(global_object, &key)
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::rc::Rc;

    use super::{evaluate_page_module, install_url_module_loader, UrlModuleHost};
    use crate::api::eval::{describe_exception, evaluate, new_global_object, program_source, VmFinalizer};
    use crate::runtime::cell_registry::run_program;
    use crate::runtime::js_promise::Status;
    use crate::wtf::text::conversion_mode::ConversionMode;

    /// Roda o módulo inline `source` na página `https://site.test/app/index.html` com a rede `files` e devolve o `log`
    /// global (em JSON) e o erro da avaliação.
    fn run(files: &[(&str, &str)], source: &str) -> (String, Option<String>) {
        let files: HashMap<String, String> = files.iter().map(|(url, text)| ((*url).to_owned(), (*text).to_owned())).collect();
        run_program(|| {
            let (vm, global_object) = new_global_object();
            let _finalizer = VmFinalizer::new(&vm);
            let host = Rc::new(UrlModuleHost::new("https://site.test/app/index.html", Rc::new(move |url: &str| files.get(url).cloned())));
            install_url_module_loader(&global_object, host.clone());
            let _ = evaluate(&global_object, &program_source("globalThis.log = [];"));
            let promise = evaluate_page_module(&global_object, &host, Some(source), None);
            vm.drain_microtasks();
            let error = match promise.status() {
                Status::Fulfilled => None,
                Status::Rejected => {
                    promise.mark_as_handled();
                    Some(describe_exception(&promise.result()))
                }
                Status::Pending => Some("pending".to_owned()),
            };
            let log = evaluate(&global_object, &program_source("JSON.stringify(log)")).ok().map_or(String::new(), |value| {
                String::from_utf8_lossy(&value.as_js_string().value().utf8(ConversionMode::LenientConversion)).into_owned()
            });
            (log, error)
        })
    }

    #[test]
    fn relative_absolute_and_root_specifiers_resolve_against_the_importer() {
        let files = [
            ("https://site.test/app/a.js", "import { b } from './lib/b.js'; export const a = 'a' + b;"),
            ("https://site.test/app/lib/b.js", "import { c } from '../../c.js'; export const b = 'b' + c;"),
            ("https://site.test/c.js", "import { d } from '/d.js'; export const c = 'c' + d;"),
            ("https://site.test/d.js", "import { e } from 'https://cdn.test/e.js'; export const d = 'd' + e;"),
            ("https://cdn.test/e.js", "export const e = 'e'; log.push(import.meta.url);"),
        ];
        let (log, error) = run(&files, "import { a } from './a.js'; log.push(a, import.meta.url);");
        assert_eq!(error, None);
        assert_eq!(log, r#"["https://cdn.test/e.js","abcde","https://site.test/app/index.html"]"#);
    }

    #[test]
    fn each_url_is_evaluated_once() {
        let files = [
            ("https://site.test/app/x.js", "log.push('x'); export const x = 1;"),
            ("https://site.test/app/y.js", "import './x.js'; export const y = 2;"),
        ];
        let (log, error) = run(&files, "import './x.js'; import './y.js'; import '/app/x.js'; log.push('done');");
        assert_eq!(error, None);
        assert_eq!(log, r#"["x","done"]"#);
    }

    #[test]
    fn bare_specifier_is_a_type_error() {
        let (_, error) = run(&[], "import 'react';");
        assert_eq!(
            error.as_deref(),
            Some(r#"TypeError: Failed to resolve module specifier "react". Relative references must start with either "/", "./", or "../"."#)
        );
    }

    #[test]
    fn dynamic_import_resolves_and_reports_network_failure() {
        let files = [("https://site.test/app/m.js", "export default 7;")];
        let (log, error) = run(
            &files,
            "const m = await import('./m.js'); log.push(m.default); \
             try { await import('./missing.js'); } catch (e) { log.push(e.name + ': ' + e.message); }",
        );
        assert_eq!(error, None);
        assert_eq!(log, r#"[7,"TypeError: Failed to fetch dynamically imported module: https://site.test/app/missing.js"]"#);
    }

    #[test]
    fn dynamic_import_waits_for_the_network_and_the_loop_runs_in_slices() {
        let files: HashMap<String, String> = [("https://site.test/app/m.js".to_owned(), "log.push('m');".to_owned())].into_iter().collect();
        let logs = run_program(|| {
            let (vm, global_object) = new_global_object();
            let _finalizer = VmFinalizer::new(&vm);
            let host = Rc::new(UrlModuleHost::new("https://site.test/app/index.html", Rc::new(move |url: &str| files.get(url).cloned())));
            host.set_network_latency_ms(1.0);
            install_url_module_loader(&global_object, host);
            crate::runtime::timers::enable_event_loop();
            let script = "globalThis.log = []; setTimeout(() => log.push('t0'), 0); setTimeout(() => log.push('t15'), 15); \
                          import('./m.js').then(() => log.push('then')); import('react').catch(() => log.push('bare'));";
            evaluate(&global_object, &program_source(script)).expect("script");
            vm.drain_microtasks();
            let read = || {
                let value = evaluate(&global_object, &program_source("JSON.stringify(log)")).expect("log");
                String::from_utf8_lossy(&value.as_js_string().value().utf8(ConversionMode::LenientConversion)).into_owned()
            };
            let before = read();
            crate::runtime::timers::run_event_loop_for(&global_object, 10.0);
            let first = read();
            // O módulo já carregado também só continua numa tarefa: depois do `setTimeout(0)` agendado antes.
            let again = "setTimeout(() => log.push('t0b'), 0); import('./m.js').then(() => log.push('again')); log.push('sync');";
            evaluate(&global_object, &program_source(again)).expect("again");
            vm.drain_microtasks();
            crate::runtime::timers::run_event_loop_for(&global_object, 10.0);
            (before, first, read())
        });
        assert_eq!(logs.0, r#"["bare"]"#);
        assert_eq!(logs.1, r#"["bare","t0","m","then"]"#);
        assert_eq!(logs.2, r#"["bare","t0","m","then","sync","t0b","again","t15"]"#);
    }

    #[test]
    fn a_slice_reaches_the_network_timer_alone_and_its_pause_ends_only_that_slice() {
        let files: HashMap<String, String> = [("https://site.test/app/m.js".to_owned(), "log.push('m');".to_owned())].into_iter().collect();
        let logs = run_program(|| {
            let (vm, global_object) = new_global_object();
            let _finalizer = VmFinalizer::new(&vm);
            let host = Rc::new(UrlModuleHost::new("https://site.test/app/index.html", Rc::new(move |url: &str| files.get(url).cloned())));
            host.set_network_latency_ms(1.0);
            install_url_module_loader(&global_object, host);
            crate::runtime::timers::enable_event_loop();
            evaluate(&global_object, &program_source("globalThis.log = []; import('./m.js').then(() => log.push('then'));")).expect("script");
            vm.drain_microtasks();
            let read = || {
                let value = evaluate(&global_object, &program_source("JSON.stringify(log)")).expect("log");
                String::from_utf8_lossy(&value.as_js_string().value().utf8(ConversionMode::LenientConversion)).into_owned()
            };
            crate::runtime::timers::run_event_loop_for(&global_object, 2.0);
            let first = read();
            evaluate(&global_object, &program_source("setTimeout(() => log.push('t3'), 3);")).expect("timer");
            crate::runtime::timers::run_event_loop_for(&global_object, 5.0);
            (first, read())
        });
        assert_eq!(logs.0, r#"["m","then"]"#);
        assert_eq!(logs.1, r#"["m","then","t3"]"#);
    }

    /// Medido no Chromium 154.
    #[test]
    fn import_meta_has_the_html_shape() {
        let files = [("https://site.test/app/m.js", "log.push(import.meta.url, import.meta.resolve('./x.js'));")];
        let (log, error) = run(
            &files,
            "const m = import.meta; const r = m.resolve; \
             log.push(Object.getPrototypeOf(m), Reflect.ownKeys(m).join(), Object.prototype.toString.call(m), Object.isExtensible(m), m === import.meta); \
             const d = Object.getOwnPropertyDescriptor(m, 'resolve'); log.push(d.writable && d.enumerable && d.configurable); \
             log.push(r.length, r.name, r.toString(), m.url, r('./x.js'), r.call({}, '/y.js')); \
             for (const s of ['react', undefined]) { try { r(s); } catch (e) { log.push(e.name + ': ' + e.message); } } \
             try { new r('./x'); } catch (e) { log.push(e.name); } \
             await import('./m.js');",
        );
        assert_eq!(error, None);
        assert_eq!(
            log,
            concat!(
                r#"[null,"url,resolve","[object Object]",true,true,true,1,"resolve","function resolve() { [native code] }","#,
                r#""https://site.test/app/index.html","https://site.test/app/x.js","https://site.test/y.js","#,
                r#""TypeError: Failed to resolve module specifier react: Relative references must start with either \"/\", \"./\", or \"../\".","#,
                r#""TypeError: Failed to resolve module specifier undefined: Relative references must start with either \"/\", \"./\", or \"../\".","#,
                r#""TypeError","https://site.test/app/m.js","https://site.test/app/x.js"]"#
            )
        );
    }

    /// O relógio virtual do navegador: `Date` e `performance` andam com o tempo dos timers a partir da origem.
    #[test]
    fn virtual_clock_moves_date_and_performance_with_the_timers() {
        let log = run_program(|| {
            let (vm, global_object) = new_global_object();
            let _finalizer = VmFinalizer::new(&vm);
            crate::wtf::date_math::set_virtual_clock(1_700_000_000_000.0);
            crate::runtime::timers::enable_event_loop();
            let script = "globalThis.log = []; const t = () => [Date.now() - performance.timeOrigin, performance.now()].join('/'); \
                          log.push(performance.timeOrigin, t(), new Date().getTime() - 1700000000000); \
                          setTimeout(() => log.push(t()), 5); setTimeout(() => log.push(t()), 30);";
            evaluate(&global_object, &program_source(script)).expect("script");
            crate::runtime::timers::run_event_loop_for(&global_object, 100.0);
            let value = evaluate(&global_object, &program_source("JSON.stringify(log)")).expect("log");
            String::from_utf8_lossy(&value.as_js_string().value().utf8(ConversionMode::LenientConversion)).into_owned()
        });
        assert_eq!(log, r#"[1700000000000,"0/0",0,"5/5","30/30"]"#);
    }

    /// O tempo virtual de CPU: o relógio anda com as instruções executadas e com a fonte cobrada, sempre igual.
    #[test]
    fn cpu_cost_moves_the_virtual_clock_deterministically() {
        let measure = |cost: bool| {
            run_program(|| {
                let (vm, global_object) = new_global_object();
                let _finalizer = VmFinalizer::new(&vm);
                crate::wtf::date_math::set_virtual_clock(1_700_000_000_000.0);
                if cost {
                    crate::runtime::cpu_clock::set_cpu_cost(1_000.0, 100.0);
                    crate::runtime::cpu_clock::charge_source_bytes(20_000);
                }
                let script = "const a = performance.now(); let s = 0; for (let i = 0; i < 10000; i++) s += i; const b = performance.now(); \
                              JSON.stringify([a, b > a, b])";
                let value = evaluate(&global_object, &program_source(script)).expect("script");
                String::from_utf8_lossy(&value.as_js_string().value().utf8(ConversionMode::LenientConversion)).into_owned()
            })
        };
        let first = measure(true);
        assert_eq!(first, measure(true));
        assert!(first.starts_with("[2.0") && first.contains(",true,"), "{first}");
        assert_eq!(measure(false), "[0,false,0]");
    }

    #[test]
    fn dynamic_import_resolution_failure_has_the_short_message() {
        let (log, error) = run(
            &[],
            "for (const s of ['react', 'http://[bad']) { try { await import(s); } catch (e) { log.push(e.name + ': ' + e.message); } }",
        );
        assert_eq!(error, None);
        assert_eq!(
            log,
            r#"["TypeError: Failed to resolve module specifier 'react'","TypeError: Failed to resolve module specifier 'http://[bad'"]"#
        );
    }
}
