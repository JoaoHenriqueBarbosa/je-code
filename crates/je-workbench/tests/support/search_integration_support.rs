//! Apoio dos portes dos `*.integrationTest.ts` da busca (`textSearch`, `search` e `fileSearch`): as pastas de fixture, a
//! escolha do `rg` e o `symlinkLoopFixture.ts`.
//!
//! - As fixtures são as cópias intactas de `src/vs/workbench/services/search/test/node/fixtures` e `fixtures2` em
//!   `tests/fixtures/search/node/`; o `FileAccess.asFileUri(...)` do original é o caminho a partir de `CARGO_MANIFEST_DIR`.
//! - O binário do `rg` é o do oráculo (`ref/oracle`), registrado em `set_rg_disk_path`, ou o `rg` do `PATH`.
//! - `createCrossLinkedFolders` só existe onde há link simbólico sem privilégio elevado (o original pula o caso no Windows).

#![allow(dead_code)]

use std::path::Path;

use je_base::glob::Expression;
use je_workbench::services::search::ripgrep_disk_path::set_rg_disk_path;

fn node_fixtures_root() -> String {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests").join("fixtures").join("search").join("node").to_string_lossy().into_owned()
}

/// `TEST_FIXTURES`.
pub fn test_fixtures() -> String {
    format!("{}/fixtures", node_fixtures_root())
}

/// `TEST_FIXTURES2`.
pub fn test_fixtures2() -> String {
    format!("{}/fixtures2", node_fixtures_root())
}

/// O `rg` do oráculo, se o repositório o tiver; senão vale o `rg` do `PATH`.
pub fn use_oracle_ripgrep() {
    let oracle = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../ref/oracle/VSCode-linux-x64/resources/app/node_modules.asar.unpacked/@vscode/ripgrep-universal/bin/linux-x64/rg");
    if oracle.is_file() {
        set_rg_disk_path(oracle.to_string_lossy().into_owned());
    }
}

/// `makeExpression(...patterns)`: cada padrão vira uma entrada `true` (o `glob.ts` quer barras normais).
pub fn make_expression(patterns: &[&str]) -> Expression {
    Expression::from_entries(patterns.iter().map(|pattern| (pattern.replace('\\', "/"), true)))
}

/// `createCrossLinkedFolders(root, folderCount, fileContents)`: pastas que têm cada uma um `file.txt` e um link simbólico para
/// toda pasta. O `rg` precisa de tempo exponencial para seguir esses links porque só detecta os que apontam para um
/// ancestral. Devolve os caminhos absolutos dos arquivos criados.
#[cfg(unix)]
pub fn create_cross_linked_folders(root: &Path, folder_count: usize, file_contents: &str) -> Vec<String> {
    let mut files = Vec::new();
    for i in 0..folder_count {
        let folder = root.join(i.to_string());
        std::fs::create_dir_all(&folder).expect("cria a pasta");

        let file = folder.join("file.txt");
        std::fs::write(&file, file_contents).expect("escreve o arquivo");
        files.push(file.to_string_lossy().into_owned());

        for j in 0..folder_count {
            std::os::unix::fs::symlink(format!("../{j}"), folder.join(format!("link{j}"))).expect("cria o link simbólico");
        }
    }
    files
}
