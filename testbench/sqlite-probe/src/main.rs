//! Sonda H12: um SQLite em Rust puro (turso) lendo e escrevendo o `state.vscdb` do VS Code.
//!
//! `sqlite-probe dump <db>` imprime `chave<TAB>tipo<TAB>valor` de `ItemTable`, ordenado por chave,
//! no mesmo formato que `oracle-harness/sqlite_dump.js` imprime pelo `@vscode/sqlite3` do oráculo.
//! `sqlite-probe write <db> <chave> <valor>` faz o upsert exatamente como o `storage.ts` faz.

use std::fmt::Write as _;

use turso::{Builder, Connection, Value};

fn render(value: &Value) -> (&'static str, String) {
    match value {
        Value::Null => ("null", String::new()),
        Value::Integer(i) => ("integer", i.to_string()),
        Value::Real(r) => ("real", r.to_string()),
        Value::Text(t) => ("text", t.clone()),
        Value::Blob(b) => ("blob", b.iter().fold(String::new(), |mut s, byte| {
            let _ = write!(s, "{byte:02x}");
            s
        })),
    }
}

async fn dump(conn: &Connection) -> turso::Result<usize> {
    let mut rows = conn.query("SELECT key, value FROM ItemTable ORDER BY key", ()).await?;
    let mut count = 0;
    while let Some(row) = rows.next().await? {
        let (_, key) = render(&row.get_value(0)?);
        let (kind, value) = render(&row.get_value(1)?);
        println!("{key}\t{kind}\t{value}");
        count += 1;
    }
    Ok(count)
}

async fn run(args: &[String]) -> turso::Result<()> {
    let db = Builder::new_local(&args[1]).build().await?;
    let conn = db.connect()?;
    match args[0].as_str() {
        "dump" => {
            let n = dump(&conn).await?;
            eprintln!("{n} linhas");
        }
        "write" => {
            let changed = conn
                .execute(
                    "INSERT INTO ItemTable VALUES (?,?) ON CONFLICT (key) DO UPDATE SET value = excluded.value WHERE value != excluded.value",
                    (args[2].as_str(), args[3].as_str()),
                )
                .await?;
            eprintln!("{changed} linha(s) alterada(s)");
        }
        other => eprintln!("modo desconhecido: {other}"),
    }
    Ok(())
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.len() < 2 {
        eprintln!("uso: sqlite-probe dump <db> | write <db> <chave> <valor>");
        std::process::exit(2);
    }
    if let Err(err) = pollster::block_on(run(&args)) {
        eprintln!("erro: {err}");
        std::process::exit(1);
    }
}
