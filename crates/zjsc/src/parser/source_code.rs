//! Tradução de `parser/SourceCode.h`.
//!
//! `SourceCode` herda de `UnlinkedSourceCode` no C++; aqui ela contém uma e faz `Deref` para ela, de
//! modo que `view`, `length`, `hash`, `start_offset` etc. valem direto. O `provider()` da
//! `SourceCode` devolve o ponteiro (possivelmente nulo) e esconde o da base, como no C++.
//! `Default` é manual: o construtor padrão do C++ põe `beforeFirst()` na linha e na coluna.

use std::ops::Deref;
use std::rc::Rc;

use crate::parser::source_provider::{SourceID, SourceProvider, SourceProviderSourceType, StringSourceProvider, NULL_ID};
use crate::parser::source_tainted_origin::SourceTaintedOrigin;
use crate::parser::unlinked_source_code::UnlinkedSourceCode;
use crate::runtime::source_origin::SourceOrigin;
use crate::wtf::text::text_position::{OrdinalNumber, TextPosition};
use crate::wtf::text::wtf_string::String as WtfString;

/// `class SourceCode`.
#[derive(Clone, PartialEq, Eq)]
pub struct SourceCode {
    unlinked: UnlinkedSourceCode,
    first_line: OrdinalNumber,
    start_column: OrdinalNumber,
}

impl Default for SourceCode {
    /// `SourceCode()`.
    fn default() -> SourceCode {
        SourceCode {
            unlinked: UnlinkedSourceCode::default(),
            first_line: OrdinalNumber::before_first(),
            start_column: OrdinalNumber::before_first(),
        }
    }
}

impl Deref for SourceCode {
    type Target = UnlinkedSourceCode;

    fn deref(&self) -> &UnlinkedSourceCode {
        &self.unlinked
    }
}

impl SourceCode {
    /// `SourceCode(Ref<SourceProvider>&&)`: linha e coluna ficam no `OrdinalNumber()` padrão.
    pub fn from_provider(provider: Rc<dyn SourceProvider>) -> SourceCode {
        SourceCode {
            unlinked: UnlinkedSourceCode::from_provider(provider),
            first_line: OrdinalNumber::default(),
            start_column: OrdinalNumber::default(),
        }
    }

    /// `SourceCode(Ref<SourceProvider>&&, int firstLine, int startColumn)`.
    pub fn with_position(provider: Rc<dyn SourceProvider>, first_line: i32, start_column: i32) -> SourceCode {
        SourceCode {
            unlinked: UnlinkedSourceCode::from_provider(provider),
            first_line: OrdinalNumber::from_one_based_int(first_line.max(1)),
            start_column: OrdinalNumber::from_one_based_int(start_column.max(1)),
        }
    }

    /// `SourceCode(RefPtr<SourceProvider>&&, int startOffset, int endOffset, int firstLine, int startColumn)`.
    pub fn with_offsets(
        provider: Option<Rc<dyn SourceProvider>>,
        start_offset: i32,
        end_offset: i32,
        first_line: i32,
        start_column: i32,
    ) -> SourceCode {
        SourceCode {
            unlinked: UnlinkedSourceCode::with_offsets(provider, start_offset, end_offset),
            first_line: OrdinalNumber::from_one_based_int(first_line.max(1)),
            start_column: OrdinalNumber::from_one_based_int(start_column.max(1)),
        }
    }

    /// `firstLine()`.
    pub fn first_line(&self) -> OrdinalNumber {
        self.first_line
    }

    /// `startColumn()`.
    pub fn start_column(&self) -> OrdinalNumber {
        self.start_column
    }

    /// `memoryCost()` (Bun).
    pub fn memory_cost(&self) -> usize {
        match &self.unlinked.provider {
            Some(provider) => provider.memory_cost(),
            None => 0,
        }
    }

    /// `providerID()`.
    pub fn provider_id(&self) -> SourceID {
        match &self.unlinked.provider {
            None => NULL_ID,
            Some(provider) => provider.as_id(),
        }
    }

    /// `provider()`: o ponteiro, nulo como `None`.
    pub fn provider(&self) -> Option<&Rc<dyn SourceProvider>> {
        self.unlinked.provider.as_ref()
    }

    /// `subExpression(openBrace, closeBrace, firstLine, startColumn)`.
    pub fn sub_expression(&self, open_brace: u32, close_brace: u32, first_line: i32, start_column: i32) -> SourceCode {
        let start_column = start_column + 1; // Convert to base 1.
        SourceCode::with_offsets(
            self.unlinked.provider.clone(),
            open_brace as i32,
            close_brace.wrapping_add(1) as i32,
            first_line,
            start_column,
        )
    }
}

/// `makeSource(source, sourceOrigin, sourceTaintedOrigin, filename, startPosition, sourceType)`. Os
/// argumentos padrão do C++ (`String()`, `TextPosition()`, `Program`) são passados por quem chama.
pub fn make_source(
    source: &WtfString,
    source_origin: &SourceOrigin,
    source_tainted_origin: SourceTaintedOrigin,
    filename: WtfString,
    start_position: TextPosition,
    source_type: SourceProviderSourceType,
) -> SourceCode {
    SourceCode::with_position(
        StringSourceProvider::create(source, source_origin, filename, source_tainted_origin, start_position, source_type),
        start_position.line.one_based_int(),
        start_position.column.one_based_int(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make(source: &str) -> SourceCode {
        make_source(
            &WtfString::from_latin1(source.as_bytes()),
            &SourceOrigin::default(),
            SourceTaintedOrigin::Untainted,
            WtfString::default(),
            TextPosition::default(),
            SourceProviderSourceType::Program,
        )
    }

    #[test]
    fn defaults() {
        let empty = SourceCode::default();
        assert!(empty.is_null());
        assert_eq!(empty.first_line(), OrdinalNumber::before_first());
        assert_eq!(empty.provider_id(), NULL_ID);
        assert!(empty.provider().is_none());
    }

    #[test]
    fn make_source_positions() {
        let code = make("a + b");
        assert_eq!(code.first_line().one_based_int(), 1);
        assert_eq!(code.start_column().one_based_int(), 1);
        assert_eq!(code.length(), 5);
        assert!(code.provider().is_some());
        assert!(code.provider_id() > NULL_ID);
    }

    #[test]
    fn sub_expression_and_equality() {
        let code = make("f(x) { y }");
        let sub = code.sub_expression(5, 9, 1, 5);
        assert_eq!(sub.view(), WtfString::from_latin1(b"{ y }"));
        assert_eq!(sub.start_column().one_based_int(), 6);
        assert!(sub == code.sub_expression(5, 9, 1, 5));
        assert!(sub != code);
    }
}
