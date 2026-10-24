//! Ferramenta de medição (não é produto): ativa uma extensão embutida sobre um `vscode` que só REGISTRA o que a extensão
//! toca (um `Proxy` de JavaScript, escrito aqui como dado de teste) e imprime os membros da API pedidos, na ordem.
//!
//! Uso: `cargo run -p je-exthost --example api_usage -- <diretório da extensão>`.
//! Serve para dimensionar a fábrica da API em Rust: quais membros do `vscode.d.ts` cada extensão usa na ativação.

use std::rc::Rc;

use je_exthost::runtime::{with_runtime_thread, DiskModuleFs, OsConsole};
use zjsc::api::embed_node::{run_node, JsData};

/// `names` entra como `__names`: o objeto de primeiro nível precisa ter essas chaves PRÓPRIAS, porque o `__toESM` dos
/// pacotes do esbuild copia as chaves próprias do módulo (`require('vscode')`) para o objeto que a extensão enxerga.
const RECORDING_VSCODE: &str = r#"
(function (__names) {
  const touched = [];
  const make = (path) => new Proxy(function () {}, {
    get(_target, key) {
      if (typeof key === 'symbol') return undefined;
      if (key === 'then') return undefined;
      const next = path ? path + '.' + key : String(key);
      if (!touched.includes(next)) touched.push(next);
      return make(next);
    },
    apply(_target, _this, args) { if (!touched.includes(path + '()')) touched.push(path + '()'); return make(path + '()'); },
    construct(_target, args) { if (!touched.includes('new ' + path)) touched.push('new ' + path); return make('new ' + path); },
  });
  globalThis.__touched = touched;
  const top = {};
  for (const name of __names) top[name] = make(name);
  return new Proxy(top, {
    get(target, key) {
      if (typeof key === 'symbol' || !(key in target)) return target[key];
      if (!touched.includes(key)) touched.push(key);
      return target[key];
    },
  });
})
"#;

/// Os nomes de primeiro nível do `vscode.d.ts` (`namespace`, `class`, `enum`, `function`, `const`).
fn top_level_names(dts: &str) -> Vec<String> {
    let mut names = Vec::new();
    for line in dts.lines() {
        let Some(rest) = line.strip_prefix("\texport ") else { continue };
        let rest = rest.strip_prefix("abstract ").unwrap_or(rest);
        for keyword in ["namespace ", "class ", "enum ", "function ", "const ", "let "] {
            if let Some(after) = rest.strip_prefix(keyword) {
                let name: String = after.chars().take_while(|c| c.is_alphanumeric() || *c == '_').collect();
                if !name.is_empty() && !names.contains(&name) {
                    names.push(name);
                }
            }
        }
    }
    names
}

fn main() {
    let ext_dir = std::env::args().nth(1).expect("uso: api_usage <diretório da extensão> [vscode.d.ts]");
    let dts_path = std::env::args().nth(2).unwrap_or_else(|| "ref/vscode/src/vscode-dts/vscode.d.ts".to_owned());
    let names = top_level_names(&std::fs::read_to_string(dts_path).expect("vscode.d.ts"));
    let report = with_runtime_thread(move || {
        let manifest = std::fs::read_to_string(format!("{ext_dir}/package.json")).expect("package.json");
        let main_file = manifest.split("\"main\"").nth(1).and_then(|rest| rest.split('"').nth(1)).expect("campo main").to_owned();
        let mut entry = format!("{ext_dir}/{}", main_file.trim_start_matches("./"));
        if !std::path::Path::new(&entry).exists() {
            entry.push_str(".js");
        }
        let main_source: &'static str = Box::leak(format!("globalThis.__extension = () => require({entry:?});").into_boxed_str());
        run_node(Rc::new(OsConsole), Rc::new(DiskModuleFs), "/je-probe/main.js", main_source, |runner| {
            let factory = runner.eval(RECORDING_VSCODE, "recording.js").unwrap();
            let names = JsData::Array(names.into_iter().map(JsData::Str).collect());
            let recording = runner.call_data(&factory, &[names]).unwrap();
            runner.register_module("vscode", &recording);
            let load = runner.eval("__extension", "load.js").unwrap();
            let outcome = match runner.call(&load, &[]) {
                Ok(_) => runner
                    .eval(
                        "(function () { const e = __extension(); const context = { subscriptions: [], extensionPath: '/x', globalState: { get() {}, update() {} }, workspaceState: { get() {}, update() {} } }; return Promise.resolve(e.activate && e.activate(context)).then(() => 'ativou', (err) => 'falhou: ' + (err && err.message)); })()",
                        "activate.js",
                    )
                    .map(|_| ()),
                Err(message) => Err(message),
            };
            runner.run_event_loop_for(50.0);
            let touched = runner.eval("globalThis.__touched.join('\\n')", "report.js").unwrap().as_text().unwrap_or_default();
            format!("carga: {outcome:?}\n{touched}")
        })
        .unwrap()
    });
    println!("{report}");
}
