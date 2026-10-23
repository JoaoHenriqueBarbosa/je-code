//! Porte de `src/vs/editor/common/model/indentationGuesser.ts`.
//!
//! O original lê o caractere com `source.getLineCharCode` quando a linha passa de 65536 unidades, para
//! não forçar o V8 a achatar uma cons-string; é só otimização (os dois caminhos dão o mesmo código), e
//! aqui a linha já vem achatada de `get_line_content`, então o porte lê sempre dela.

use je_base::char_code as cc;

use crate::piece_tree_text_buffer::PieceTreeTextBuffer;

#[derive(Clone, Copy, Debug, Default)]
struct SpacesDiffResult {
    spaces_diff: usize,
    looks_like_alignment: bool,
}

/// A diferença em espaços entre a indentação de duas linhas: `a[..a_length]` e `b[..b_length]` são as
/// indentações, `a` e `b` as linhas inteiras.
fn spaces_diff(
    a: &[u16],
    a_length: usize,
    b: &[u16],
    b_length: usize,
    result: &mut SpacesDiffResult,
) {
    result.spaces_diff = 0;
    result.looks_like_alignment = false;

    // Pode ir para os dois lados, por exemplo:
    //  - a: "\t"
    //  - b: "\t    "
    //  => conta 1 tabulação e 4 espaços

    let i = a[..a_length]
        .iter()
        .zip(&b[..b_length])
        .take_while(|(a, b)| a == b)
        .count();

    let count = |indentation: &[u16]| {
        let spaces = indentation
            .iter()
            .filter(|&&char_code| char_code == cc::Space)
            .count();
        (spaces, indentation.len() - spaces)
    };
    let (a_spaces_cnt, a_tabs_count) = count(&a[i..a_length]);
    let (b_spaces_cnt, b_tabs_count) = count(&b[i..b_length]);

    if a_spaces_cnt > 0 && a_tabs_count > 0 {
        return;
    }
    if b_spaces_cnt > 0 && b_tabs_count > 0 {
        return;
    }

    let tabs_diff = a_tabs_count.abs_diff(b_tabs_count);
    let spaces_diff = a_spaces_cnt.abs_diff(b_spaces_cnt);

    if tabs_diff == 0 {
        // A diferença de indentação pode vir de alinhamento: há quem alinhe o código, e isso não deve
        // servir de pista.
        result.spaces_diff = spaces_diff;

        if spaces_diff > 0
            && b_spaces_cnt >= 1
            && b_spaces_cnt - 1 < a.len()
            && b_spaces_cnt < b.len()
            && b[b_spaces_cnt] != cc::Space
            && a[b_spaces_cnt - 1] == cc::Space
            && a[a.len() - 1] == cc::Comma
        {
            // Parece desejo de alinhamento, por exemplo:
            // const a = b + c,
            //       d = b - c;
            result.looks_like_alignment = true;
        }
        return;
    }
    if spaces_diff % tabs_diff == 0 {
        result.spaces_diff = spaces_diff / tabs_diff;
    }
}

/// `IGuessedIndentation`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IndentationGuess {
    /// Com indentação por espaços (`insert_spaces`), quantos espaços formam um nível.
    pub tab_size: i32,
    /// Se a indentação é por espaços.
    pub insert_spaces: bool,
}

/// Prefere os palpites pares para `tab_size`, limitados a `[2, 8]`.
const ALLOWED_TAB_SIZE_GUESSES: [usize; 7] = [2, 4, 6, 8, 3, 5, 7];
/// O maior de [`ALLOWED_TAB_SIZE_GUESSES`].
const MAX_ALLOWED_TAB_SIZE_GUESS: usize = 8;

/// `guessIndentation`: adivinha a indentação olhando no máximo as 10 mil primeiras linhas.
pub fn guess_indentation(
    source: &PieceTreeTextBuffer,
    default_tab_size: i32,
    default_insert_spaces: bool,
) -> IndentationGuess {
    let lines_count = source.get_line_count().min(10000);

    // Linhas com pelo menos uma tabulação na indentação.
    let mut lines_indented_with_tabs_count = 0;
    // Linhas só com espaços na indentação.
    let mut lines_indented_with_spaces_count = 0;

    // A última linha com algum caractere que não é espaço em branco, e onde ele começa.
    let mut previous_line_text = Vec::new();
    let mut previous_line_indentation = 0;

    // A pontuação de cada `tab_size`.
    let mut spaces_diff_count = [0usize; MAX_ALLOWED_TAB_SIZE_GUESS + 1];
    let mut tmp = SpacesDiffResult::default();

    for line_number in 1..=lines_count as i32 {
        let current_line_text = source.get_line_content(line_number).units().to_vec();

        let mut current_spaces_count = 0;
        let mut current_tabs_count = 0;
        // Onde a linha tem o primeiro caractere que não é espaço em branco, se tiver.
        let mut current_line_indentation = None;
        for (j, &char_code) in current_line_text.iter().enumerate() {
            if char_code == cc::Tab {
                current_tabs_count += 1;
            } else if char_code == cc::Space {
                current_spaces_count += 1;
            } else {
                current_line_indentation = Some(j);
                break;
            }
        }

        // Ignora as linhas vazias ou só com espaço em branco.
        let Some(current_line_indentation) = current_line_indentation else {
            continue;
        };

        if current_tabs_count > 0 {
            lines_indented_with_tabs_count += 1;
        } else if current_spaces_count > 1 {
            lines_indented_with_spaces_count += 1;
        }

        spaces_diff(
            &previous_line_text,
            previous_line_indentation,
            &current_line_text,
            current_line_indentation,
            &mut tmp,
        );

        if tmp.looks_like_alignment {
            // Com `default_insert_spaces` e a diferença igual a `default_tab_size`, a linha pode contar
            // como indentação válida:
            //
            // - item1
            //   - item2
            //
            // senão ela fica de fora:
            //
            // const a = 1,
            //       b = 2;
            if !(default_insert_spaces && usize::try_from(default_tab_size) == Ok(tmp.spaces_diff))
            {
                continue;
            }
        }

        if let Some(score) = spaces_diff_count.get_mut(tmp.spaces_diff) {
            *score += 1;
        }

        previous_line_text = current_line_text;
        previous_line_indentation = current_line_indentation;
    }

    let mut insert_spaces = default_insert_spaces;
    if lines_indented_with_tabs_count != lines_indented_with_spaces_count {
        insert_spaces = lines_indented_with_tabs_count < lines_indented_with_spaces_count;
    }

    let mut tab_size = default_tab_size;

    // Só adivinha `tab_size` quando a indentação é por espaços.
    if insert_spaces {
        let mut tab_size_score = 0;
        for possible_tab_size in ALLOWED_TAB_SIZE_GUESSES {
            let possible_tab_size_score = spaces_diff_count[possible_tab_size];
            if possible_tab_size_score > tab_size_score {
                tab_size_score = possible_tab_size_score;
                tab_size = possible_tab_size as i32;
            }
        }

        // 2 ganha de 4 só com pelo menos 2/3 das ocorrências de 4. Ajuda a reconhecer a indentação de 2
        // espaços em arquivos como YAML, onde o aninhamento fundo produz algumas diferenças de 4, sem
        // deixar de preferir 4 quando ele predomina.
        if tab_size == 4
            && spaces_diff_count[4] > 0
            && spaces_diff_count[2] > 0
            && 3 * spaces_diff_count[2] >= 2 * spaces_diff_count[4]
        {
            tab_size = 2;
        }
    }

    IndentationGuess {
        tab_size,
        insert_spaces,
    }
}
