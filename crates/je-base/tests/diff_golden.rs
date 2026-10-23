//! Conferência de `diff.ts` contra o `diff.ts` real rodando no Node (V8): o diff de caracteres (com e sem
//! `pretty`), o diff de linhas (`string[]`, que liga as fronteiras de linha vazia e a busca de sequência
//! contígua melhor) e a distância de Levenshtein (curta e longa, com os blocos de 32 e de 64).
//!
//! O arquivo `golden/diff.txt` é gerado por `scripts/gen_diff_golden.mjs`, que carrega o `diff.ts` com os
//! tipos removidos (`module.stripTypeScriptTypes`) e sorteia os casos com uma semente fixa. A primeira
//! linha registra a versão do Node. Cada linha tem o tipo (`C` caracteres, `L` linhas, `V` Levenshtein),
//! as entradas e o resultado.

use std::rc::Rc;

use je_base::diff::{DiffChange, ISequence, LcsDiff, SequenceElements, StringDiffSequence, compute_levenshtein_distance};
use je_base::js_string::JsString;

fn units(field: &str) -> Vec<u16> {
    if field == "-" { Vec::new() } else { field.split(' ').map(|h| u16::from_str_radix(h, 16).unwrap()).collect() }
}

fn changes(field: &str) -> Vec<DiffChange> {
    if field == "-" {
        return Vec::new();
    }
    field
        .split(' ')
        .map(|c| {
            let n: Vec<i32> = c.split(',').map(|v| v.parse().unwrap()).collect();
            DiffChange::new(n[0], n[1], n[2], n[3])
        })
        .collect()
}

struct LineSequence(Vec<JsString>);

impl ISequence for LineSequence {
    fn get_elements(&self) -> SequenceElements {
        SequenceElements::Strings(self.0.clone())
    }
}

fn lines(field: &str) -> Vec<JsString> {
    if field == "=" {
        return Vec::new();
    }
    field
        .split(' ')
        .map(|line| {
            if line == "-" {
                JsString::new()
            } else {
                JsString::from_units(line.split('.').map(|h| u16::from_str_radix(h, 16).unwrap()).collect())
            }
        })
        .collect()
}

#[test]
fn diff_matches_node() {
    let golden = include_str!("golden/diff.txt");
    let (mut chars, mut line_cases, mut levenshtein) = (0, 0, 0);
    for (number, line) in golden.lines().enumerate().skip(1) {
        let (kind, rest) = line.split_once(' ').unwrap();
        let fields: Vec<&str> = rest.split(" | ").collect();
        match kind {
            "C" => {
                let (pretty, original) = fields[0].split_once(' ').unwrap();
                let (quit_early, expected) = fields[2].split_once(' ').unwrap();
                let original = JsString::from_units(units(original));
                let modified = JsString::from_units(units(fields[1]));
                let result = LcsDiff::new(Rc::new(StringDiffSequence::new(original.clone())), Rc::new(StringDiffSequence::new(modified.clone())), None)
                    .compute_diff(pretty == "1");
                assert_eq!(result.changes, changes(expected), "linha {}: {original:?} contra {modified:?}", number + 1);
                assert_eq!(result.quit_early, quit_early == "1");
                chars += 1;
            }
            "L" => {
                let (quit_early, expected) = fields[2].split_once(' ').unwrap();
                let result = LcsDiff::new(Rc::new(LineSequence(lines(fields[0]))), Rc::new(LineSequence(lines(fields[1]))), None).compute_diff(true);
                assert_eq!(result.changes, changes(expected), "linha {} do golden", number + 1);
                assert_eq!(result.quit_early, quit_early == "1");
                line_cases += 1;
            }
            "V" => {
                let (a, b) = (units(fields[0]), units(fields[1]));
                let expected: i32 = fields[2].parse().unwrap();
                assert_eq!(compute_levenshtein_distance(&a, &b), expected, "linha {} do golden: {:?} contra {:?}", number + 1, String::from_utf16_lossy(&a), String::from_utf16_lossy(&b));
                levenshtein += 1;
            }
            other => panic!("tipo desconhecido no golden: {other}"),
        }
    }
    assert!(chars >= 800 && line_cases >= 400 && levenshtein >= 300, "{chars} {line_cases} {levenshtein}");
}
