//! Porte de `src/vs/base/common/jsonErrorMessages.ts`.

use crate::json::ParseErrorCode;
use crate::nls::localize;

/// `getParseErrorMessage`.
pub fn parse_error_message(error_code: ParseErrorCode) -> String {
    let (key, message) = match error_code {
        ParseErrorCode::InvalidSymbol => ("error.invalidSymbol", "Invalid symbol"),
        ParseErrorCode::InvalidNumberFormat => ("error.invalidNumberFormat", "Invalid number format"),
        ParseErrorCode::PropertyNameExpected => ("error.propertyNameExpected", "Property name expected"),
        ParseErrorCode::ValueExpected => ("error.valueExpected", "Value expected"),
        ParseErrorCode::ColonExpected => ("error.colonExpected", "Colon expected"),
        ParseErrorCode::CommaExpected => ("error.commaExpected", "Comma expected"),
        ParseErrorCode::CloseBraceExpected => ("error.closeBraceExpected", "Closing brace expected"),
        ParseErrorCode::CloseBracketExpected => ("error.closeBracketExpected", "Closing bracket expected"),
        ParseErrorCode::EndOfFileExpected => ("error.endOfFileExpected", "End of file expected"),
        _ => return String::new(),
    };
    localize(key, message, &[])
}
