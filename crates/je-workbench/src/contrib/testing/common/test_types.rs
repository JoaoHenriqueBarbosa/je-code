//! Porte de `src/vs/workbench/contrib/testing/common/testTypes.ts`: os tipos de dado do contrib testing (estados,
//! perfis de execução, mensagens, itens, cobertura e diffs) com as formas serializadas que atravessam o protocolo e o
//! disco.
//!
//! Divergências:
//! - **Namespaces.** `namespace ITestMessage { serialize, deserialize }` e afins viram funções associadas dos tipos
//!   (`ITestMessage::serialize`), e a interface `Serialized` de cada um é um tipo `...Serialized`. Onde o
//!   `Serialized` é o mesmo dado do tipo vivo (`Range` e `Position` já são `IRange` e `IPosition` no porte do editor),
//!   não há tipo nem função de conversão: a de repasse não existe (regra DRY do projeto). Isso vale para
//!   `IBranchCoverage`, `IDeclarationCoverage`, `IStatementCoverage` e `CoverageDetails`.
//! - **`UriComponents`.** O que `URI.toJSON()` devolve é o [`UriState`]; `URI.revive` é `URI::revive` na plataforma do
//!   hospedeiro.
//! - **`const enum` de bits** (`TestControllerCapability`, `TestRunProfileBitset`): são `u32` (o original os soma, mascara
//!   e guarda `group` com a combinação), com as constantes nos módulos [`test_controller_capability`] e
//!   [`test_run_profile_bitset`].
//! - **`string | IMarkdownString`** é [`TestMessageText`]. O `IMarkdownString` do porte não tem `PartialEq`, então a
//!   igualdade do enum usa `markdown_string_equal`.
//! - **`Partial<ITestItem>`** é [`PartialTestItem`]: `None` é "ausente" (`undefined`) e `Some(None)` é o `null` dos campos
//!   que o aceitam (`range`, `description`, `error`, `sortText`).
//! - **`applyTestItemUpdate(internal: InternalTestItem | ITestItemUpdate, patch)`** são duas funções: a que atualiza um
//!   [`InternalTestItem`] e a que atualiza um [`ITestItemUpdate`].
//! - **`ITestUriCanonicalizer`** é um trait com implementação para todo `IUriIdentityService` (o original é um
//!   subconjunto estrutural dele). As funções de `deserialize` recebem `&(impl ITestUriCanonicalizer + ?Sized)`.
//! - **`isStartControllerTests`** opera sobre o enum [`CallProfileOrStartController`] (a união dos dois tipos).
//! - **`AbstractIncrementalTestCollection`** é uma struct genérica sobre o nó (`T extends IncrementalTestCollectionItem`
//!   é o trait [`IncrementalNode`]); os membros abstratos (`createItem`, e os `createChangeCollector` e `retireTest`
//!   que as subclasses sobrescrevem) são o trait [`IncrementalCollectionHooks`]. Os nós são `Rc<RefCell<T>>`, porque o
//!   original os compartilha e muta por referência. `children: Set<string>` é um `IndexSet` (ordem de inserção).
//!   `apply` faz como o original: cada `Add` e `Update` passa por `serialize` e `deserialize`, mesmo vindo vivo.
//! - **Números.** `number` é `f64` quando é medida ou instante (`duration`, `completedAt`, contagens de cobertura, que
//!   aceitam diferença negativa) e inteiro quando é índice ou identificador.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use indexmap::{IndexMap, IndexSet};
use je_base::html_content::{IMarkdownString, markdown_string_equal};
use je_base::marshalling_ids::MarshalledId;
use je_base::nls::localize;
use je_base::uri::{URI, UriState};
use je_editor::position::Position;
use je_editor::range::Range;
use je_platform::resources::{ext_uri, host_platform};
use je_platform::uri_identity::uri_identity::IUriIdentityService;

use super::test_id::TestId;

/// `TestResultState`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TestResultState {
    Unset = 0,
    Queued = 1,
    Running = 2,
    Passed = 3,
    Failed = 4,
    Skipped = 5,
    Errored = 6,
}

impl TestResultState {
    /// Todos os estados, na ordem numérica.
    pub const ALL: [TestResultState; 7] = [
        TestResultState::Unset,
        TestResultState::Queued,
        TestResultState::Running,
        TestResultState::Passed,
        TestResultState::Failed,
        TestResultState::Skipped,
        TestResultState::Errored,
    ];

    /// O estado de um número, se for um dos sete.
    pub fn from_number(value: f64) -> Option<TestResultState> {
        TestResultState::ALL.iter().copied().find(|state| f64::from(*state as u8) == value)
    }

    /// O valor numérico do `const enum`.
    pub fn as_number(self) -> f64 {
        f64::from(self as u8)
    }
}

/// `testResultStateToContextValues[state]`.
pub fn test_result_state_to_context_value(state: TestResultState) -> &'static str {
    match state {
        TestResultState::Unset => "unset",
        TestResultState::Queued => "queued",
        TestResultState::Running => "running",
        TestResultState::Passed => "passed",
        TestResultState::Failed => "failed",
        TestResultState::Skipped => "skipped",
        TestResultState::Errored => "errored",
    }
}

/// `ExtTestRunProfileKind`. Note: keep in sync with TestRunProfileKind in vscode.d.ts
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExtTestRunProfileKind {
    Run = 1,
    Debug = 2,
    Coverage = 3,
}

/// `TestControllerCapability`.
pub type TestControllerCapability = u32;

pub mod test_controller_capability {
    pub const REFRESH: u32 = 1 << 1;
    pub const CODE_RELATED_TO_TEST: u32 = 1 << 2;
    pub const TEST_RELATED_TO_CODE: u32 = 1 << 3;
}

/// `TestRunProfileBitset`.
pub type TestRunProfileBitset = u32;

pub mod test_run_profile_bitset {
    pub const RUN: u32 = 1 << 1;
    pub const DEBUG: u32 = 1 << 2;
    pub const COVERAGE: u32 = 1 << 3;
    pub const HAS_NON_DEFAULT_PROFILE: u32 = 1 << 4;
    pub const HAS_CONFIGURABLE: u32 = 1 << 5;
    pub const SUPPORTS_CONTINUOUS_RUN: u32 = 1 << 6;
}

/// `testProfileBitset`: o rótulo de cada grupo de perfil.
pub fn test_profile_bitset() -> Vec<(TestRunProfileBitset, String)> {
    vec![
        (test_run_profile_bitset::RUN, localize("testing.runProfileBitset.run", "Run", &[])),
        (test_run_profile_bitset::DEBUG, localize("testing.runProfileBitset.debug", "Debug", &[])),
        (test_run_profile_bitset::COVERAGE, localize("testing.runProfileBitset.coverage", "Coverage", &[])),
    ]
}

/// List of all test run profile bitset values.
pub const TEST_RUN_PROFILE_BITSET_LIST: [TestRunProfileBitset; 6] = [
    test_run_profile_bitset::RUN,
    test_run_profile_bitset::DEBUG,
    test_run_profile_bitset::COVERAGE,
    test_run_profile_bitset::HAS_NON_DEFAULT_PROFILE,
    test_run_profile_bitset::HAS_CONFIGURABLE,
    test_run_profile_bitset::SUPPORTS_CONTINUOUS_RUN,
];

/// DTO for a controller's run profiles.
#[derive(Clone, Debug, PartialEq)]
pub struct ITestRunProfile {
    pub controller_id: String,
    pub profile_id: u32,
    pub label: String,
    pub group: TestRunProfileBitset,
    pub is_default: bool,
    pub tag: Option<String>,
    pub has_configuration_handler: bool,
    pub supports_continuous_run: bool,
}

/// `Partial<ITestRunProfile>`: `None` é "ausente"; o `tag` aceita `null`, que é `Some(None)`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PartialTestRunProfile {
    pub controller_id: Option<String>,
    pub profile_id: Option<u32>,
    pub label: Option<String>,
    pub group: Option<TestRunProfileBitset>,
    pub is_default: Option<bool>,
    pub tag: Option<Option<String>>,
    pub has_configuration_handler: Option<bool>,
    pub supports_continuous_run: Option<bool>,
}

/// `ITestRunProfileReference`.
#[derive(Clone, Debug, PartialEq)]
pub struct ITestRunProfileReference {
    pub controller_id: String,
    pub profile_id: u32,
    pub group: TestRunProfileBitset,
}

/// Um elemento de `ResolvedTestRunRequest.targets`.
#[derive(Clone, Debug, PartialEq)]
pub struct TestRunTarget {
    pub test_ids: Vec<String>,
    pub controller_id: String,
    pub profile_id: u32,
}

/// A fully-resolved request to run tests, passsed between the main thread
/// and extension host.
#[derive(Clone, Debug, PartialEq)]
pub struct ResolvedTestRunRequest {
    pub group: TestRunProfileBitset,
    pub targets: Vec<TestRunTarget>,
    pub exclude: Option<Vec<String>>,
    /// Whether this is a continuous test run
    pub continuous: Option<bool>,
    /// Whether this was trigged by a user action in UI. Default=true
    pub preserve_focus: Option<bool>,
}

/// O `profile?: { group, id }` de [`ExtensionRunTestsRequest`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ExtensionRunProfile {
    pub group: TestRunProfileBitset,
    pub id: u32,
}

/// Request to the main thread to run a set of tests.
#[derive(Clone, Debug, PartialEq)]
pub struct ExtensionRunTestsRequest {
    pub id: String,
    pub include: Vec<String>,
    pub exclude: Vec<String>,
    pub controller_id: String,
    pub profile: Option<ExtensionRunProfile>,
    pub persist: bool,
    pub preserve_focus: bool,
    /// Whether this is a result of a continuous test run request
    pub continuous: bool,
}

/// Request parameters a controller run handler. This is different than
/// [`IStartControllerTests`]. The latter is used to ask for one or more test
/// runs tracked directly by the renderer.
///
/// This alone can be used to start an autorun, without a specific associated runId.
#[derive(Clone, Debug, PartialEq)]
pub struct ICallProfileRunHandler {
    pub controller_id: String,
    pub profile_id: u32,
    pub exclude_ext_ids: Vec<String>,
    pub test_ids: Vec<String>,
}

/// Request from the main thread to run tests for a single controller.
#[derive(Clone, Debug, PartialEq)]
pub struct IStartControllerTests {
    pub controller_id: String,
    pub profile_id: u32,
    pub exclude_ext_ids: Vec<String>,
    pub test_ids: Vec<String>,
    pub run_id: String,
}

/// `ICallProfileRunHandler | IStartControllerTests`.
#[derive(Clone, Debug, PartialEq)]
pub enum CallProfileOrStartController {
    Call(ICallProfileRunHandler),
    Start(IStartControllerTests),
}

/// `isStartControllerTests`: `'runId' in t`.
pub fn is_start_controller_tests(t: &CallProfileOrStartController) -> bool {
    matches!(t, CallProfileOrStartController::Start(_))
}

/// `IStartControllerTestsResult`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct IStartControllerTestsResult {
    pub error: Option<String>,
}

/// Location with a fully-instantiated Range and URI.
#[derive(Clone, Debug, PartialEq)]
pub struct IRichLocation {
    pub range: Range,
    pub uri: URI,
}

/// Subset of the IUriIdentityService
pub trait ITestUriCanonicalizer {
    fn as_canonical_uri(&self, uri: &URI) -> URI;
}

impl<T: IUriIdentityService + ?Sized> ITestUriCanonicalizer for T {
    fn as_canonical_uri(&self, uri: &URI) -> URI {
        IUriIdentityService::as_canonical_uri(self, uri)
    }
}

/// `IRichLocation.Serialize`.
#[derive(Clone, Debug, PartialEq)]
pub struct IRichLocationSerialized {
    pub range: Range,
    pub uri: UriState,
}

/// `URI.revive(components)` seguido de `uriIdentity.asCanonicalUri`.
fn revive_canonical_uri(uri_identity: &(impl ITestUriCanonicalizer + ?Sized), components: &UriState) -> URI {
    uri_identity.as_canonical_uri(&URI::revive(components, host_platform()))
}

impl IRichLocation {
    pub fn serialize(location: &IRichLocation) -> IRichLocationSerialized {
        IRichLocationSerialized { range: location.range, uri: location.uri.to_json() }
    }

    pub fn deserialize(uri_identity: &(impl ITestUriCanonicalizer + ?Sized), location: &IRichLocationSerialized) -> IRichLocation {
        IRichLocation { range: location.range, uri: revive_canonical_uri(uri_identity, &location.uri) }
    }
}

/// `TestMessageType`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TestMessageType {
    Error,
    Output,
}

/// `ITestMessageStackFrame`.
#[derive(Clone, Debug, PartialEq)]
pub struct ITestMessageStackFrame {
    pub label: String,
    pub uri: Option<URI>,
    pub position: Option<Position>,
}

/// `ITestMessageStackFrame.Serialized`.
#[derive(Clone, Debug, PartialEq)]
pub struct ITestMessageStackFrameSerialized {
    pub label: String,
    pub uri: Option<UriState>,
    pub position: Option<Position>,
}

impl ITestMessageStackFrame {
    pub fn serialize(stack: &ITestMessageStackFrame) -> ITestMessageStackFrameSerialized {
        ITestMessageStackFrameSerialized { label: stack.label.clone(), uri: stack.uri.as_ref().map(URI::to_json), position: stack.position }
    }

    pub fn deserialize(uri_identity: &(impl ITestUriCanonicalizer + ?Sized), stack: &ITestMessageStackFrameSerialized) -> ITestMessageStackFrame {
        ITestMessageStackFrame {
            label: stack.label.clone(),
            uri: stack.uri.as_ref().map(|uri| revive_canonical_uri(uri_identity, uri)),
            position: stack.position,
        }
    }
}

/// `string | IMarkdownString`.
#[derive(Clone, Debug)]
pub enum TestMessageText {
    Text(String),
    Markdown(IMarkdownString),
}

impl PartialEq for TestMessageText {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (TestMessageText::Text(a), TestMessageText::Text(b)) => a == b,
            (TestMessageText::Markdown(a), TestMessageText::Markdown(b)) => markdown_string_equal(a, b, &ext_uri()),
            _ => false,
        }
    }
}

/// `ITestErrorMessage` (o `type` é sempre [`TestMessageType::Error`]).
#[derive(Clone, Debug, PartialEq)]
pub struct ITestErrorMessage {
    pub message: TestMessageText,
    pub expected: Option<String>,
    pub actual: Option<String>,
    pub context_value: Option<String>,
    pub location: Option<IRichLocation>,
    pub stack_trace: Option<Vec<ITestMessageStackFrame>>,
}

/// `ITestErrorMessage.Serialized`.
#[derive(Clone, Debug, PartialEq)]
pub struct ITestErrorMessageSerialized {
    pub message: TestMessageText,
    pub expected: Option<String>,
    pub actual: Option<String>,
    pub context_value: Option<String>,
    pub location: Option<IRichLocationSerialized>,
    pub stack_trace: Option<Vec<ITestMessageStackFrameSerialized>>,
}

impl ITestErrorMessage {
    pub fn serialize(message: &ITestErrorMessage) -> ITestErrorMessageSerialized {
        ITestErrorMessageSerialized {
            message: message.message.clone(),
            expected: message.expected.clone(),
            actual: message.actual.clone(),
            context_value: message.context_value.clone(),
            location: message.location.as_ref().map(IRichLocation::serialize),
            stack_trace: message.stack_trace.as_ref().map(|frames| frames.iter().map(ITestMessageStackFrame::serialize).collect()),
        }
    }

    pub fn deserialize(uri_identity: &(impl ITestUriCanonicalizer + ?Sized), message: &ITestErrorMessageSerialized) -> ITestErrorMessage {
        ITestErrorMessage {
            message: message.message.clone(),
            expected: message.expected.clone(),
            actual: message.actual.clone(),
            context_value: message.context_value.clone(),
            location: message.location.as_ref().map(|location| IRichLocation::deserialize(uri_identity, location)),
            stack_trace: message.stack_trace.as_ref().map(|frames| frames.iter().map(|s| ITestMessageStackFrame::deserialize(uri_identity, s)).collect()),
        }
    }
}

/// `ITestOutputMessage` (o `type` é sempre [`TestMessageType::Output`]).
#[derive(Clone, Debug, PartialEq)]
pub struct ITestOutputMessage {
    pub message: String,
    pub offset: usize,
    pub length: usize,
    pub marker: Option<u32>,
    pub location: Option<IRichLocation>,
}

/// Gets the TTY marker ID for either starting or ending
/// an ITestOutputMessage.marker of the given ID.
pub fn get_mark_id(marker: u32, start: bool) -> String {
    format!("{}{}", if start { 's' } else { 'e' }, marker)
}

/// `ITestOutputMessage.Serialized`.
#[derive(Clone, Debug, PartialEq)]
pub struct ITestOutputMessageSerialized {
    pub message: String,
    pub offset: usize,
    pub length: usize,
    pub location: Option<IRichLocationSerialized>,
}

impl ITestOutputMessage {
    pub fn serialize(message: &ITestOutputMessage) -> ITestOutputMessageSerialized {
        ITestOutputMessageSerialized {
            message: message.message.clone(),
            offset: message.offset,
            length: message.length,
            location: message.location.as_ref().map(IRichLocation::serialize),
        }
    }

    pub fn deserialize(uri_identity: &(impl ITestUriCanonicalizer + ?Sized), message: &ITestOutputMessageSerialized) -> ITestOutputMessage {
        ITestOutputMessage {
            message: message.message.clone(),
            offset: message.offset,
            length: message.length,
            marker: None,
            location: message.location.as_ref().map(|location| IRichLocation::deserialize(uri_identity, location)),
        }
    }
}

/// `ITestMessage = ITestErrorMessage | ITestOutputMessage`.
#[derive(Clone, Debug, PartialEq)]
pub enum ITestMessage {
    Error(ITestErrorMessage),
    Output(ITestOutputMessage),
}

/// `ITestMessage.Serialized`.
#[derive(Clone, Debug, PartialEq)]
pub enum ITestMessageSerialized {
    Error(ITestErrorMessageSerialized),
    Output(ITestOutputMessageSerialized),
}

impl ITestMessage {
    pub fn serialize(message: &ITestMessage) -> ITestMessageSerialized {
        match message {
            ITestMessage::Error(error) => ITestMessageSerialized::Error(ITestErrorMessage::serialize(error)),
            ITestMessage::Output(output) => ITestMessageSerialized::Output(ITestOutputMessage::serialize(output)),
        }
    }

    pub fn deserialize(uri_identity: &(impl ITestUriCanonicalizer + ?Sized), message: &ITestMessageSerialized) -> ITestMessage {
        match message {
            ITestMessageSerialized::Error(error) => ITestMessage::Error(ITestErrorMessage::deserialize(uri_identity, error)),
            ITestMessageSerialized::Output(output) => ITestMessage::Output(ITestOutputMessage::deserialize(uri_identity, output)),
        }
    }

    /// `isDiffable`: um erro com `actual` e `expected`.
    pub fn is_diffable(message: &ITestMessage) -> bool {
        matches!(message, ITestMessage::Error(error) if error.actual.is_some() && error.expected.is_some())
    }

    pub fn message_type(&self) -> TestMessageType {
        match self {
            ITestMessage::Error(_) => TestMessageType::Error,
            ITestMessage::Output(_) => TestMessageType::Output,
        }
    }
}

/// `ITestTaskState`.
#[derive(Clone, Debug, PartialEq)]
pub struct ITestTaskState {
    pub state: TestResultState,
    pub duration: Option<f64>,
    pub messages: Vec<ITestMessage>,
}

/// `ITestTaskState.Serialized`.
#[derive(Clone, Debug, PartialEq)]
pub struct ITestTaskStateSerialized {
    pub state: TestResultState,
    pub duration: Option<f64>,
    pub messages: Vec<ITestMessageSerialized>,
}

impl ITestTaskState {
    pub fn serialize_without_messages(state: &ITestTaskState) -> ITestTaskStateSerialized {
        ITestTaskStateSerialized { state: state.state, duration: state.duration, messages: Vec::new() }
    }

    pub fn serialize(state: &ITestTaskState) -> ITestTaskStateSerialized {
        ITestTaskStateSerialized { state: state.state, duration: state.duration, messages: state.messages.iter().map(ITestMessage::serialize).collect() }
    }

    pub fn deserialize(uri_identity: &(impl ITestUriCanonicalizer + ?Sized), state: &ITestTaskStateSerialized) -> ITestTaskState {
        ITestTaskState {
            state: state.state,
            duration: state.duration,
            messages: state.messages.iter().map(|m| ITestMessage::deserialize(uri_identity, m)).collect(),
        }
    }
}

/// `ITestRunTask`.
#[derive(Clone, Debug, PartialEq)]
pub struct ITestRunTask {
    pub id: String,
    pub name: String,
    pub running: bool,
    pub ctrl_id: String,
}

/// `ITestTag`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ITestTag {
    pub id: String,
}

const TEST_TAG_DELIMITER: char = '\0';

pub fn namespace_test_tag(ctrl_id: &str, tag_id: &str) -> String {
    format!("{ctrl_id}{TEST_TAG_DELIMITER}{tag_id}")
}

/// `denamespaceTestTag`: `(ctrlId, tagId)`. Sem delimitador, `indexOf` dá `-1` e o `slice(0, -1)` do original tira o
/// último caractere do texto, e `slice(0)` devolve o texto todo.
pub fn denamespace_test_tag(namespaced: &str) -> (String, String) {
    match namespaced.find(TEST_TAG_DELIMITER) {
        Some(index) => (namespaced[..index].to_string(), namespaced[index + TEST_TAG_DELIMITER.len_utf8()..].to_string()),
        None => {
            let mut chars = namespaced.chars();
            chars.next_back();
            (chars.as_str().to_string(), namespaced.to_string())
        }
    }
}

/// `ITestTagDisplayInfo`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ITestTagDisplayInfo {
    pub id: String,
}

/// The TestItem from .d.ts, as a plain object without children.
#[derive(Clone, Debug, PartialEq)]
pub struct ITestItem {
    /// ID of the test given by the test controller
    pub ext_id: String,
    pub label: String,
    pub tags: Vec<String>,
    pub busy: bool,
    pub uri: Option<URI>,
    pub range: Option<Range>,
    pub description: Option<String>,
    pub error: Option<TestMessageText>,
    pub sort_text: Option<String>,
}

/// `ITestItem.Serialized`.
#[derive(Clone, Debug, PartialEq)]
pub struct ITestItemSerialized {
    pub ext_id: String,
    pub label: String,
    pub tags: Vec<String>,
    pub busy: bool,
    pub uri: Option<UriState>,
    pub range: Option<Range>,
    pub description: Option<String>,
    pub error: Option<TestMessageText>,
    pub sort_text: Option<String>,
}

impl ITestItem {
    pub fn serialize(item: &ITestItem) -> ITestItemSerialized {
        ITestItemSerialized {
            ext_id: item.ext_id.clone(),
            label: item.label.clone(),
            tags: item.tags.clone(),
            busy: item.busy,
            uri: item.uri.as_ref().map(URI::to_json),
            range: item.range,
            description: item.description.clone(),
            error: item.error.clone(),
            sort_text: item.sort_text.clone(),
        }
    }

    pub fn deserialize(uri_identity: &(impl ITestUriCanonicalizer + ?Sized), serialized: &ITestItemSerialized) -> ITestItem {
        ITestItem {
            ext_id: serialized.ext_id.clone(),
            label: serialized.label.clone(),
            tags: serialized.tags.clone(),
            busy: serialized.busy,
            uri: serialized.uri.as_ref().map(|uri| revive_canonical_uri(uri_identity, uri)),
            range: serialized.range,
            description: serialized.description.clone(),
            error: serialized.error.clone(),
            sort_text: serialized.sort_text.clone(),
        }
    }

    /// `Object.assign(item, patch)`: copia só o que o patch traz.
    pub fn assign(&mut self, patch: &PartialTestItem) {
        if let Some(ext_id) = &patch.ext_id {
            self.ext_id = ext_id.clone();
        }
        if let Some(label) = &patch.label {
            self.label = label.clone();
        }
        if let Some(tags) = &patch.tags {
            self.tags = tags.clone();
        }
        if let Some(busy) = patch.busy {
            self.busy = busy;
        }
        if let Some(uri) = &patch.uri {
            self.uri = Some(uri.clone());
        }
        if let Some(range) = patch.range {
            self.range = range;
        }
        if let Some(description) = &patch.description {
            self.description = description.clone();
        }
        if let Some(error) = &patch.error {
            self.error = error.clone();
        }
        if let Some(sort_text) = &patch.sort_text {
            self.sort_text = sort_text.clone();
        }
    }
}

/// `Partial<ITestItem>`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PartialTestItem {
    pub ext_id: Option<String>,
    pub label: Option<String>,
    pub tags: Option<Vec<String>>,
    pub busy: Option<bool>,
    pub uri: Option<URI>,
    pub range: Option<Option<Range>>,
    pub description: Option<Option<String>>,
    pub error: Option<Option<TestMessageText>>,
    pub sort_text: Option<Option<String>>,
}

impl PartialTestItem {
    /// `Object.assign(patch, other)`: sobrescreve só o que `other` traz.
    pub fn assign(&mut self, other: &PartialTestItem) {
        if other.ext_id.is_some() {
            self.ext_id = other.ext_id.clone();
        }
        if other.label.is_some() {
            self.label = other.label.clone();
        }
        if other.tags.is_some() {
            self.tags = other.tags.clone();
        }
        if other.busy.is_some() {
            self.busy = other.busy;
        }
        if other.uri.is_some() {
            self.uri = other.uri.clone();
        }
        if other.range.is_some() {
            self.range = other.range;
        }
        if other.description.is_some() {
            self.description = other.description.clone();
        }
        if other.error.is_some() {
            self.error = other.error.clone();
        }
        if other.sort_text.is_some() {
            self.sort_text = other.sort_text.clone();
        }
    }
}

/// `TestItemExpandState`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TestItemExpandState {
    NotExpandable = 0,
    Expandable = 1,
    BusyExpanding = 2,
    Expanded = 3,
}

impl TestItemExpandState {
    pub fn from_number(value: f64) -> Option<TestItemExpandState> {
        [TestItemExpandState::NotExpandable, TestItemExpandState::Expandable, TestItemExpandState::BusyExpanding, TestItemExpandState::Expanded]
            .into_iter()
            .find(|state| f64::from(*state as u8) == value)
    }

    pub fn as_number(self) -> f64 {
        f64::from(self as u8)
    }
}

/// TestItem-like shape, but with an ID and children as strings.
#[derive(Clone, Debug, PartialEq)]
pub struct InternalTestItem {
    /// Controller ID from whence this test came
    pub controller_id: String,
    /// Expandability state
    pub expand: TestItemExpandState,
    /// Raw test item properties
    pub item: ITestItem,
}

/// `InternalTestItem.Serialized`.
#[derive(Clone, Debug, PartialEq)]
pub struct InternalTestItemSerialized {
    pub expand: TestItemExpandState,
    pub item: ITestItemSerialized,
}

impl InternalTestItem {
    pub fn serialize(item: &InternalTestItem) -> InternalTestItemSerialized {
        InternalTestItemSerialized { expand: item.expand, item: ITestItem::serialize(&item.item) }
    }

    pub fn deserialize(uri_identity: &(impl ITestUriCanonicalizer + ?Sized), serialized: &InternalTestItemSerialized) -> InternalTestItem {
        InternalTestItem {
            // the `controllerId` is derived from the test.item.extId. It's redundant
            // in the non-serialized InternalTestItem too, but there just because it's
            // checked against in many hot paths.
            controller_id: TestId::root_string(&serialized.item.ext_id).to_string(),
            expand: serialized.expand,
            item: ITestItem::deserialize(uri_identity, &serialized.item),
        }
    }
}

/// A partial update made to an existing InternalTestItem.
#[derive(Clone, Debug, PartialEq)]
pub struct ITestItemUpdate {
    pub ext_id: String,
    pub expand: Option<TestItemExpandState>,
    pub item: Option<PartialTestItem>,
}

/// `Partial<ITestItem.Serialized>`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PartialTestItemSerialized {
    pub label: Option<String>,
    pub tags: Option<Vec<String>>,
    pub busy: Option<bool>,
    pub uri: Option<UriState>,
    pub range: Option<Option<Range>>,
    pub description: Option<Option<String>>,
    pub error: Option<Option<TestMessageText>>,
    pub sort_text: Option<Option<String>>,
}

/// `ITestItemUpdate.Serialized`.
#[derive(Clone, Debug, PartialEq)]
pub struct ITestItemUpdateSerialized {
    pub ext_id: String,
    pub expand: Option<TestItemExpandState>,
    pub item: Option<PartialTestItemSerialized>,
}

impl ITestItemUpdate {
    pub fn serialize(u: &ITestItemUpdate) -> ITestItemUpdateSerialized {
        let item = u.item.as_ref().map(|item| PartialTestItemSerialized {
            label: item.label.clone(),
            tags: item.tags.clone(),
            busy: item.busy,
            uri: item.uri.as_ref().map(URI::to_json),
            range: item.range,
            description: item.description.clone(),
            error: item.error.clone(),
            sort_text: item.sort_text.clone(),
        });

        ITestItemUpdateSerialized { ext_id: u.ext_id.clone(), expand: u.expand, item }
    }

    /// Como o original, não copia o `uri` (a atualização de URI não é suportada num item existente).
    pub fn deserialize(u: &ITestItemUpdateSerialized) -> ITestItemUpdate {
        let item = u.item.as_ref().map(|item| PartialTestItem {
            label: item.label.clone(),
            tags: item.tags.clone(),
            busy: item.busy,
            range: item.range,
            description: item.description.clone(),
            error: item.error.clone(),
            sort_text: item.sort_text.clone(),
            ..PartialTestItem::default()
        });

        ITestItemUpdate { ext_id: u.ext_id.clone(), expand: u.expand, item }
    }
}

/// `applyTestItemUpdate(internal: InternalTestItem, patch)`.
pub fn apply_test_item_update(internal: &mut InternalTestItem, patch: &ITestItemUpdate) {
    if let Some(expand) = patch.expand {
        internal.expand = expand;
    }
    if let Some(item) = &patch.item {
        internal.item.assign(item);
    }
}

/// `applyTestItemUpdate(internal: ITestItemUpdate, patch)`: sem `item` ainda, o patch o assume.
pub fn apply_test_item_update_to_update(internal: &mut ITestItemUpdate, patch: &ITestItemUpdate) {
    if let Some(expand) = patch.expand {
        internal.expand = Some(expand);
    }
    if let Some(item) = &patch.item {
        match &mut internal.item {
            Some(existing) => existing.assign(item),
            None => internal.item = Some(item.clone()),
        }
    }
}

/// Request to an ext host to get followup messages for a test failure.
#[derive(Clone, Debug, PartialEq)]
pub struct TestMessageFollowupRequest {
    pub result_id: String,
    pub ext_id: String,
    pub task_index: usize,
    pub message_index: usize,
}

/// Request to an ext host to get followup messages for a test failure.
#[derive(Clone, Debug, PartialEq)]
pub struct TestMessageFollowupResponse {
    pub id: u32,
    pub title: String,
}

/// Test result item used in the main thread.
#[derive(Clone, Debug, PartialEq)]
pub struct TestResultItem {
    /// Controller ID from whence this test came
    pub controller_id: String,
    /// Expandability state
    pub expand: TestItemExpandState,
    /// Raw test item properties
    pub item: ITestItem,
    /// State of this test in various tasks
    pub tasks: Vec<ITestTaskState>,
    /// State of this test as a computation of its tasks
    pub own_computed_state: TestResultState,
    /// Computed state based on children
    pub computed_state: TestResultState,
    /// Max duration of the item's tasks (if run directly)
    pub own_duration: Option<f64>,
    /// Whether this test item is outdated
    pub retired: Option<bool>,
}

/// Serialized version of the TestResultItem. Note that 'retired' is not
/// included since all hydrated items are automatically retired.
#[derive(Clone, Debug, PartialEq)]
pub struct TestResultItemSerialized {
    pub expand: TestItemExpandState,
    pub item: ITestItemSerialized,
    pub tasks: Vec<ITestTaskStateSerialized>,
    pub own_computed_state: TestResultState,
    pub computed_state: TestResultState,
}

impl TestResultItem {
    pub fn serialize_without_messages(original: &TestResultItem) -> TestResultItemSerialized {
        TestResultItemSerialized {
            expand: original.expand,
            item: ITestItem::serialize(&original.item),
            own_computed_state: original.own_computed_state,
            computed_state: original.computed_state,
            tasks: original.tasks.iter().map(ITestTaskState::serialize_without_messages).collect(),
        }
    }

    pub fn serialize(original: &TestResultItem) -> TestResultItemSerialized {
        TestResultItemSerialized {
            expand: original.expand,
            item: ITestItem::serialize(&original.item),
            own_computed_state: original.own_computed_state,
            computed_state: original.computed_state,
            tasks: original.tasks.iter().map(ITestTaskState::serialize).collect(),
        }
    }

    pub fn deserialize(uri_identity: &(impl ITestUriCanonicalizer + ?Sized), serialized: &TestResultItemSerialized) -> TestResultItem {
        let internal = InternalTestItem::deserialize(
            uri_identity,
            &InternalTestItemSerialized { expand: serialized.expand, item: serialized.item.clone() },
        );
        TestResultItem {
            controller_id: internal.controller_id,
            expand: internal.expand,
            item: internal.item,
            own_computed_state: serialized.own_computed_state,
            computed_state: serialized.computed_state,
            tasks: serialized.tasks.iter().map(|m| ITestTaskState::deserialize(uri_identity, m)).collect(),
            own_duration: None,
            retired: Some(true),
        }
    }
}

/// Um elemento de `ISerializedTestResults.tasks`.
#[derive(Clone, Debug, PartialEq)]
pub struct SerializedTestResultsTask {
    pub id: String,
    pub name: Option<String>,
    pub ctrl_id: String,
    pub has_coverage: bool,
}

/// `ISerializedTestResults`.
#[derive(Clone, Debug, PartialEq)]
pub struct ISerializedTestResults {
    /// ID of these test results
    pub id: String,
    /// Time the results were compelted
    pub completed_at: f64,
    /// Subset of test result items
    pub items: Vec<TestResultItemSerialized>,
    /// Tasks involved in the run.
    pub tasks: Vec<SerializedTestResultsTask>,
    /// Human-readable name of the test run.
    pub name: String,
    /// Test trigger informaton
    pub request: ResolvedTestRunRequest,
}

/// `ITestCoverage`.
#[derive(Clone, Debug, PartialEq)]
pub struct ITestCoverage {
    pub files: Vec<IFileCoverage>,
}

/// `ICoverageCount`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ICoverageCount {
    pub covered: f64,
    pub total: f64,
}

impl ICoverageCount {
    pub fn empty() -> ICoverageCount {
        ICoverageCount { covered: 0.0, total: 0.0 }
    }

    pub fn sum(target: &mut ICoverageCount, src: &ICoverageCount) {
        target.covered += src.covered;
        target.total += src.total;
    }
}

/// `IFileCoverage`.
#[derive(Clone, Debug, PartialEq)]
pub struct IFileCoverage {
    pub id: String,
    pub uri: URI,
    pub test_ids: Option<Vec<String>>,
    pub statement: ICoverageCount,
    pub branch: Option<ICoverageCount>,
    pub declaration: Option<ICoverageCount>,
}

/// `IFileCoverage.Serialized`.
#[derive(Clone, Debug, PartialEq)]
pub struct IFileCoverageSerialized {
    pub id: String,
    pub uri: UriState,
    pub test_ids: Option<Vec<String>>,
    pub statement: ICoverageCount,
    pub branch: Option<ICoverageCount>,
    pub declaration: Option<ICoverageCount>,
}

impl IFileCoverage {
    pub fn serialize(original: &IFileCoverage) -> IFileCoverageSerialized {
        IFileCoverageSerialized {
            id: original.id.clone(),
            statement: original.statement,
            branch: original.branch,
            declaration: original.declaration,
            test_ids: original.test_ids.clone(),
            uri: original.uri.to_json(),
        }
    }

    pub fn deserialize(uri_identity: &(impl ITestUriCanonicalizer + ?Sized), serialized: &IFileCoverageSerialized) -> IFileCoverage {
        IFileCoverage {
            id: serialized.id.clone(),
            statement: serialized.statement,
            branch: serialized.branch,
            declaration: serialized.declaration,
            test_ids: serialized.test_ids.clone(),
            uri: revive_canonical_uri(uri_identity, &serialized.uri),
        }
    }

    pub fn empty(id: &str, uri: URI) -> IFileCoverage {
        IFileCoverage { id: id.to_string(), uri, statement: ICoverageCount::empty(), test_ids: None, branch: None, declaration: None }
    }
}

/// Number of recent runs in which coverage reports should be retained.
pub const KEEP_N_LAST_COVERAGE_REPORTS: usize = 3;

/// `DetailType`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DetailType {
    Declaration,
    Statement,
    Branch,
}

/// `Range | Position`: o `location` de uma cobertura (`IRange | IPosition` na forma serializada, que é o mesmo dado).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum CoverageLocation {
    Range(Range),
    Position(Position),
}

/// `number | boolean`: o `count` de uma cobertura.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum CoverageCountValue {
    Number(f64),
    Bool(bool),
}

impl CoverageCountValue {
    /// A veracidade (`count ? 1 : 0`): `0`, `NaN` e `false` são falsos.
    pub fn is_truthy(self) -> bool {
        match self {
            CoverageCountValue::Number(n) => n != 0.0 && !n.is_nan(),
            CoverageCountValue::Bool(b) => b,
        }
    }
}

/// `IBranchCoverage` (e `IBranchCoverage.Serialized`, o mesmo dado).
#[derive(Clone, Debug, PartialEq)]
pub struct IBranchCoverage {
    pub count: CoverageCountValue,
    pub label: Option<String>,
    pub location: Option<CoverageLocation>,
}

/// `IDeclarationCoverage` (e `IDeclarationCoverage.Serialized`, o mesmo dado).
#[derive(Clone, Debug, PartialEq)]
pub struct IDeclarationCoverage {
    pub name: String,
    pub count: CoverageCountValue,
    pub location: CoverageLocation,
}

/// `IStatementCoverage` (e `IStatementCoverage.Serialized`, o mesmo dado).
#[derive(Clone, Debug, PartialEq)]
pub struct IStatementCoverage {
    pub count: CoverageCountValue,
    pub location: CoverageLocation,
    pub branches: Option<Vec<IBranchCoverage>>,
}

/// `CoverageDetails = IDeclarationCoverage | IStatementCoverage` (e `CoverageDetails.Serialized`).
#[derive(Clone, Debug, PartialEq)]
pub enum CoverageDetails {
    Declaration(IDeclarationCoverage),
    Statement(IStatementCoverage),
}

impl CoverageDetails {
    pub fn detail_type(&self) -> DetailType {
        match self {
            CoverageDetails::Declaration(_) => DetailType::Declaration,
            CoverageDetails::Statement(_) => DetailType::Statement,
        }
    }
}

/// `TestDiffOpType`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TestDiffOpType {
    /// Adds a new test (with children)
    Add,
    /// Shallow-updates an existing test
    Update,
    /// Ranges of some tests in a document were synced, so it should be considered up-to-date
    DocumentSynced,
    /// Removes a test (and all its children)
    Remove,
    /// Changes the number of controllers who are yet to publish their collection roots.
    IncrementPendingExtHosts,
    /// Retires a test/result
    Retire,
    /// Add a new test tag
    AddTag,
    /// Remove a test tag
    RemoveTag,
}

/// `TestsDiffOp`.
#[derive(Clone, Debug, PartialEq)]
pub enum TestsDiffOp {
    Add { item: InternalTestItem },
    Update { item: ITestItemUpdate },
    Remove { item_id: String },
    Retire { item_id: String },
    IncrementPendingExtHosts { amount: i32 },
    AddTag { tag: ITestTagDisplayInfo },
    RemoveTag { id: String },
    DocumentSynced { uri: URI, docv: Option<f64> },
}

/// `TestsDiffOp.Serialized`.
#[derive(Clone, Debug, PartialEq)]
pub enum TestsDiffOpSerialized {
    Add { item: InternalTestItemSerialized },
    Update { item: ITestItemUpdateSerialized },
    Remove { item_id: String },
    Retire { item_id: String },
    IncrementPendingExtHosts { amount: i32 },
    AddTag { tag: ITestTagDisplayInfo },
    RemoveTag { id: String },
    DocumentSynced { uri: UriState, docv: Option<f64> },
}

impl TestsDiffOp {
    pub fn op_type(&self) -> TestDiffOpType {
        match self {
            TestsDiffOp::Add { .. } => TestDiffOpType::Add,
            TestsDiffOp::Update { .. } => TestDiffOpType::Update,
            TestsDiffOp::Remove { .. } => TestDiffOpType::Remove,
            TestsDiffOp::Retire { .. } => TestDiffOpType::Retire,
            TestsDiffOp::IncrementPendingExtHosts { .. } => TestDiffOpType::IncrementPendingExtHosts,
            TestsDiffOp::AddTag { .. } => TestDiffOpType::AddTag,
            TestsDiffOp::RemoveTag { .. } => TestDiffOpType::RemoveTag,
            TestsDiffOp::DocumentSynced { .. } => TestDiffOpType::DocumentSynced,
        }
    }

    pub fn deserialize(uri_identity: &(impl ITestUriCanonicalizer + ?Sized), u: &TestsDiffOpSerialized) -> TestsDiffOp {
        match u {
            TestsDiffOpSerialized::Add { item } => TestsDiffOp::Add { item: InternalTestItem::deserialize(uri_identity, item) },
            TestsDiffOpSerialized::Update { item } => TestsDiffOp::Update { item: ITestItemUpdate::deserialize(item) },
            TestsDiffOpSerialized::DocumentSynced { uri, docv } => {
                TestsDiffOp::DocumentSynced { uri: revive_canonical_uri(uri_identity, uri), docv: *docv }
            }
            TestsDiffOpSerialized::Remove { item_id } => TestsDiffOp::Remove { item_id: item_id.clone() },
            TestsDiffOpSerialized::Retire { item_id } => TestsDiffOp::Retire { item_id: item_id.clone() },
            TestsDiffOpSerialized::IncrementPendingExtHosts { amount } => TestsDiffOp::IncrementPendingExtHosts { amount: *amount },
            TestsDiffOpSerialized::AddTag { tag } => TestsDiffOp::AddTag { tag: tag.clone() },
            TestsDiffOpSerialized::RemoveTag { id } => TestsDiffOp::RemoveTag { id: id.clone() },
        }
    }

    pub fn serialize(u: &TestsDiffOp) -> TestsDiffOpSerialized {
        match u {
            TestsDiffOp::Add { item } => TestsDiffOpSerialized::Add { item: InternalTestItem::serialize(item) },
            TestsDiffOp::Update { item } => TestsDiffOpSerialized::Update { item: ITestItemUpdate::serialize(item) },
            TestsDiffOp::DocumentSynced { uri, docv } => TestsDiffOpSerialized::DocumentSynced { uri: uri.to_json(), docv: *docv },
            TestsDiffOp::Remove { item_id } => TestsDiffOpSerialized::Remove { item_id: item_id.clone() },
            TestsDiffOp::Retire { item_id } => TestsDiffOpSerialized::Retire { item_id: item_id.clone() },
            TestsDiffOp::IncrementPendingExtHosts { amount } => TestsDiffOpSerialized::IncrementPendingExtHosts { amount: *amount },
            TestsDiffOp::AddTag { tag } => TestsDiffOpSerialized::AddTag { tag: tag.clone() },
            TestsDiffOp::RemoveTag { id } => TestsDiffOpSerialized::RemoveTag { id: id.clone() },
        }
    }
}

/// Context for actions taken in the test explorer view.
#[derive(Clone, Debug, PartialEq)]
pub struct ITestItemContext {
    /// Marshalling marker (`MarshalledId.TestItemContext`)
    pub mid: MarshalledId,
    /// Tests and parents from the root to the current items
    pub tests: Vec<InternalTestItemSerialized>,
}

/// Context for actions taken in the test explorer view.
#[derive(Clone, Debug, PartialEq)]
pub struct ITestMessageMenuArgs {
    /// Marshalling marker (`MarshalledId.TestMessageMenuArgs`)
    pub mid: MarshalledId,
    /// Tests ext ID
    pub test: InternalTestItemSerialized,
    /// Serialized test message
    pub message: ITestMessageSerialized,
}

/// Request from the ext host or main thread to indicate that tests have
/// changed. It's assumed that any item upserted *must* have its children
/// previously also upserted, or upserted as part of the same operation.
/// Children that no longer exist in an upserted item will be removed.
pub type TestsDiff = Vec<TestsDiffOp>;

/// `IncrementalTestCollectionItem` (`@private`).
#[derive(Clone, Debug, PartialEq)]
pub struct IncrementalTestCollectionItem {
    /// Controller ID from whence this test came
    pub controller_id: String,
    /// Expandability state
    pub expand: TestItemExpandState,
    /// Raw test item properties
    pub item: ITestItem,
    pub children: IndexSet<String>,
}

/// O que o código genérico da coleção lê de um nó (`T extends IncrementalTestCollectionItem`).
pub trait IncrementalNode {
    fn node(&self) -> &IncrementalTestCollectionItem;
    fn node_mut(&mut self) -> &mut IncrementalTestCollectionItem;
}

impl IncrementalNode for IncrementalTestCollectionItem {
    fn node(&self) -> &IncrementalTestCollectionItem {
        self
    }

    fn node_mut(&mut self) -> &mut IncrementalTestCollectionItem {
        self
    }
}

/// O handle compartilhado de um nó da coleção.
pub type IncrementalNodeRef<T> = Rc<RefCell<T>>;

/// The IncrementalChangeCollector is used in the IncrementalTestCollection
/// and called with diff changes as they're applied. This is used in the
/// ext host to create a cohesive change event from a diff. Todos os métodos são opcionais no original.
pub trait IncrementalChangeCollector<T> {
    /// A node was added.
    fn add(&self, _node: &IncrementalNodeRef<T>) {}

    /// A node in the collection was updated.
    fn update(&self, _node: &IncrementalNodeRef<T>) {}

    /// A node was removed.
    fn remove(&self, _node: &IncrementalNodeRef<T>, _is_nested_operation: bool) {}

    /// Called when the diff has been applied.
    fn complete(&self) {}
}

/// O `createChangeCollector` padrão: `return {}`.
struct NoopChangeCollector;

impl<T> IncrementalChangeCollector<T> for NoopChangeCollector {}

/// Os membros que as subclasses de `AbstractIncrementalTestCollection` definem ou sobrescrevem.
pub trait IncrementalCollectionHooks<T> {
    /// Creates a new item for the collection from the internal test item.
    fn create_item(&self, internal: &InternalTestItem, parent: Option<&IncrementalNodeRef<T>>) -> T;

    /// Called before a diff is applied to create a new change collector.
    fn create_change_collector(&self) -> Rc<dyn IncrementalChangeCollector<T>> {
        Rc::new(NoopChangeCollector)
    }

    /// Called when the extension signals a test result should be retired.
    fn retire_test(&self, _test_id: &str) {
        // no-op
    }
}

/// Maintains tests in this extension host sent from the main thread.
pub struct AbstractIncrementalTestCollection<T: IncrementalNode + 'static> {
    uri_identity: Rc<dyn ITestUriCanonicalizer>,
    tags: RefCell<IndexMap<String, ITestTagDisplayInfo>>,
    /// Map of item IDs to test item objects.
    pub items: RefCell<HashMap<String, IncrementalNodeRef<T>>>,
    /// ID of test root items.
    pub roots: RefCell<Vec<IncrementalNodeRef<T>>>,
    /// Number of 'busy' controllers.
    pub busy_controller_count: Cell<i32>,
    /// Number of pending roots.
    pub pending_root_count: Cell<i32>,
    hooks: Rc<dyn IncrementalCollectionHooks<T>>,
}

impl<T: IncrementalNode + 'static> AbstractIncrementalTestCollection<T> {
    pub fn new(uri_identity: Rc<dyn ITestUriCanonicalizer>, hooks: Rc<dyn IncrementalCollectionHooks<T>>) -> Self {
        Self {
            uri_identity,
            tags: RefCell::new(IndexMap::new()),
            items: RefCell::new(HashMap::new()),
            roots: RefCell::new(Vec::new()),
            busy_controller_count: Cell::new(0),
            pending_root_count: Cell::new(0),
            hooks,
        }
    }

    /// Known test tags.
    pub fn tags(&self) -> Vec<ITestTagDisplayInfo> {
        self.tags.borrow().values().cloned().collect()
    }

    /// Applies the diff to the collection.
    pub fn apply(&self, diff: &TestsDiff) {
        let changes = self.hooks.create_change_collector();

        for op in diff {
            match op {
                TestsDiffOp::Add { item } => {
                    // O original desserializa o item mesmo quando o diff já é vivo (os tipos são estruturais).
                    let internal = InternalTestItem::deserialize(self.uri_identity.as_ref(), &InternalTestItem::serialize(item));
                    self.add(&internal, changes.as_ref());
                }
                TestsDiffOp::Update { item } => {
                    let update = ITestItemUpdate::deserialize(&ITestItemUpdate::serialize(item));
                    self.update(&update, changes.as_ref());
                }
                TestsDiffOp::Remove { item_id } => self.remove(item_id, changes.as_ref()),
                TestsDiffOp::Retire { item_id } => self.hooks.retire_test(item_id),
                TestsDiffOp::IncrementPendingExtHosts { amount } => self.update_pending_roots(*amount),
                TestsDiffOp::AddTag { tag } => {
                    self.tags.borrow_mut().insert(tag.id.clone(), tag.clone());
                }
                TestsDiffOp::RemoveTag { id } => {
                    self.tags.borrow_mut().shift_remove(id);
                }
                TestsDiffOp::DocumentSynced { .. } => {}
            }
        }

        changes.complete();
    }

    fn add(&self, item: &InternalTestItem, changes: &dyn IncrementalChangeCollector<T>) -> Option<IncrementalNodeRef<T>> {
        let parent_id = TestId::parent_id_string(&item.item.ext_id).filter(|id| !id.is_empty()).map(str::to_string);
        let created: IncrementalNodeRef<T>;
        match parent_id {
            None => {
                created = Rc::new(RefCell::new(self.hooks.create_item(item, None)));
                self.roots.borrow_mut().push(created.clone());
                self.items.borrow_mut().insert(item.item.ext_id.clone(), created.clone());
            }
            Some(parent_id) => {
                let parent = self.items.borrow().get(&parent_id).cloned();
                match parent {
                    Some(parent) => {
                        parent.borrow_mut().node_mut().children.insert(item.item.ext_id.clone());
                        created = Rc::new(RefCell::new(self.hooks.create_item(item, Some(&parent))));
                        self.items.borrow_mut().insert(item.item.ext_id.clone(), created.clone());
                    }
                    None => {
                        eprintln!("Test with unknown parent ID: {item:?}");
                        return None;
                    }
                }
            }
        }

        changes.add(&created);
        if item.expand == TestItemExpandState::BusyExpanding {
            self.busy_controller_count.set(self.busy_controller_count.get() + 1);
        }

        Some(created)
    }

    fn update(&self, patch: &ITestItemUpdate, changes: &dyn IncrementalChangeCollector<T>) -> Option<IncrementalNodeRef<T>> {
        let existing = self.items.borrow().get(&patch.ext_id).cloned()?;

        if patch.expand.is_some() {
            if existing.borrow().node().expand == TestItemExpandState::BusyExpanding {
                self.busy_controller_count.set(self.busy_controller_count.get() - 1);
            }
            if patch.expand == Some(TestItemExpandState::BusyExpanding) {
                self.busy_controller_count.set(self.busy_controller_count.get() + 1);
            }
        }

        {
            let mut node = existing.borrow_mut();
            let node = node.node_mut();
            if let Some(expand) = patch.expand {
                node.expand = expand;
            }
            if let Some(item) = &patch.item {
                node.item.assign(item);
            }
        }
        changes.update(&existing);
        Some(existing)
    }

    fn remove(&self, item_id: &str, changes: &dyn IncrementalChangeCollector<T>) {
        let Some(to_remove) = self.items.borrow().get(item_id).cloned() else {
            return;
        };

        let to_remove_ext_id = to_remove.borrow().node().item.ext_id.clone();
        match TestId::parent_id_string(&to_remove_ext_id).filter(|id| !id.is_empty()) {
            Some(parent_id) => {
                let parent = self.items.borrow().get(parent_id).cloned();
                if let Some(parent) = parent {
                    parent.borrow_mut().node_mut().children.shift_remove(&to_remove_ext_id);
                }
            }
            None => self.roots.borrow_mut().retain(|root| !Rc::ptr_eq(root, &to_remove)),
        }

        let mut queue: Vec<Vec<String>> = vec![vec![item_id.to_string()]];
        while let Some(ids) = queue.pop() {
            for id in ids {
                let existing = self.items.borrow().get(&id).cloned();
                if let Some(existing) = existing {
                    queue.push(existing.borrow().node().children.iter().cloned().collect());
                    self.items.borrow_mut().remove(&id);
                    changes.remove(&existing, !Rc::ptr_eq(&existing, &to_remove));

                    if existing.borrow().node().expand == TestItemExpandState::BusyExpanding {
                        self.busy_controller_count.set(self.busy_controller_count.get() - 1);
                    }
                }
            }
        }
    }

    /// Updates the number of test root sources who are yet to report. When
    /// the total pending test roots reaches 0, the roots for all controllers
    /// will exist in the collection.
    pub fn update_pending_roots(&self, delta: i32) {
        self.pending_root_count.set(self.pending_root_count.get() + delta);
    }
}
