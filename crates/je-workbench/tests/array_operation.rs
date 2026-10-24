//! Porte de `src/vs/workbench/services/textMate/test/browser/arrayOperation.test.ts` (suíte `array operation`),
//! caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - `edit.applyArray(arr, undefined)` usa `Option<String>` como tipo do elemento, com `None` no lugar do
//!   `undefined`; a conferência de vazamento do fim da suíte não tem o que conferir (nada é descartável).
//! - O `deepStrictEqual` de `undefined` no array vira `None`; a linha `newArr[t]: undefined` do texto sairia
//!   igual no original, e aqui o `Option` imprime como `undefined` na função `show`.

use je_editor::edits::length_edit::{LengthEdit, LengthReplacement};
use je_workbench::services::text_mate::index_transformer::{IIndexTransformer, MonotonousIndexTransformer};

fn seq(start: i32, end: i32) -> Vec<i32> {
    (start..end).collect()
}

fn show(item: &Option<String>) -> String {
    item.clone().unwrap_or_else(|| "undefined".to_owned())
}

#[test]
fn simple() {
    let edit = LengthEdit::create(vec![
        LengthReplacement::create(4, 7, 2),
        LengthReplacement::create(8, 8, 2),
        LengthReplacement::create(9, 11, 0),
    ]);

    let arr: Vec<Option<String>> = seq(0, 15).into_iter().map(|x| Some(format!("item{x}"))).collect();
    let new_arr = edit.apply_array(&arr, None);
    let expected: Vec<Option<String>> = [
        Some("item0"),
        Some("item1"),
        Some("item2"),
        Some("item3"),
        None,
        None,
        Some("item7"),
        None,
        None,
        Some("item8"),
        Some("item11"),
        Some("item12"),
        Some("item13"),
        Some("item14"),
    ]
    .into_iter()
    .map(|item| item.map(str::to_owned))
    .collect();
    assert_eq!(new_arr, expected);

    let mut transformer = MonotonousIndexTransformer::new(&edit);
    let actual: Vec<String> = seq(0, 15)
        .into_iter()
        .map(|x| {
            let t = transformer.transform(x);
            let mut r = format!("arr[{x}]: {} -> ", show(&arr[x as usize]));
            match t {
                Some(t) => r.push_str(&format!("newArr[{t}]: {}", show(&new_arr[t as usize]))),
                None => r.push_str("undefined"),
            }
            r
        })
        .collect();
    assert_eq!(
        actual,
        vec![
            "arr[0]: item0 -> newArr[0]: item0",
            "arr[1]: item1 -> newArr[1]: item1",
            "arr[2]: item2 -> newArr[2]: item2",
            "arr[3]: item3 -> newArr[3]: item3",
            "arr[4]: item4 -> undefined",
            "arr[5]: item5 -> undefined",
            "arr[6]: item6 -> undefined",
            "arr[7]: item7 -> newArr[6]: item7",
            "arr[8]: item8 -> newArr[9]: item8",
            "arr[9]: item9 -> undefined",
            "arr[10]: item10 -> undefined",
            "arr[11]: item11 -> newArr[10]: item11",
            "arr[12]: item12 -> newArr[11]: item12",
            "arr[13]: item13 -> newArr[12]: item13",
            "arr[14]: item14 -> newArr[13]: item14",
        ]
    );
}
