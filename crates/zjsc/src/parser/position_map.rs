//! Mapa de posições do texto que o porte executa para o texto original do programa.
//!
//! O bun transpila todo fonte antes de entregá-lo ao JSC e remapeia as posições de `Error.stack` (e de
//! `line`/`column` do erro) pelo source map do transpilador (`SavedSourceMap`): o texto que o JSC vê é o
//! reimpresso (`Function.prototype.toString` e as mensagens com trecho do fonte citam ele), mas as posições
//! saem no fonte ORIGINAL. O golden carrega o texto reimpresso como o programa, e o gerador grava ao lado um
//! mapa de posições (`scripts/golden-prelude.js`, `positionRuns`): este módulo é o outro lado dele.
//!
//! Formato: uma sequência de corridas `(line, column, line_delta, column_delta)`, ordenada por posição, todas
//! em coordenadas do programa gravado no golden (linha e coluna de base 1). Uma posição `(l, c)` do texto
//! gravado pertence à última corrida com `(line, column) <= (l, c)` e vira `(l + line_delta, c + column_delta)`.
//! O gerador alinha os tokens do texto gravado com os do original e emite uma corrida só onde o deslocamento
//! muda (um recuo uniforme de 2 colunas é uma corrida; uma `class` içada ou um `var` desmembrado abrem outras).
//! Posição antes da primeira corrida fica como está.
//!
//! O outro formato é o de segmentos (`PositionMap::from_segments`): o mapa que o impressor do transpilador do bun
//! emite de verdade, uma entrada por token mapeado, que o `Mapping::find` do bun consulta pela maior entrada da
//! mesma linha gerada com coluna `<=` à pedida e devolve a posição ORIGINAL dela sem somar deslocamento. Esse é o
//! caminho do fonte que passou pelo transpilador do zbun (`wip/zbun/transpile`); as corridas são o do golden.

use std::rc::Rc;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Run {
    line: i64,
    column: i64,
    line_delta: i64,
    column_delta: i64,
}

/// Uma entrada do mapa do impressor: a posição gerada `(generated_line, generated_column)` é a original
/// `(original_line, original_column)`. As quatro são de base 0, como no campo `mappings` de um source map.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MapSegment {
    pub generated_line: u32,
    pub generated_column: u32,
    pub original_line: u32,
    pub original_column: u32,
}

/// O mapa de um programa; vive no `SourceProviderBase` (`SourceProvider::set_position_map`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PositionMap {
    /// Linha do porte menos linha do golden. O programa que o porte executa pode ganhar ou perder linhas em
    /// relação ao gravado (o wrapper CJS põe o cabeçalho na primeira linha e a diretiva "use strict" sai).
    line_shift: i64,
    runs: Vec<Run>,
    /// Os segmentos do impressor, ordenados por posição gerada; não vazio só no mapa de `from_segments`.
    segments: Vec<MapSegment>,
    /// O fonte original que o transpilador reimprimiu: o trecho de código que o relato de erro mostra sai dele (as linhas e
    /// colunas já são as do original), como no bun, e não do texto reimpresso que o motor executa.
    original_source: Option<Rc<str>>,
}

impl PositionMap {
    /// `numbers` é a lista plana `line, column, line_delta, column_delta, ...` do tsv; `None` se o tamanho não
    /// é múltiplo de quatro.
    pub fn new(numbers: &[i64], line_shift: i64) -> Option<PositionMap> {
        if numbers.len() % 4 != 0 {
            return None;
        }
        let runs = numbers
            .chunks_exact(4)
            .map(|chunk| Run { line: chunk[0], column: chunk[1], line_delta: chunk[2], column_delta: chunk[3] })
            .collect();
        Some(PositionMap { line_shift, runs, segments: Vec::new(), original_source: None })
    }

    /// O mapa do impressor do transpilador. `segments` vem ordenado por posição gerada, como o VLQ o entrega.
    pub fn from_segments(segments: Vec<MapSegment>) -> PositionMap {
        PositionMap { line_shift: 0, runs: Vec::new(), segments, original_source: None }
    }

    /// Guarda o fonte original de que este mapa leva as posições.
    pub fn with_original_source(mut self, source: &str) -> PositionMap {
        self.original_source = Some(Rc::from(source));
        self
    }

    /// O fonte original, quando o mapa é de um arquivo reimpresso.
    pub fn original_source(&self) -> Option<&str> {
        self.original_source.as_deref()
    }

    /// O mapa é de segmentos do impressor (`from_segments`) e se consulta por `map_exact`.
    pub fn is_exact(&self) -> bool {
        !self.segments.is_empty()
    }

    /// `Mapping::find` do bun: a posição original do maior segmento da linha gerada `line` (base 1) com coluna `<=`
    /// a `column` (base 1); `None` quando a linha não tem segmento até a coluna, e a posição crua fica.
    pub fn map_exact(&self, line: u32, column: u32) -> Option<(u32, u32)> {
        let key = (line.checked_sub(1)?, column.checked_sub(1)?);
        let after = self.segments.partition_point(|segment| (segment.generated_line, segment.generated_column) <= key);
        let segment = self.segments[after.checked_sub(1)?];
        (segment.generated_line == key.0).then(|| (segment.original_line + 1, segment.original_column + 1))
    }

    /// Posição original de `(line, column)` (base 1) do texto que o porte executa.
    pub fn map(&self, line: u32, column: u32) -> (u32, u32) {
        let golden_line = i64::from(line) - self.line_shift;
        if golden_line < 1 {
            return (line, column);
        }
        let key = (golden_line, i64::from(column));
        let covering = self.runs.partition_point(|run| (run.line, run.column) <= key);
        let Some(run) = covering.checked_sub(1).map(|index| self.runs[index]) else {
            return (line, column);
        };
        let mapped_line = golden_line + run.line_delta;
        let mapped_column = i64::from(column) + run.column_delta;
        (u32::try_from(mapped_line.max(1)).unwrap_or(line), u32::try_from(mapped_column.max(1)).unwrap_or(column))
    }
}

/// Lê um array JSON de inteiros (`[1,2,-3]`), a forma da coluna de metadados do tsv.
pub fn parse_integer_array(text: &str) -> Option<Vec<i64>> {
    let inner = text.trim().strip_prefix('[')?.strip_suffix(']')?;
    if inner.trim().is_empty() {
        return Some(Vec::new());
    }
    inner.split(',').map(|item| item.trim().parse::<i64>().ok()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uniform_indent_run() {
        let map = PositionMap::new(&[1, 1, 0, -2], 0).unwrap();
        assert_eq!(map.map(3, 8), (3, 6));
    }

    #[test]
    fn later_run_wins_and_shift_applies() {
        // O porte tem uma linha a mais que o golden (cabeçalho do wrapper).
        let map = PositionMap::new(&[1, 1, 0, -2, 5, 3, 2, 0], 1).unwrap();
        // A linha 5 do porte é a 4 do golden: só a primeira corrida a cobre, e a linha original é a do golden.
        assert_eq!(map.map(5, 9), (4, 7));
        assert_eq!(map.map(6, 3), (7, 3));
        assert_eq!(map.map(6, 4), (7, 4));
    }

    #[test]
    fn header_line_and_empty_map_are_untouched() {
        let map = PositionMap::new(&[1, 1, 0, -2], 1).unwrap();
        assert_eq!(map.map(1, 30), (1, 30));
        assert_eq!(PositionMap::new(&[], 0).unwrap().map(4, 4), (4, 4));
        assert!(PositionMap::new(&[1, 2, 3], 0).is_none());
    }

    fn segment(generated: (u32, u32), original: (u32, u32)) -> MapSegment {
        MapSegment { generated_line: generated.0, generated_column: generated.1, original_line: original.0, original_column: original.1 }
    }

    #[test]
    fn exact_lookup_returns_the_covering_segment_of_the_same_line() {
        let map = PositionMap::from_segments(vec![
            segment((0, 0), (0, 0)),
            segment((0, 8), (1, 2)),
            segment((2, 4), (5, 9)),
        ]);
        assert!(map.is_exact());
        // Base 1 dos dois lados; a coluna 8 de base 0 é a 9 de base 1.
        assert_eq!(map.map_exact(1, 1), Some((1, 1)));
        assert_eq!(map.map_exact(1, 9), Some((2, 3)));
        // Depois do último segmento da linha vale o dele, sem somar o excesso.
        assert_eq!(map.map_exact(1, 80), Some((2, 3)));
        // A linha 2 gerada não tem segmento: a posição crua fica.
        assert_eq!(map.map_exact(2, 1), None);
        assert_eq!(map.map_exact(3, 3), None);
        assert_eq!(map.map_exact(3, 5), Some((6, 10)));
        assert_eq!(map.map_exact(0, 1), None);
    }

    #[test]
    fn integer_array_parses() {
        assert_eq!(parse_integer_array("[2,1,1,0,-2]"), Some(vec![2, 1, 1, 0, -2]));
        assert_eq!(parse_integer_array("[]"), Some(Vec::new()));
        assert_eq!(parse_integer_array("[1,x]"), None);
    }
}
