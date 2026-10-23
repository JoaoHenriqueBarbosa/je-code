//! Porte de `src/vs/editor/test/common/modes/linkComputer.test.ts`, caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` não tem equivalente aqui: o detector de links não cria
//!   nenhum descartável.
//! - `SimpleLinkComputerTarget` vira uma struct com as linhas em [`JsString`].
//! - `assert.deepStrictEqual(r, [{ range, url }])` vira a comparação de uma lista de um elemento, com o
//!   `url` lido como texto UTF-16.
//! - `charAt` e `substring` do original operam em unidades UTF-16, então o teste trabalha em `Vec<u16>`.

use je_base::JsString;
use je_editor::languages::{ILink, LinkUrl};
use je_editor::link_computer::{ILinkComputerTarget, compute_links};
use je_editor::range::Range;

struct SimpleLinkComputerTarget {
    lines: Vec<JsString>,
}

impl SimpleLinkComputerTarget {
    fn new(lines: &[&str]) -> Self {
        Self { lines: lines.iter().map(|line| JsString::from(*line)).collect() }
    }
}

impl ILinkComputerTarget for SimpleLinkComputerTarget {
    fn get_line_count(&self) -> i32 {
        self.lines.len() as i32
    }

    fn get_line_content(&self, line_number: i32) -> JsString {
        self.lines[(line_number - 1) as usize].clone()
    }
}

fn my_compute_links(lines: &[&str]) -> Vec<ILink> {
    let target = SimpleLinkComputerTarget::new(lines);
    compute_links(Some(&target))
}

fn assert_link(text: &str, extracted_link: &str) {
    let extracted: Vec<u16> = extracted_link.encode_utf16().collect();
    let is_blank = |unit: u16| unit == b' ' as u16 || unit == b'\t' as u16;

    let mut start_column: i32 = 0;
    let mut end_column: i32 = 0;

    for (i, &unit) in extracted.iter().enumerate() {
        if !is_blank(unit) {
            start_column = i as i32 + 1;
            break;
        }
    }

    for (i, &unit) in extracted.iter().enumerate().rev() {
        if !is_blank(unit) {
            end_column = i as i32 + 2;
            break;
        }
    }

    let r = my_compute_links(&[text]);
    assert_eq!(r.len(), 1, "links de {text:?}");
    assert_eq!(
        r[0].range,
        Range {
            start_line_number: 1,
            start_column,
            end_line_number: 1,
            end_column,
        }
    );
    let expected_url = &extracted[(start_column - 1) as usize..(end_column - 1) as usize];
    match &r[0].url {
        Some(LinkUrl::Text(url)) => assert_eq!(url.units(), expected_url, "url de {text:?}"),
        _ => panic!("o url deveria ser texto"),
    }
    assert!(r[0].tooltip.is_none());
}

#[test]
fn null_model() {
    let r = compute_links(None);
    assert!(r.is_empty());
}

#[test]
fn parsing() {
    assert_link("x = \"http://foo.bar\";", "     http://foo.bar  ");

    assert_link("x = (http://foo.bar);", "     http://foo.bar  ");

    assert_link("x = [http://foo.bar];", "     http://foo.bar  ");

    assert_link("x = 'http://foo.bar';", "     http://foo.bar  ");

    assert_link("x =  http://foo.bar ;", "     http://foo.bar  ");

    assert_link("x = <http://foo.bar>;", "     http://foo.bar  ");

    assert_link("x = {http://foo.bar};", "     http://foo.bar  ");

    assert_link("(see http://foo.bar)", "     http://foo.bar  ");
    assert_link("[see http://foo.bar]", "     http://foo.bar  ");
    assert_link("{see http://foo.bar}", "     http://foo.bar  ");
    assert_link("<see http://foo.bar>", "     http://foo.bar  ");
    assert_link("<url>http://mylink.com</url>", "     http://mylink.com      ");
    assert_link(
        "// Click here to learn more. https://go.microsoft.com/fwlink/?LinkID=513275&clcid=0x409",
        "                             https://go.microsoft.com/fwlink/?LinkID=513275&clcid=0x409",
    );
    assert_link(
        "// Click here to learn more. https://msdn.microsoft.com/en-us/library/windows/desktop/aa365247(v=vs.85).aspx",
        "                             https://msdn.microsoft.com/en-us/library/windows/desktop/aa365247(v=vs.85).aspx",
    );
    assert_link(
        "// https://github.com/projectkudu/kudu/blob/master/Kudu.Core/Scripts/selectNodeVersion.js",
        "   https://github.com/projectkudu/kudu/blob/master/Kudu.Core/Scripts/selectNodeVersion.js",
    );
    assert_link(
        "<!-- !!! Do not remove !!!   WebContentRef(link:https://go.microsoft.com/fwlink/?LinkId=166007, area:Admin, updated:2015, nextUpdate:2016, tags:SqlServer)   !!! Do not remove !!! -->",
        "                                                https://go.microsoft.com/fwlink/?LinkId=166007                                                                                        ",
    );
    assert_link(
        "For instructions, see https://go.microsoft.com/fwlink/?LinkId=166007.</value>",
        "                      https://go.microsoft.com/fwlink/?LinkId=166007         ",
    );
    assert_link(
        "For instructions, see https://msdn.microsoft.com/en-us/library/windows/desktop/aa365247(v=vs.85).aspx.</value>",
        "                      https://msdn.microsoft.com/en-us/library/windows/desktop/aa365247(v=vs.85).aspx         ",
    );
    assert_link(
        "x = \"https://en.wikipedia.org/wiki/Zürich\";",
        "     https://en.wikipedia.org/wiki/Zürich  ",
    );
    assert_link(
        "請參閱 http://go.microsoft.com/fwlink/?LinkId=761051。",
        "    http://go.microsoft.com/fwlink/?LinkId=761051 ",
    );
    assert_link(
        "（請參閱 http://go.microsoft.com/fwlink/?LinkId=761051）",
        "     http://go.microsoft.com/fwlink/?LinkId=761051 ",
    );

    assert_link("x = \"file:///foo.bar\";", "     file:///foo.bar  ");
    assert_link("x = \"file://c:/foo.bar\";", "     file://c:/foo.bar  ");

    assert_link("x = \"file://shares/foo.bar\";", "     file://shares/foo.bar  ");

    assert_link("x = \"file://shäres/foo.bar\";", "     file://shäres/foo.bar  ");
    assert_link("Some text, then http://www.bing.com.", "                http://www.bing.com ");
    assert_link(
        "let url = `http://***/_api/web/lists/GetByTitle('Teambuildingaanvragen')/items`;",
        "           http://***/_api/web/lists/GetByTitle('Teambuildingaanvragen')/items  ",
    );
}

#[test]
fn issue_7855() {
    assert_link(
        "7. At this point, ServiceMain has been called.  There is no functionality presently in ServiceMain, but you can consult the [MSDN documentation](https://msdn.microsoft.com/en-us/library/windows/desktop/ms687414(v=vs.85).aspx) to add functionality as desired!",
        "                                                                                                                                                 https://msdn.microsoft.com/en-us/library/windows/desktop/ms687414(v=vs.85).aspx                                  ",
    );
}

#[test]
fn issue_62278_ctrl_click_to_follow_link_for_ipv6_urls() {
    assert_link(
        "let x = \"http://[::1]:5000/connect/token\"",
        "         http://[::1]:5000/connect/token  ",
    );
}

#[test]
fn issue_70254_bold_links_dont_open_in_markdown_file_using_editor_mode_with_ctrl_click() {
    assert_link(
        "2. Navigate to **https://portal.azure.com**",
        "                 https://portal.azure.com  ",
    );
}

#[test]
fn issue_86358_url_wrong_recognition_pattern() {
    assert_link(
        "POST|https://portal.azure.com|2019-12-05|",
        "     https://portal.azure.com            ",
    );
}

#[test]
fn issue_67022_space_as_end_of_hyperlink_isnt_always_good_idea() {
    assert_link(
        "aa  https://foo.bar/[this is foo site]  aa",
        "    https://foo.bar/[this is foo site]    ",
    );
}

#[test]
fn issue_100353_link_detection_stops_at_double_byte_ampersand() {
    assert_link(
        "aa  http://tree-mark.chips.jp/レーズン＆ベリーミックス  aa",
        "    http://tree-mark.chips.jp/レーズン＆ベリーミックス    ",
    );
}

#[test]
fn issue_121438_link_detection_stops_at_lenticular_brackets() {
    assert_link(
        "aa  https://zh.wikipedia.org/wiki/【我推的孩子】 aa",
        "    https://zh.wikipedia.org/wiki/【我推的孩子】   ",
    );
}

#[test]
fn issue_121438_link_detection_stops_at_double_angle_brackets() {
    assert_link(
        "aa  https://zh.wikipedia.org/wiki/《新青年》编辑部旧址 aa",
        "    https://zh.wikipedia.org/wiki/《新青年》编辑部旧址   ",
    );
}

#[test]
fn issue_121438_link_detection_stops_at_curly_double_quotes() {
    assert_link(
        "aa  https://zh.wikipedia.org/wiki/“常凯申”误译事件 aa",
        "    https://zh.wikipedia.org/wiki/“常凯申”误译事件   ",
    );
}

#[test]
fn issue_150905_colon_after_bare_hyperlink_is_treated_as_its_part() {
    assert_link(
        "https://site.web/page.html: blah blah blah",
        "https://site.web/page.html                ",
    );
}

// Removido no original por causa do #156875:
// issue #151631: Link parsing stoped where comments include a single quote.

#[test]
fn issue_156875_links_include_quotes() {
    assert_link(
        "\"This file has been converted from https://github.com/jeff-hykin/better-c-syntax/blob/master/autogenerated/c.tmLanguage.json\",",
        "                                   https://github.com/jeff-hykin/better-c-syntax/blob/master/autogenerated/c.tmLanguage.json  ",
    );
}

#[test]
fn issue_225513_cmd_click_works_on_jsdoc_link_url_linktext_format() {
    assert_link(
        " * {@link https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Promise/withResolvers|Promise.withResolvers}",
        "          https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Promise/withResolvers                       ",
    );
}
