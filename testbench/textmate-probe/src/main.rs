//! Sonda da H14: tokeniza as fixtures do `vscode-colorize-tests` com o `ferriki-textmate` e compara
//! com as goldens geradas pelo `_workbench.captureSyntaxTokens` do VS Code 1.143.1.

mod catalog;
mod glob;
mod jsonc;
mod theme;
mod tokenize;

use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, mpsc};
use std::time::{Duration, Instant};

use catalog::Catalog;
use serde_json::Value;
use theme::Theme;
use tokenize::{Outcome, Token};

const FIXTURE_TIMEOUT: Duration = Duration::from_secs(180);

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").canonicalize().expect("raiz do workspace")
}

fn load_golden(path: &Path) -> Option<Vec<Token>> {
    let value: Value = serde_json::from_str(&fs::read_to_string(path).ok()?).ok()?;
    Some(
        value
            .as_array()?
            .iter()
            .map(|t| Token {
                c: t["c"].as_str().unwrap_or_default().to_string(),
                t: t["t"].as_str().unwrap_or_default().to_string(),
                r: t["r"]
                    .as_object()
                    .map(|m| m.iter().filter_map(|(k, v)| v.as_str().map(|v| (k.clone(), v.to_string()))).collect())
                    .unwrap_or_default(),
            })
            .collect(),
    )
}

/// Casa token a token pelo deslocamento no texto concatenado, para uma divergência não deslocar o resto.
struct Score {
    total: usize,
    ct_equal: usize,
    full_equal: usize,
}

fn offsets(tokens: &[Token]) -> Vec<usize> {
    let mut at = 0;
    tokens
        .iter()
        .map(|t| {
            let start = at;
            at += t.c.encode_utf16().count();
            start
        })
        .collect()
}

fn score(expected: &[Token], got: &[Token]) -> Score {
    let by_position: HashMap<(usize, &str, &str), &Token> =
        offsets(got).into_iter().zip(got).map(|(o, t)| ((o, t.c.as_str(), t.t.as_str()), t)).collect();
    let mut s = Score { total: expected.len(), ct_equal: 0, full_equal: 0 };
    for (offset, e) in offsets(expected).into_iter().zip(expected) {
        if let Some(g) = by_position.get(&(offset, e.c.as_str(), e.t.as_str())) {
            s.ct_equal += 1;
            s.full_equal += usize::from(g.r == e.r);
        }
    }
    s
}

fn describe_divergence(expected: &[Token], got: &[Token]) -> String {
    let index = expected.iter().zip(got).position(|(e, g)| e != g).unwrap_or(expected.len().min(got.len()));
    match (expected.get(index), got.get(index)) {
        (Some(e), Some(g)) => {
            let r_diff: Vec<String> = e
                .r
                .iter()
                .filter(|(k, v)| g.r.get(*k) != Some(v))
                .map(|(k, v)| format!("{k}: esperado {v:?}, obtido {:?}", g.r.get(k)))
                .collect();
            format!(
                "#{index} esperado c={:?} t={:?}\n      obtido   c={:?} t={:?}{}",
                e.c,
                e.t,
                g.c,
                g.t,
                if e.c == g.c && e.t == g.t { format!("\n      r: {}", r_diff.join("; ")) } else { String::new() }
            )
        }
        (Some(e), None) => format!("#{index} esperado c={:?} t={:?}; obtido: fim dos tokens", e.c, e.t),
        (None, Some(g)) => format!("#{index} esperado: fim dos tokens; obtido c={:?} t={:?}", g.c, g.t),
        (None, None) => "sem divergência".to_string(),
    }
}

struct FixtureReport {
    name: String,
    expected: Vec<Token>,
    got: Vec<Token>,
    status: String,
    elapsed: Duration,
}

fn percent(part: usize, total: usize) -> f64 {
    if total == 0 { 100.0 } else { part as f64 * 100.0 / total as f64 }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let detail = args.iter().position(|a| a == "--detail").and_then(|i| args.get(i + 1)).cloned();
    let only = args.iter().position(|a| a == "--only").and_then(|i| args.get(i + 1)).cloned();
    let options = tokenize::Options {
        workaround_bare_p: args.iter().any(|a| a == "--workaround-bare-p"),
        no_injections: args.iter().any(|a| a == "--control-no-injections"),
    };
    let root = workspace_root();
    let extensions = root.join("ref/vscode/extensions");
    let tests = extensions.join("vscode-colorize-tests/test");

    let catalog = Arc::new(Catalog::load(&extensions).expect("catálogo de extensões"));
    let themes = Arc::new(load_themes(&extensions));
    let grammars = Arc::new(tokenize::GrammarSources::load(&catalog, options));
    println!(
        "{} linguagens, {} gramáticas, {} temas ({})",
        catalog.languages.len(),
        catalog.grammars.len(),
        themes.len(),
        themes.iter().map(|t| t.id.as_str()).collect::<Vec<_>>().join(", ")
    );

    let mut fixtures: Vec<PathBuf> = fs::read_dir(tests.join("colorize-fixtures"))
        .expect("fixtures")
        .filter_map(|e| e.ok().map(|e| e.path()))
        .collect();
    fixtures.sort();
    if let Some(only) = &only {
        fixtures.retain(|p| p.file_name().is_some_and(|n| n.to_string_lossy() == *only));
    }

    let mut reports = Vec::new();
    for fixture in &fixtures {
        let name = fixture.file_name().expect("nome").to_string_lossy().to_string();
        let golden_path = tests.join("colorize-results").join(format!("{}.json", name.replacen('.', "_", 1)));
        let Some(expected) = load_golden(&golden_path) else {
            println!("{name}: sem golden em {}", golden_path.display());
            continue;
        };
        let started = Instant::now();
        let (tx, rx) = mpsc::channel();
        let (catalog, themes, grammars, path) = (catalog.clone(), themes.clone(), grammars.clone(), fixture.clone());
        std::thread::Builder::new()
            .stack_size(512 << 20)
            .spawn(move || {
                let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    tokenize::run(&catalog, &themes, &grammars, &path)
                }))
                .unwrap_or_else(|p| {
                    let msg = p.downcast_ref::<String>().cloned().or_else(|| p.downcast_ref::<&str>().map(|s| s.to_string()));
                    Outcome { tokens: Vec::new(), status: format!("pânico: {}", msg.unwrap_or_default()) }
                });
                let _ = tx.send(outcome);
            })
            .expect("thread");
        let outcome = rx
            .recv_timeout(FIXTURE_TIMEOUT)
            .unwrap_or_else(|_| Outcome { tokens: Vec::new(), status: format!("timeout de {}s", FIXTURE_TIMEOUT.as_secs()) });
        reports.push(FixtureReport { name, expected, got: outcome.tokens, status: outcome.status, elapsed: started.elapsed() });
    }

    print_report(&reports, detail.as_deref());
    std::process::exit(0);
}

fn load_themes(extensions: &Path) -> Vec<Theme> {
    let dir = extensions.join("theme-defaults");
    let manifest = jsonc::parse(&fs::read_to_string(dir.join("package.json")).expect("package.json")).expect("json");
    manifest["contributes"]["themes"]
        .as_array()
        .expect("themes")
        .iter()
        .map(|entry| Theme::load(&dir, entry).unwrap_or_else(|e| panic!("tema: {e}")))
        .collect()
}

fn print_report(reports: &[FixtureReport], detail: Option<&str>) {
    println!("\n{:<32} {:>13}  {:>13}  estado", "fixture", "c+t", "c+t+r");
    let (mut total, mut ct, mut full, mut whole) = (0, 0, 0, 0);
    let mut worst: Vec<(f64, &FixtureReport)> = Vec::new();
    let mut by_status: BTreeMap<String, usize> = BTreeMap::new();
    for r in reports {
        let s = score(&r.expected, &r.got);
        let matches_whole = r.expected == r.got;
        total += s.total;
        ct += s.ct_equal;
        full += s.full_equal;
        whole += usize::from(matches_whole);
        let status = if matches_whole {
            "igual".to_string()
        } else if !r.status.is_empty() {
            r.status.clone()
        } else {
            "diverge".to_string()
        };
        *by_status.entry(status.split([':', '"']).next().unwrap_or_default().trim().to_string()).or_default() += 1;
        println!(
            "{:<32} {:>6}/{:<6}  {:>6}/{:<6}  {} ({:.1}s)",
            r.name, s.ct_equal, s.total, s.full_equal, s.total, status, r.elapsed.as_secs_f64()
        );
        if !matches_whole {
            worst.push((percent(s.full_equal, s.total), r));
        }
    }
    println!("\nfixtures inteiras iguais: {whole} de {}", reports.len());
    println!("tokens com c+t iguais: {ct}/{total} = {:.2}%", percent(ct, total));
    println!("tokens com c+t+r iguais: {full}/{total} = {:.2}%", percent(full, total));
    println!("estados: {by_status:?}");

    worst.sort_by(|a, b| a.0.total_cmp(&b.0));
    println!("\n10 piores fixtures (por tokens c+t+r iguais):");
    for (pct, r) in worst.iter().take(10) {
        println!("  {} ({pct:.1}%, estado: {})\n    primeiro token divergente {}", r.name, r.status, describe_divergence(&r.expected, &r.got));
    }
    if let Some(name) = detail {
        println!("\ndetalhe de {name}:");
        if let Some(r) = reports.iter().find(|r| r.name == name) {
            let mut shown = 0;
            for (i, (e, g)) in r.expected.iter().zip(&r.got).enumerate() {
                if e != g && shown < 25 {
                    shown += 1;
                    println!("  {}", describe_divergence(&r.expected[i..], &r.got[i..]).replacen("#0", &format!("#{i}"), 1));
                }
            }
        }
    }
}
