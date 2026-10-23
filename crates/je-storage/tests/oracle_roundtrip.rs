//! Retrocompatibilidade do `state.vscdb` com o oráculo (H12): o banco que o je-storage escreve é lido
//! item a item pelo `@vscode/sqlite3` do VS Code real, e o banco que o VS Code escreve é lido (e
//! atualizado) pelo je-storage, com o oráculo relendo o resultado.
//!
//! Depende do oráculo instalado em `ref/oracle` (fora do git), então os testes são `#[ignore]`:
//! `cargo test -p je-storage --test oracle_roundtrip -- --ignored`.

mod common;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::rc::Rc;

use common::{TestDir, run_with_real_timers};
use je_base::map::OrderedMap;
use je_storage::{
    KeySet, SqliteStorageDatabase, SqliteStorageDatabaseOptions, StorageDatabase, UpdateRequest,
};

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("raiz do repositório")
}

/// Roda um script do `oracle-harness` no Node do Electron do VS Code real e devolve (stdout, stderr).
fn run_oracle(script: &str, args: &[&str]) -> (String, String) {
    let root = repo_root();
    let code = root.join("ref/oracle/VSCode-linux-x64/code");
    let module =
        root.join("ref/oracle/VSCode-linux-x64/resources/app/node_modules.asar/@vscode/sqlite3");
    assert!(code.exists(), "oráculo ausente em {}", code.display());
    let output = Command::new(&code)
        .env("ELECTRON_RUN_AS_NODE", "1")
        .arg(root.join("oracle-harness").join(script))
        .arg(&module)
        .args(args)
        .output()
        .expect("executa o oráculo");
    let stdout = String::from_utf8(output.stdout).expect("stdout em UTF-8");
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    assert!(output.status.success(), "o oráculo falhou: {stderr}");
    (stdout, stderr)
}

/// `sqlite_dump.js`: `chave<TAB>tipo<TAB>valor` por linha, ordenado por chave. Só vale para itens de
/// texto sem TAB nem quebra de linha.
fn oracle_dump(db: &str) -> (BTreeMap<String, String>, String) {
    let (stdout, stderr) = run_oracle("sqlite_dump.js", &[db]);
    let mut items = BTreeMap::new();
    for line in stdout.lines() {
        let mut parts = line.splitn(3, '\t');
        let (key, kind, value) = (
            parts.next().unwrap(),
            parts.next().unwrap(),
            parts.next().unwrap_or(""),
        );
        assert_eq!(
            kind, "text",
            "o je-storage grava texto, como o storage.ts: {key}"
        );
        items.insert(key.to_string(), value.to_string());
    }
    (items, stderr)
}

/// Itens variados: acentos, emoji, aspas, JSON, vazio, valor grande e mais de 129 pares (para passar
/// por mais de um pedaço do INSERT, como o `MAX_HOST_PARAMETERS` do original).
fn sample_items() -> BTreeMap<String, String> {
    let mut items = BTreeMap::new();
    items.insert("colorthemedata".to_string(), r##"{"id":"vs vscode-theme-defaults-themes-light_plus-json","colorMap":{"editor.background":"#ffffff"}}"##.to_string());
    items.insert(
        "telemetry.instanceid".to_string(),
        "d52bfcd4-4be6-476b-a38f-d44c717c41d6".to_string(),
    );
    items.insert(
        "acentuação/ção".to_string(),
        "programação em português: ç ã é ü 日本語 \u{1F600}".to_string(),
    );
    items.insert(
        "aspas".to_string(),
        r#"ele disse "oi" e 'tchau' \ barra"#.to_string(),
    );
    items.insert("vazio".to_string(), String::new());
    items.insert("grande".to_string(), "x".repeat(300_000));
    for i in 0..300 {
        items.insert(format!("lote/{i:03}"), format!("valor {i}"));
    }
    items
}

fn to_ordered(items: &BTreeMap<String, String>) -> OrderedMap<String, String> {
    let mut map = OrderedMap::new();
    for (key, value) in items {
        map.insert(key.clone(), value.clone());
    }
    map
}

/// O `user_version` do cabeçalho do SQLite (4 bytes big-endian no deslocamento 60).
fn user_version(db: &str) -> u32 {
    let header = std::fs::read(db).expect("lê o banco");
    u32::from_be_bytes(header[60..64].try_into().unwrap())
}

#[test]
#[ignore = "precisa do oráculo em ref/oracle"]
fn je_storage_writes_oracle_reads() {
    let dir = TestDir::new("oracle");
    let db = dir.join("state.vscdb");
    let items = sample_items();
    run_with_real_timers({
        let (db, items) = (db.clone(), items.clone());
        async move {
            let storage = SqliteStorageDatabase::new(&db, SqliteStorageDatabaseOptions::default());
            storage
                .update_items(UpdateRequest {
                    insert: Some(to_ordered(&items)),
                    delete: None,
                })
                .await
                .unwrap();
            storage.close(None).await.unwrap();
        }
    });

    assert_eq!(user_version(&db), 1);
    let header = std::fs::read(&db).unwrap();
    eprintln!(
        "versão de escrita/leitura no cabeçalho (2 é WAL): {} {}",
        header[18], header[19]
    );
    let (read, stderr) = oracle_dump(&db);
    assert!(stderr.contains("integrity_check: ok"), "{stderr}");
    assert_eq!(read.len(), items.len());
    assert_eq!(read, items);
}

#[test]
#[ignore = "precisa do oráculo em ref/oracle"]
fn oracle_writes_je_storage_reads_and_updates() {
    let dir = TestDir::new("oracle");
    let db = dir.join("state.vscdb");
    let seed = dir.join("items.json");
    let items = sample_items();
    // O JSON do seed: objeto chave -> valor.
    let json = {
        let mut out = String::from("{");
        for (i, (key, value)) in items.iter().enumerate() {
            if i > 0 {
                out.push(',');
            }
            out.push_str(&je_base::dynamic::json_quote(&key.as_str().into()));
            out.push(':');
            out.push_str(&je_base::dynamic::json_quote(&value.as_str().into()));
        }
        out.push('}');
        out
    };
    std::fs::write(&seed, json).unwrap();
    run_oracle("sqlite_seed.js", &[&db, &seed]);

    // O je-storage lê tudo que o VS Code escreveu, depois atualiza, apaga e insere por cima.
    let mut expected = items.clone();
    expected.insert("lote/000".to_string(), "mudou".to_string());
    expected.insert("novo".to_string(), "criado pelo je-storage".to_string());
    for i in 100..250 {
        expected.remove(&format!("lote/{i:03}"));
    }
    run_with_real_timers({
        let (db, items, expected) = (db.clone(), items.clone(), expected.clone());
        async move {
            let storage = SqliteStorageDatabase::new(&db, SqliteStorageDatabaseOptions::default());
            let stored = storage.get_items().await.unwrap();
            assert_eq!(stored.len(), items.len());
            for (key, value) in &items {
                assert_eq!(stored.get(key).as_ref(), Some(value), "{key}");
            }

            let mut insert = OrderedMap::new();
            insert.insert("lote/000".to_string(), "mudou".to_string());
            insert.insert("novo".to_string(), "criado pelo je-storage".to_string());
            let mut delete = KeySet::new();
            for i in 100..250 {
                delete.insert(format!("lote/{i:03}"), ());
            }
            storage
                .update_items(UpdateRequest {
                    insert: Some(insert),
                    delete: Some(delete),
                })
                .await
                .unwrap();
            let after = Rc::new(storage.get_items().await.unwrap());
            assert_eq!(after.len(), expected.len());
            storage.close(None).await.unwrap();
        }
    });

    let (read, stderr) = oracle_dump(&db);
    assert!(stderr.contains("integrity_check: ok"), "{stderr}");
    assert_eq!(read, expected);
    assert_eq!(user_version(&db), 1);
}
