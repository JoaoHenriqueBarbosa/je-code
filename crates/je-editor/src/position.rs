//! Porte de `src/vs/editor/common/core/position.ts`.
//!
//! Diferenças de forma, nenhuma de comportamento: `IPosition` e `Position` são o mesmo tipo (o objeto
//! literal `{ lineNumber, column }` do TypeScript vira um `Position`), e `equals`/`equals` estático viram
//! `PartialEq` (`Option<Position>` cobre o `null` do original). `clone`, `lift` e `toJSON` não existem:
//! `Position` é `Copy`.

use std::fmt;

/// Uma posição no editor: linha e coluna começam em 1 e a coluna conta unidades UTF-16.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Position {
    /// Número da linha, a partir de 1.
    pub line_number: i32,
    /// Coluna, a partir de 1.
    pub column: i32,
}

impl Position {
    pub const fn new(line_number: i32, column: i32) -> Self {
        Self { line_number, column }
    }

    /// `with(newLineNumber, newColumn)`.
    pub fn with(self, new_line_number: i32, new_column: i32) -> Position {
        Position::new(new_line_number, new_column)
    }

    /// `delta(deltaLineNumber, deltaColumn)`: soma, com piso em 1.
    pub fn delta(self, delta_line_number: i32, delta_column: i32) -> Position {
        self.with((self.line_number + delta_line_number).max(1), (self.column + delta_column).max(1))
    }

    /// `isBefore`.
    pub fn is_before(&self, other: &Position) -> bool {
        if self.line_number < other.line_number {
            return true;
        }
        if other.line_number < self.line_number {
            return false;
        }
        self.column < other.column
    }

    /// `isBeforeOrEqual`.
    pub fn is_before_or_equal(&self, other: &Position) -> bool {
        if self.line_number < other.line_number {
            return true;
        }
        if other.line_number < self.line_number {
            return false;
        }
        self.column <= other.column
    }

    /// `Position.compare`: negativo, zero ou positivo.
    pub fn compare(a: &Position, b: &Position) -> i32 {
        if a.line_number == b.line_number {
            return a.column - b.column;
        }
        a.line_number - b.line_number
    }
}

impl fmt::Display for Position {
    /// `toString()`: `(linha,coluna)`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "({},{})", self.line_number, self.column)
    }
}
