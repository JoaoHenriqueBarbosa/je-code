//! Porte de `src/vs/workbench/contrib/testing/common/testExplorerFilterState.ts`: o estado do filtro do explorador de
//! testes (o texto digitado, a lista de globs, as etiquetas incluídas e excluídas e os termos especiais como `@doc`).
//!
//! Divergências:
//! - **`ITestExplorerFilterState`** é o trait de mesmo nome, com as propriedades como métodos (`text`, `fuzzy`, `reveal`
//!   devolvem o objeto observável; `globList`, `includeTags` e `excludeTags` devolvem cópias dos valores atuais).
//! - **`const enum TestFilterTerm`** é o enum [`TestFilterTerm`], com [`TestFilterTerm::as_str`] para o texto de cada
//!   termo (`@failed`, `@executed`, `@doc`, `@openedFiles`, `@hidden`).
//! - **`matchAll(tagRe)`** e o índice em UTF-16: a expressão `/!?@([^ ,:]+)/g` roda sobre as unidades UTF-16 do texto
//!   ([`je_base::js_regexp::match_all`]), e `text[nextIndex]` é a unidade nesse índice, como no JavaScript.
//! - **`Set` de etiquetas** é um `IndexSet`: a ordem de inserção é a do `Set` do JavaScript (o teste de aspas lê
//!   `[...t.includeTags]` e depende dela).
//! - **`@IStorageService`** é o `Rc<dyn StorageService>`.

use std::cell::RefCell;
use std::rc::Rc;

use indexmap::IndexSet;
use je_base::JsString;
use je_base::js_string;
use je_base::event::{Emitter, Event};
use je_base::glob::split_glob_aware;
use je_base::js_regexp::match_all;
use je_base::lifecycle::{Disposable, IDisposable};
use je_base::observable::{ISettableObservable, Settable, observable_value};
use je_instantiation::service_id;
use je_platform::storage::{StorageScope, StorageService, StorageTarget};
use zyarr::RegExp;

use super::observable_value::MutableObservableValue;
use super::stored_value::{StoredValue, StoredValueOptions};
use super::test_types::namespace_test_tag;

/// `TestFilterTerm`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TestFilterTerm {
    Failed,
    Executed,
    CurrentDoc,
    OpenedFiles,
    Hidden,
}

impl TestFilterTerm {
    /// O texto do termo no campo de filtro.
    pub fn as_str(self) -> &'static str {
        match self {
            TestFilterTerm::Failed => "@failed",
            TestFilterTerm::Executed => "@executed",
            TestFilterTerm::CurrentDoc => "@doc",
            TestFilterTerm::OpenedFiles => "@openedFiles",
            TestFilterTerm::Hidden => "@hidden",
        }
    }

    /// `allTestFilterTerms.includes(tag)`: o termo cujo texto é `tag`, se houver.
    pub fn parse(tag: &str) -> Option<TestFilterTerm> {
        ALL_TEST_FILTER_TERMS.into_iter().find(|term| term.as_str() == tag)
    }
}

const ALL_TEST_FILTER_TERMS: [TestFilterTerm; 5] =
    [TestFilterTerm::Failed, TestFilterTerm::Executed, TestFilterTerm::CurrentDoc, TestFilterTerm::OpenedFiles, TestFilterTerm::Hidden];

/// Um item da `globList`: `{ include, text }`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GlobFilter {
    pub include: bool,
    pub text: String,
}

/// `ITestExplorerFilterState`.
pub trait ITestExplorerFilterState {
    /// `text`: current filter text.
    fn text(&self) -> Rc<MutableObservableValue<String>>;

    /// `reveal`: test ID the user wants to reveal in the explorer.
    fn reveal(&self) -> ISettableObservable<Option<String>>;

    /// `onDidSelectTestInExplorer`: a test was selected in the explorer.
    fn on_did_select_test_in_explorer(&self) -> Event<Option<String>>;

    /// `onDidRequestInputFocus`: event that fires when [`ITestExplorerFilterState::focus_input`] is invoked.
    fn on_did_request_input_focus(&self) -> Event<()>;

    /// `globList`: glob list to filter for based on the text.
    fn glob_list(&self) -> Vec<GlobFilter>;

    /// `includeTags`: the user requested to filter including tags.
    fn include_tags(&self) -> IndexSet<String>;

    /// `excludeTags`: the user requested to filter excluding tags.
    fn exclude_tags(&self) -> IndexSet<String>;

    /// `fuzzy`: whether fuzzy searching is enabled.
    fn fuzzy(&self) -> Rc<MutableObservableValue<bool>>;

    /// `focusInput()`: focuses the filter input in the test explorer view.
    fn focus_input(&self);

    /// `setText(text)`: replaces the filter text.
    fn set_text(&self, text: &str);

    /// `isFilteringFor(term)`: whether the text is filtering for a special term.
    fn is_filtering_for(&self, term: TestFilterTerm) -> bool;

    /// `toggleFilteringFor(term, shouldFilter?)`: whether the text includes a special filter term.
    fn toggle_filtering_for(&self, term: TestFilterTerm, should_filter: Option<bool>);

    /// `didSelectTestInExplorer(testId)`: called when a test in the test explorer is selected.
    fn did_select_test_in_explorer(&self, test_id: &str);
}

service_id!(pub I_TEST_EXPLORER_FILTER_STATE: dyn ITestExplorerFilterState = "testingFilterState");

thread_local! {
    /// `tagRe = /!?@([^ ,:]+)/g`.
    static TAG_RE: RegExp = {
        let pattern: Vec<u16> = "!?@([^ ,:]+)".encode_utf16().collect();
        let flags: Vec<u16> = "g".encode_utf16().collect();
        RegExp::new(&pattern, &flags).expect("a expressão das etiquetas é válida")
    };
}

/// `str.trim()`.
fn js_trim(text: &str) -> String {
    let units: Vec<u16> = text.encode_utf16().collect();
    String::from_utf16_lossy(js_string::trim(&units))
}

/// `trimExtraWhitespace(str)`: `str.replace(/\s\s+/g, ' ').trim()`.
fn trim_extra_whitespace(text: &str) -> String {
    let units: Vec<u16> = text.encode_utf16().collect();
    let mut out: Vec<u16> = Vec::with_capacity(units.len());
    let mut index = 0;
    while index < units.len() {
        let run = units[index..].iter().take_while(|&&unit| js_string::is_js_whitespace(unit)).count();
        if run >= 2 {
            out.push(u16::from(b' '));
            index += run;
        } else {
            out.push(units[index]);
            index += 1;
        }
    }
    js_trim(&String::from_utf16_lossy(&out))
}

/// `TestExplorerFilterState`.
pub struct TestExplorerFilterState {
    base: Rc<Disposable>,
    focus_emitter: Rc<Emitter<()>>,
    /// Mapping of terms to whether they're included in the text.
    term_filter_state: RefCell<Vec<TestFilterTerm>>,
    glob_list: RefCell<Vec<GlobFilter>>,
    include_tags: RefCell<IndexSet<String>>,
    exclude_tags: RefCell<IndexSet<String>>,
    text: Rc<MutableObservableValue<String>>,
    fuzzy: Rc<MutableObservableValue<bool>>,
    reveal: ISettableObservable<Option<String>>,
    select_test_in_explorer_emitter: Rc<Emitter<Option<String>>>,
}

impl TestExplorerFilterState {
    /// `new TestExplorerFilterState(storageService)`.
    pub fn new(storage_service: Rc<dyn StorageService>) -> Rc<Self> {
        let base = Disposable::new();
        let focus_emitter = Emitter::new();
        base.register(focus_emitter.clone());
        let text = MutableObservableValue::new(String::new());
        base.register(text.clone());
        let select_test_in_explorer_emitter = Emitter::new();
        base.register(select_test_in_explorer_emitter.clone());
        let fuzzy = MutableObservableValue::stored(
            StoredValue::<bool>::new(
                StoredValueOptions { key: "testHistoryFuzzy".to_owned(), scope: StorageScope::Profile, target: StorageTarget::User },
                storage_service,
            ),
            false,
        );
        base.register(fuzzy.clone());
        Rc::new(Self {
            base,
            focus_emitter,
            term_filter_state: RefCell::default(),
            glob_list: RefCell::default(),
            include_tags: RefCell::default(),
            exclude_tags: RefCell::default(),
            text,
            fuzzy,
            reveal: observable_value("TestExplorerFilterState.reveal", None).to_settable(),
            select_test_in_explorer_emitter,
        })
    }
}

impl IDisposable for TestExplorerFilterState {
    fn dispose(&self) {
        self.base.dispose();
    }
}

impl ITestExplorerFilterState for TestExplorerFilterState {
    fn text(&self) -> Rc<MutableObservableValue<String>> {
        self.text.clone()
    }

    fn reveal(&self) -> ISettableObservable<Option<String>> {
        self.reveal.clone()
    }

    fn on_did_select_test_in_explorer(&self) -> Event<Option<String>> {
        self.select_test_in_explorer_emitter.event()
    }

    fn on_did_request_input_focus(&self) -> Event<()> {
        self.focus_emitter.event()
    }

    fn glob_list(&self) -> Vec<GlobFilter> {
        self.glob_list.borrow().clone()
    }

    fn include_tags(&self) -> IndexSet<String> {
        self.include_tags.borrow().clone()
    }

    fn exclude_tags(&self) -> IndexSet<String> {
        self.exclude_tags.borrow().clone()
    }

    fn fuzzy(&self) -> Rc<MutableObservableValue<bool>> {
        self.fuzzy.clone()
    }

    fn did_select_test_in_explorer(&self, test_id: &str) {
        self.select_test_in_explorer_emitter.fire(Some(test_id.to_owned()));
    }

    fn focus_input(&self) {
        self.focus_emitter.fire(());
    }

    fn set_text(&self, text: &str) {
        if text == self.text.value() {
            return;
        }

        self.term_filter_state.borrow_mut().clear();
        self.glob_list.borrow_mut().clear();
        self.include_tags.borrow_mut().clear();
        self.exclude_tags.borrow_mut().clear();

        let units: Vec<u16> = text.encode_utf16().collect();
        let slice = |from: usize, to: usize| -> String { String::from_utf16_lossy(&units[from.min(units.len())..to.min(units.len()).max(from.min(units.len()))]) };
        let unit_at = |index: usize| -> Option<u16> { units.get(index).copied() };

        let mut glob_text = String::new();
        let mut last_index = 0;
        let matches = TAG_RE.with(|regexp| match_all(regexp, &units));
        for found in matches {
            let mut next_index = found.end();

            let tag = slice(found.start(), found.end());
            let term = TestFilterTerm::parse(&tag);
            if let Some(term) = term {
                let mut state = self.term_filter_state.borrow_mut();
                if !state.contains(&term) {
                    state.push(term);
                }
            }

            // recognize and parse @ctrlId:tagId or quoted like @ctrlId:"tag \\"id"
            let mut is_tag = false;
            if unit_at(next_index) == Some(u16::from(b':')) {
                is_tag = true;
                next_index += 1;

                let mut delimiter = unit_at(next_index);
                if delimiter != Some(u16::from(b'"')) && delimiter != Some(u16::from(b'\'')) {
                    delimiter = Some(u16::from(b' '));
                } else {
                    next_index += 1;
                }

                let mut tag_id: Vec<u16> = Vec::new();
                while next_index < units.len() && unit_at(next_index) != delimiter {
                    if units[next_index] == u16::from(b'\\') {
                        // `tagId += text[nextIndex + 1]`: além do fim, `undefined` vira o texto "undefined"
                        match unit_at(next_index + 1) {
                            Some(unit) => tag_id.push(unit),
                            None => tag_id.extend("undefined".encode_utf16()),
                        }
                        next_index += 2;
                    } else {
                        tag_id.push(units[next_index]);
                        next_index += 1;
                    }
                }

                let ctrl_id = found.captures.get(1).and_then(|range| range.clone()).map(|range| String::from_utf16_lossy(&units[range])).unwrap_or_default();
                let namespaced = namespace_test_tag(&ctrl_id, &String::from_utf16_lossy(&tag_id));
                if tag.starts_with('!') {
                    self.exclude_tags.borrow_mut().insert(namespaced);
                } else {
                    self.include_tags.borrow_mut().insert(namespaced);
                }
                next_index += 1;
            }

            // If the @-prefixed text is not a known filter term or tag,
            // treat it as regular filter text (e.g., a test named "@smoke")
            if term.is_none() && !is_tag {
                continue;
            }

            glob_text.push_str(&slice(last_index, found.start()));
            last_index = next_index;
        }

        glob_text.push_str(js_trim(&slice(last_index, units.len())).as_str());

        if !glob_text.is_empty() {
            for filter in split_glob_aware(&glob_text, ',').iter().map(|s| js_trim(s)).filter(|s| !s.is_empty()) {
                if let Some(rest) = filter.strip_prefix('!') {
                    self.glob_list.borrow_mut().push(GlobFilter { include: false, text: JsString::from(rest).to_lower_case().to_string_lossy() });
                } else {
                    self.glob_list.borrow_mut().push(GlobFilter { include: true, text: JsString::from(filter.as_str()).to_lower_case().to_string_lossy() });
                }
            }
        }

        self.text.set_value(text.to_owned()); // purposely afterwards so everything is updated when the change event happen
    }

    fn is_filtering_for(&self, term: TestFilterTerm) -> bool {
        self.term_filter_state.borrow().contains(&term)
    }

    fn toggle_filtering_for(&self, term: TestFilterTerm, should_filter: Option<bool>) {
        let text = js_trim(&self.text.value());
        let filtering = self.is_filtering_for(term);
        if should_filter != Some(false) && !filtering {
            self.set_text(&if text.is_empty() { term.as_str().to_owned() } else { format!("{text} {}", term.as_str()) });
        } else if should_filter != Some(true) && filtering {
            // `text.replace(term, '')` troca só a primeira ocorrência
            self.set_text(&trim_extra_whitespace(&text.replacen(term.as_str(), "", 1)));
        }
    }
}
