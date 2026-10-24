//! Gera as tabelas de errno e de sinais a partir de `data/linux_facts.json`,
//! extraído do Debian 13 real pelo experimento E05. Assim nenhuma mensagem é digitada à mão.

use std::fmt::Write as _;
use std::path::PathBuf;

fn main() {
    let manifest = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"));
    let facts_path = manifest.join("data/linux_facts.json");
    println!("cargo:rerun-if-changed={}", facts_path.display());
    let text = std::fs::read_to_string(&facts_path).expect("linux_facts.json (rode o E05)");
    let facts: serde_json::Value = serde_json::from_str(&text).expect("JSON válido");

    let mut out = String::new();

    // errno: (número, nome, mensagem).
    let mut errnos: Vec<(i32, Option<String>, String)> = facts["errno"]
        .as_object()
        .expect("errno")
        .iter()
        .map(|(n, v)| {
            (
                n.parse().expect("número"),
                v["name"].as_str().map(str::to_string),
                v["strerror"].as_str().expect("strerror").to_string(),
            )
        })
        .collect();
    errnos.sort_by_key(|e| e.0);
    out.push_str("impl Errno {\n");
    for (n, name, _) in &errnos {
        if let Some(name) = name {
            writeln!(out, "    pub const {name}: Errno = Errno({n});").unwrap();
        }
    }
    out.push_str("}\n\n");
    out.push_str("/// (número, nome, strerror da glibc 2.41).\n");
    out.push_str("pub(crate) const ERRNO_TABLE: &[(i32, Option<&str>, &str)] = &[\n");
    for (n, name, msg) in &errnos {
        let name = match name {
            Some(s) => format!("Some({s:?})"),
            None => "None".to_string(),
        };
        writeln!(out, "    ({n}, {name}, {msg:?}),").unwrap();
    }
    out.push_str("];\n\n");

    // Sinais: (número, nomes, descrição do strsignal).
    let mut signals: Vec<(i32, Vec<String>, String)> = facts["signals"]
        .as_object()
        .expect("signals")
        .iter()
        .map(|(n, v)| {
            (
                n.parse().expect("número"),
                v["names"].as_array().expect("names").iter().map(|x| x.as_str().unwrap().to_string()).collect(),
                v["description"].as_str().unwrap_or("").to_string(),
            )
        })
        .collect();
    signals.sort_by_key(|s| s.0);
    out.push_str("impl Signal {\n");
    for (n, names, _) in &signals {
        for name in names {
            writeln!(out, "    pub const {name}: Signal = Signal({n});").unwrap();
        }
    }
    out.push_str("}\n\n");
    out.push_str("/// (número, nome canônico, descrição do strsignal da glibc 2.41).\n");
    out.push_str("pub(crate) const SIGNAL_TABLE: &[(i32, &str, &str)] = &[\n");
    for (n, names, desc) in &signals {
        // Nome canônico: o primeiro que não é alias histórico.
        let canonical = names
            .iter()
            .find(|s| !matches!(s.as_str(), "SIGIOT" | "SIGPOLL" | "SIGCLD"))
            .unwrap_or(&names[0]);
        writeln!(out, "    ({n}, {canonical:?}, {desc:?}),").unwrap();
    }
    out.push_str("];\n");

    let dest = PathBuf::from(std::env::var("OUT_DIR").expect("OUT_DIR")).join("linux_tables.rs");
    std::fs::write(dest, out).expect("gravar tabelas");
}
