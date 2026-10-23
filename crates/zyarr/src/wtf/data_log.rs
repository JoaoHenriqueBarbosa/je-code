//! `WTF/wtf/DataLog.h`, reduzido: o `dataFile()` do zjsc (trocável, por thread, sobre o `host()` do
//! runtime) vira `stderr` direto, que é o padrão dele. Só o YARR chama, para o `dumpPattern`.

use std::io::Write;

/// `dataLog(values...)` com o texto já formatado: escreve em `stderr` e dá `flush`.
pub fn data_log(text: &str) {
    let mut stderr = std::io::stderr();
    let _ = stderr.write_all(text.as_bytes());
    let _ = stderr.flush();
}
