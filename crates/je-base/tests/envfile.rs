//! Porte de `src/vs/base/test/common/envfile.test.ts` (suíte `parseEnvFile`), caso a caso e na mesma ordem.
//!
//! O `ensureNoDisposablesAreLeakedInTestSuite` não entra, porque o `envfile` não cria `IDisposable`.

use je_base::envfile::parse_env_file;

/*
Test cases from https://github.com/motdotla/dotenv/blob/master/tests/.env

	Copyright (c) 2015, Scott Motte
	All rights reserved.

	Redistribution and use in source and binary forms, with or without
	modification, are permitted provided that the following conditions are met:

	* Redistributions of source code must retain the above copyright notice, this
		list of conditions and the following disclaimer.

	* Redistributions in binary form must reproduce the above copyright notice,
		this list of conditions and the following disclaimer in the documentation
		and/or other materials provided with the distribution.

	THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS "AS IS"
	AND ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE
	IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE
	DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT HOLDER OR CONTRIBUTORS BE LIABLE
	FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL
	DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR
	SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER
	CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY,
	OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE
	OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.
*/

const EXAMPLE: &str = "
BASIC=basic

# previous line intentionally left blank
AFTER_LINE=after_line
EMPTY=
EMPTY_SINGLE_QUOTES=''
EMPTY_DOUBLE_QUOTES=\"\"
EMPTY_BACKTICKS=``
SINGLE_QUOTES='single_quotes'
SINGLE_QUOTES_SPACED='    single quotes    '
DOUBLE_QUOTES=\"double_quotes\"
DOUBLE_QUOTES_SPACED=\"    double quotes    \"
DOUBLE_QUOTES_INSIDE_SINGLE='double \"quotes\" work inside single quotes'
DOUBLE_QUOTES_WITH_NO_SPACE_BRACKET=\"{ port: $MONGOLAB_PORT}\"
SINGLE_QUOTES_INSIDE_DOUBLE=\"single 'quotes' work inside double quotes\"
BACKTICKS_INSIDE_SINGLE='`backticks` work inside single quotes'
BACKTICKS_INSIDE_DOUBLE=\"`backticks` work inside double quotes\"
BACKTICKS=`backticks`
BACKTICKS_SPACED=`    backticks    `
DOUBLE_QUOTES_INSIDE_BACKTICKS=`double \"quotes\" work inside backticks`
SINGLE_QUOTES_INSIDE_BACKTICKS=`single 'quotes' work inside backticks`
DOUBLE_AND_SINGLE_QUOTES_INSIDE_BACKTICKS=`double \"quotes\" and single 'quotes' work inside backticks`
EXPAND_NEWLINES=\"expand\\nnew\\nlines\"
DONT_EXPAND_UNQUOTED=dontexpand\\nnewlines
DONT_EXPAND_SQUOTED='dontexpand\\nnewlines'
# COMMENTS=work
INLINE_COMMENTS=inline comments # work #very #well
INLINE_COMMENTS_SINGLE_QUOTES='inline comments outside of #singlequotes' # work
INLINE_COMMENTS_DOUBLE_QUOTES=\"inline comments outside of #doublequotes\" # work
INLINE_COMMENTS_BACKTICKS=`inline comments outside of #backticks` # work
INLINE_COMMENTS_SPACE=inline comments start with a#number sign. no space required.
EQUAL_SIGNS=equals==
RETAIN_INNER_QUOTES={\"foo\": \"bar\"}
RETAIN_INNER_QUOTES_AS_STRING='{\"foo\": \"bar\"}'
RETAIN_INNER_QUOTES_AS_BACKTICKS=`{\"foo\": \"bar's\"}`
TRIM_SPACE_FROM_UNQUOTED=    some spaced out string
USERNAME=therealnerdybeast@example.tld
		SPACED_KEY = parsed
";

#[test]
fn parses() {
    let parsed = parse_env_file(EXAMPLE);
    assert_eq!(parsed.get("BASIC"), Some("basic"));
    assert_eq!(parsed.get("AFTER_LINE"), Some("after_line"));
    assert_eq!(parsed.get("EMPTY"), Some(""));
    assert_eq!(parsed.get("EMPTY_SINGLE_QUOTES"), Some(""));
    assert_eq!(parsed.get("EMPTY_DOUBLE_QUOTES"), Some(""));
    assert_eq!(parsed.get("EMPTY_BACKTICKS"), Some(""));
    assert_eq!(parsed.get("SINGLE_QUOTES"), Some("single_quotes"));
    assert_eq!(parsed.get("SINGLE_QUOTES_SPACED"), Some("    single quotes    "));
    assert_eq!(parsed.get("DOUBLE_QUOTES"), Some("double_quotes"));
    assert_eq!(parsed.get("DOUBLE_QUOTES_SPACED"), Some("    double quotes    "));
    assert_eq!(parsed.get("DOUBLE_QUOTES_INSIDE_SINGLE"), Some("double \"quotes\" work inside single quotes"));
    assert_eq!(parsed.get("DOUBLE_QUOTES_WITH_NO_SPACE_BRACKET"), Some("{ port: $MONGOLAB_PORT}"));
    assert_eq!(parsed.get("SINGLE_QUOTES_INSIDE_DOUBLE"), Some("single 'quotes' work inside double quotes"));
    assert_eq!(parsed.get("BACKTICKS_INSIDE_SINGLE"), Some("`backticks` work inside single quotes"));
    assert_eq!(parsed.get("BACKTICKS_INSIDE_DOUBLE"), Some("`backticks` work inside double quotes"));
    assert_eq!(parsed.get("BACKTICKS"), Some("backticks"));
    assert_eq!(parsed.get("BACKTICKS_SPACED"), Some("    backticks    "));
    assert_eq!(parsed.get("DOUBLE_QUOTES_INSIDE_BACKTICKS"), Some("double \"quotes\" work inside backticks"));
    assert_eq!(parsed.get("SINGLE_QUOTES_INSIDE_BACKTICKS"), Some("single 'quotes' work inside backticks"));
    assert_eq!(
        parsed.get("DOUBLE_AND_SINGLE_QUOTES_INSIDE_BACKTICKS"),
        Some("double \"quotes\" and single 'quotes' work inside backticks")
    );
    assert_eq!(parsed.get("EXPAND_NEWLINES"), Some("expand\nnew\nlines"));
    assert_eq!(parsed.get("DONT_EXPAND_UNQUOTED"), Some("dontexpand\\nnewlines"));
    assert_eq!(parsed.get("DONT_EXPAND_SQUOTED"), Some("dontexpand\\nnewlines"));
    assert_eq!(parsed.get("COMMENTS"), None);
    assert_eq!(parsed.get("INLINE_COMMENTS"), Some("inline comments"));
    assert_eq!(parsed.get("INLINE_COMMENTS_SINGLE_QUOTES"), Some("inline comments outside of #singlequotes"));
    assert_eq!(parsed.get("INLINE_COMMENTS_DOUBLE_QUOTES"), Some("inline comments outside of #doublequotes"));
    assert_eq!(parsed.get("INLINE_COMMENTS_BACKTICKS"), Some("inline comments outside of #backticks"));
    assert_eq!(parsed.get("INLINE_COMMENTS_SPACE"), Some("inline comments start with a"));
    assert_eq!(parsed.get("EQUAL_SIGNS"), Some("equals=="));
    assert_eq!(parsed.get("RETAIN_INNER_QUOTES"), Some("{\"foo\": \"bar\"}"));
    assert_eq!(parsed.get("RETAIN_INNER_QUOTES_AS_STRING"), Some("{\"foo\": \"bar\"}"));
    assert_eq!(parsed.get("RETAIN_INNER_QUOTES_AS_BACKTICKS"), Some("{\"foo\": \"bar's\"}"));
    assert_eq!(parsed.get("TRIM_SPACE_FROM_UNQUOTED"), Some("some spaced out string"));
    assert_eq!(parsed.get("USERNAME"), Some("therealnerdybeast@example.tld"));
    assert_eq!(parsed.get("SPACED_KEY"), Some("parsed"));
    let payload = parse_env_file("BUFFER=true");
    assert_eq!(payload.get("BUFFER"), Some("true"));
    let expected_payload = [("SERVER", "localhost"), ("PASSWORD", "password"), ("DB", "tests")];
    let r_payload = parse_env_file("SERVER=localhost\rPASSWORD=password\rDB=tests\r");
    assert_eq!(r_payload.iter().collect::<Vec<_>>(), expected_payload);
    let n_payload = parse_env_file("SERVER=localhost\nPASSWORD=password\nDB=tests\n");
    assert_eq!(n_payload.iter().collect::<Vec<_>>(), expected_payload);
    let rn_payload = parse_env_file("SERVER=localhost\r\nPASSWORD=password\r\nDB=tests\r\n");
    assert_eq!(rn_payload.iter().collect::<Vec<_>>(), expected_payload);
}
