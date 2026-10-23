//! Conferência de `comparers` contra o `comparers.ts` real rodando no Node (V8 com ICU), sobre todos os
//! pares de um corpus de 169 nomes com acentos, caixa, números (`file2` contra `file10`), pontuação,
//! nomes com ponto, dotfiles, terminadores de linha, CJK, emoji e surrogate isolado.
//!
//! O arquivo `golden/comparers.txt` é gerado por um script Node que carrega o `comparers.ts` com os tipos
//! removidos (`module.stripTypeScriptTypes`), e a primeira linha dele registra as versões do V8, do ICU e
//! do CLDR. A locale é `en-US` nos dois lados.

use je_base::comparers;

fn parse_units(line: &str) -> Vec<u16> {
    if line == "-" { Vec::new() } else { line.split(' ').map(|h| u16::from_str_radix(h, 16).unwrap()).collect() }
}

fn sign(n: i32) -> char {
    match n.signum() {
        -1 => '-',
        1 => '+',
        _ => '0',
    }
}

fn show(units: &[u16]) -> String {
    format!("{:?}", String::from_utf16_lossy(units))
}

/// Cada função do golden, com o mesmo trio de argumentos que o script usa (`lookFor` é `a` nas duas
/// que o recebem, e `comparePaths` é insensível a caixa).
fn call(name: &str, a: &[u16], b: &[u16]) -> i32 {
    let look_for = [u16::from(b'a')];
    let platform = je_base::platform::Platform::from_native(&je_base::platform::NativeEnvironment {
        platform: "linux",
        env: &|_| None,
        is_electron: false,
    });
    match name {
        "compareFileNames" => comparers::compare_file_names(Some(a), Some(b)),
        "compareFileNamesDefault" => comparers::compare_file_names_default(Some(a), Some(b)),
        "compareFileNamesUpper" => comparers::compare_file_names_upper(Some(a), Some(b)),
        "compareFileNamesLower" => comparers::compare_file_names_lower(Some(a), Some(b)),
        "compareFileNamesUnicode" => comparers::compare_file_names_unicode(Some(a), Some(b)),
        "compareFileExtensions" => comparers::compare_file_extensions(Some(a), Some(b)),
        "compareFileExtensionsDefault" => comparers::compare_file_extensions_default(Some(a), Some(b)),
        "compareFileExtensionsUpper" => comparers::compare_file_extensions_upper(Some(a), Some(b)),
        "compareFileExtensionsLower" => comparers::compare_file_extensions_lower(Some(a), Some(b)),
        "compareFileExtensionsUnicode" => comparers::compare_file_extensions_unicode(Some(a), Some(b)),
        "comparePaths" => comparers::compare_paths(a, b, false, &platform),
        "compareAnything" => comparers::compare_anything(a, b, &look_for),
        "compareByPrefix" => comparers::compare_by_prefix(a, b, &look_for),
        other => panic!("função desconhecida no golden: {other}"),
    }
}

fn null_call(name: &str, a: Option<&[u16]>, b: Option<&[u16]>) -> i32 {
    match name {
        "compareFileNames" => comparers::compare_file_names(a, b),
        "compareFileNamesDefault" => comparers::compare_file_names_default(a, b),
        "compareFileNamesUpper" => comparers::compare_file_names_upper(a, b),
        "compareFileNamesLower" => comparers::compare_file_names_lower(a, b),
        "compareFileNamesUnicode" => comparers::compare_file_names_unicode(a, b),
        "compareFileExtensions" => comparers::compare_file_extensions(a, b),
        "compareFileExtensionsDefault" => comparers::compare_file_extensions_default(a, b),
        "compareFileExtensionsUpper" => comparers::compare_file_extensions_upper(a, b),
        "compareFileExtensionsLower" => comparers::compare_file_extensions_lower(a, b),
        "compareFileExtensionsUnicode" => comparers::compare_file_extensions_unicode(a, b),
        other => panic!("função desconhecida no golden: {other}"),
    }
}

#[test]
fn comparers_match_node_on_every_pair_of_the_corpus() {
    let golden = include_str!("golden/comparers.txt");
    let mut lines = golden.lines().filter(|l| !l.starts_with('#'));

    let count: usize = lines.next().unwrap().strip_prefix("corpus ").unwrap().parse().unwrap();
    let corpus: Vec<Vec<u16>> = (0..count).map(|_| parse_units(lines.next().unwrap())).collect();

    let mut mismatches = Vec::new();
    let mut checked = 0usize;
    let mut functions = 0usize;
    let mut pending = lines.peekable();
    while let Some(line) = pending.next() {
        if let Some(name) = line.strip_prefix("fn ") {
            functions += 1;
            for a in &corpus {
                let row = pending.next().unwrap();
                for (b, expected) in corpus.iter().zip(row.chars()) {
                    let actual = sign(call(name, a, b));
                    checked += 1;
                    if actual != expected {
                        mismatches.push(format!("{name}({}, {}): Node {expected}, aqui {actual}", show(a), show(b)));
                    }
                }
            }
        } else if let Some(rest) = line.strip_prefix("null ") {
            let (name, expected) = rest.split_once(' ').unwrap();
            let a = [u16::from(b'a')];
            let actual: String = [null_call(name, None, None), null_call(name, None, Some(&a)), null_call(name, Some(&a), None)]
                .into_iter()
                .map(sign)
                .collect();
            assert_eq!(actual, expected, "{name} com null");
        }
    }

    assert_eq!(functions, 13);
    assert!(
        mismatches.is_empty(),
        "{} divergências de {checked} pares; as primeiras:\n{}",
        mismatches.len(),
        mismatches.iter().take(25).cloned().collect::<Vec<_>>().join("\n")
    );
}
