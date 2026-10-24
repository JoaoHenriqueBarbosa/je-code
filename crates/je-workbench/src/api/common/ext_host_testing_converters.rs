//! Porte dos conversores de testes de `src/vs/workbench/api/common/extHostTypeConverters.ts` (`TestMessage`, `TestTag`,
//! `TestRunProfile`, `TestRunProfileKind`, `TestItem`, `TestResults` e `TestCoverage`), entre as classes da API
//! ([`super::ext_host_testing_types`], [`TestItemImpl`]) e os DTOs do `testTypes`.
//!
//! Divergências:
//! - **Formas `Serialized`.** `TestMessage.from` devolve o [`ITestErrorMessageSerialized`] e `TestMessage.to` o lê.
//!   `TestItem.from` devolve o [`ITestItem`] com o URI vivo (é o que a coleção de itens compara e o que
//!   [`ITestItem::serialize`] serializa); `TestItem.toPlain` lê esse mesmo [`ITestItem`] (para a forma serializada,
//!   [`ITestItem::deserialize`] com o [`IdentityCanonicalizer`]). `ILocation` e `ICoverageDetails` são os tipos do editor.
//! - **`MarkdownString.fromStrict(...) || ''`** é [`from_strict`] com o texto vazio como valor padrão.
//! - **Falhas.** Onde o original lança (`Range.to` com número negativo, contagem de cobertura inválida) as funções
//!   devolvem [`Result`], menos `TestItem.toPlain`, que lança com [`throw`] porque o `ITestItem` de entrada é válido.
//! - **`TestMessage.from`** só devolve `location` quando a localização tem URI e intervalo (os dois são obrigatórios em
//!   `IRichLocation`; o original passa `undefined` adiante).
//! - **`TestResultState`** da API não tem o `0` (`Unset`): o estado do snapshot é `None` nesse caso.

use std::cell::RefCell;
use std::rc::Rc;

use je_base::errors::{Error, throw};
use je_base::prefix_tree::{Node, WellDefinedPrefixTree};
use je_base::uri::URI;
use je_editor::languages::StringOrMarkdown;
use je_platform::resources::host_platform;

use super::ext_host_test_item::TestItemImpl;
use super::ext_host_testing_types as api;
use super::ext_host_type_converters::{Location, LocationDto, MarkdownString, Position, Range};
use super::ext_host_types as types;
use crate::contrib::testing::common::test_id::TestId;
use crate::contrib::testing::common::test_types::{
    CoverageCountValue, CoverageDetails, CoverageLocation, IBranchCoverage, ICoverageCount, IDeclarationCoverage, IFileCoverageSerialized,
    IRichLocationSerialized, ISerializedTestResults, IStatementCoverage, ITestErrorMessageSerialized, ITestItem, ITestMessageSerialized,
    ITestMessageStackFrameSerialized, ITestRunProfileReference, ITestTag, ITestUriCanonicalizer, TestMessageText, TestResultItemSerialized,
    TestRunProfileBitset, denamespace_test_tag, namespace_test_tag, test_run_profile_bitset,
};

/// O `{ asCanonicalUri: u => u }` que o host de extensões passa ao `deserialize` dos diffs: a identidade.
pub struct IdentityCanonicalizer;

impl ITestUriCanonicalizer for IdentityCanonicalizer {
    fn as_canonical_uri(&self, uri: &URI) -> URI {
        uri.clone()
    }
}

/// `string | IMarkdownString` do editor para o texto do `testTypes`.
pub fn string_or_markdown_to_text(value: StringOrMarkdown) -> TestMessageText {
    match value {
        StringOrMarkdown::String(text) => TestMessageText::Text(text),
        StringOrMarkdown::Markdown(markdown) => TestMessageText::Markdown(markdown),
    }
}

/// `MarkdownString.fromStrict(error) || null` do `error` de um item.
pub fn error_to_text(error: Option<&super::ext_host_test_item::TestItemError>) -> Option<TestMessageText> {
    MarkdownString::from_strict(error.map(|error| error.to_documentation()).as_ref()).map(string_or_markdown_to_text)
}

/// `typeConverters.TestMessage`.
pub struct TestMessage;

impl TestMessage {
    pub fn from(message: &api::TestMessage) -> ITestErrorMessageSerialized {
        ITestErrorMessageSerialized {
            message: MarkdownString::from_strict(Some(&message.message)).map(string_or_markdown_to_text).unwrap_or_else(|| TestMessageText::Text(String::new())),
            expected: message.expected_output.clone(),
            actual: message.actual_output.clone(),
            context_value: message.context_value.clone(),
            location: message.location.as_ref().and_then(|location| {
                Some(IRichLocationSerialized { range: Range::from(location.range.as_ref()?), uri: location.uri.as_ref()?.to_json() })
            }),
            stack_trace: message.stack_trace.as_ref().map(|frames| {
                frames
                    .iter()
                    .map(|frame| ITestMessageStackFrameSerialized {
                        label: frame.label.clone(),
                        position: frame.position.as_ref().map(Position::from),
                        uri: frame.uri.as_ref().map(URI::to_json),
                    })
                    .collect()
            }),
        }
    }

    /// `TestMessage.from` no formato de mensagem de teste (`ITestMessage.Serialized`) que o protocolo carrega.
    pub fn from_message(message: &api::TestMessage) -> ITestMessageSerialized {
        ITestMessageSerialized::Error(TestMessage::from(message))
    }

    pub fn to(item: &ITestErrorMessageSerialized) -> Result<api::TestMessage, Error> {
        let message = match &item.message {
            TestMessageText::Text(text) => types::Documentation::Text(text.clone()),
            TestMessageText::Markdown(markdown) => types::Documentation::Markdown(MarkdownString::to(markdown)),
        };
        let mut result = api::TestMessage::new(message);
        result.actual_output = item.actual.clone();
        result.expected_output = item.expected.clone();
        result.context_value = item.context_value.clone();
        result.location = match &item.location {
            Some(location) => Some(Location::to(&LocationDto {
                uri: Some(URI::revive(&location.uri, host_platform())),
                range: Some(location.range),
            })?),
            None => None,
        };
        Ok(result)
    }
}

/// `typeConverters.TestTag`.
pub struct TestTag;

impl TestTag {
    pub fn namespace(ctrl_id: &str, tag_id: &str) -> String {
        namespace_test_tag(ctrl_id, tag_id)
    }

    /// `denamespace`: `(ctrlId, tagId)`.
    pub fn denamespace(namespaced: &str) -> (String, String) {
        denamespace_test_tag(namespaced)
    }

    pub fn from(tag: &api::TestTag) -> ITestTag {
        ITestTag { id: tag.id.clone() }
    }

    pub fn to(tag: &ITestTag) -> api::TestTag {
        api::TestTag::new(&tag.id)
    }
}

/// `typeConverters.TestRunProfile`.
pub struct TestRunProfile;

impl TestRunProfile {
    pub fn from(item: &api::TestRunProfileBase) -> ITestRunProfileReference {
        ITestRunProfileReference { controller_id: item.controller_id.clone(), profile_id: item.profile_id, group: TestRunProfileKind::from(item.kind) }
    }
}

/// `typeConverters.TestRunProfileKind`.
pub struct TestRunProfileKind;

impl TestRunProfileKind {
    pub fn from(kind: types::TestRunProfileKind) -> TestRunProfileBitset {
        match kind {
            types::TestRunProfileKind::Coverage => test_run_profile_bitset::COVERAGE,
            types::TestRunProfileKind::Debug => test_run_profile_bitset::DEBUG,
            types::TestRunProfileKind::Run => test_run_profile_bitset::RUN,
        }
    }
}

/// `typeConverters.TestItem`.
pub struct TestItem;

impl TestItem {
    pub fn from(item: &TestItemImpl) -> ITestItem {
        let ctrl_id = item.controller_id();
        ITestItem {
            ext_id: TestId::from_ext_host_test_item(item, &ctrl_id, None).to_string(),
            label: item.label(),
            uri: item.uri(),
            busy: item.busy(),
            tags: item.tags().iter().map(|tag| namespace_test_tag(&ctrl_id, &tag.id)).collect(),
            range: item.range().as_ref().map(Range::from),
            description: item.description().filter(|description| !description.is_empty()),
            sort_text: item.sort_text().filter(|sort_text| !sort_text.is_empty()),
            error: error_to_text(item.error().as_ref()),
        }
    }

    pub fn to_plain(item: &ITestItem) -> api::PlainTestItem {
        api::PlainTestItem {
            id: TestId::from_string(&item.ext_id).local_id().to_string(),
            label: item.label.clone(),
            uri: item.uri.clone(),
            tags: item.tags.iter().map(|tag| api::TestTag::new(&TestTag::denamespace(tag).1)).collect(),
            range: Range::to_optional(item.range.as_ref()).unwrap_or_else(|error| throw(error)),
            busy: item.busy,
            description: item.description.clone().filter(|description| !description.is_empty()),
            sort_text: item.sort_text.clone().filter(|sort_text| !sort_text.is_empty()),
        }
    }
}

/// `typeConverters.TestResults`.
pub struct TestResults;

type ItemNode<'a> = &'a Node<TestResultItemSerialized>;

impl TestResults {
    fn convert_test_result_item(node: ItemNode<'_>, parent: Option<&Rc<api::TestResultSnapshot>>) -> Option<Rc<api::TestResultSnapshot>> {
        let item = node.value.as_ref()?; // should be unreachable

        let plain = ITestItem::deserialize(&IdentityCanonicalizer, &item.item);
        let snapshot = Rc::new(api::TestResultSnapshot {
            item: TestItem::to_plain(&plain),
            parent: parent.map(Rc::downgrade),
            task_states: item
                .tasks
                .iter()
                .map(|task| api::TestSnapshotTaskState {
                    state: types::TestResultState::from_value(task.state.as_number() as i32),
                    duration: task.duration,
                    messages: task
                        .messages
                        .iter()
                        .filter_map(|message| match message {
                            ITestMessageSerialized::Error(error) => Some(TestMessage::to(error).unwrap_or_else(|error| throw(error))),
                            ITestMessageSerialized::Output(_) => None,
                        })
                        .collect(),
                })
                .collect(),
            children: RefCell::new(Vec::new()),
        });

        for (_, child) in node.child_entries() {
            if let Some(converted) = TestResults::convert_test_result_item(child, Some(&snapshot)) {
                snapshot.children.borrow_mut().push(converted);
            }
        }

        Some(snapshot)
    }

    pub fn to(serialized: &ISerializedTestResults) -> api::TestRunResult {
        let mut tree: WellDefinedPrefixTree<TestResultItemSerialized> = WellDefinedPrefixTree::new();
        for item in &serialized.items {
            tree.insert(TestId::from_string(&item.item.ext_id).path().iter().cloned(), item.clone(), None);
        }

        // Get the first node with a value in each subtree of IDs.
        let mut queue: Vec<Vec<ItemNode<'_>>> = vec![tree.nodes().collect()];
        let mut roots: Vec<ItemNode<'_>> = Vec::new();
        while let Some(nodes) = queue.pop() {
            for node in nodes {
                if node.value.is_some() {
                    roots.push(node);
                } else if node.children.is_some() {
                    queue.push(node.child_entries().map(|(_, child)| child).collect());
                }
            }
        }

        api::TestRunResult {
            completed_at: serialized.completed_at,
            results: roots.into_iter().filter_map(|root| TestResults::convert_test_result_item(root, None)).collect(),
            get_detailed_coverage: RefCell::new(None),
        }
    }
}

/// `typeConverters.TestCoverage`.
pub struct TestCoverage;

impl TestCoverage {
    fn from_coverage_count(count: &api::TestCoverageCount) -> ICoverageCount {
        ICoverageCount { covered: count.covered, total: count.total }
    }

    fn from_location(location: &api::ApiCoverageLocation) -> CoverageLocation {
        match location {
            api::ApiCoverageLocation::Position(position) => CoverageLocation::Position(Position::from(position)),
            api::ApiCoverageLocation::Range(range) => CoverageLocation::Range(Range::from(range)),
        }
    }

    fn to_location(location: &CoverageLocation) -> Result<api::ApiCoverageLocation, Error> {
        Ok(match location {
            CoverageLocation::Range(range) => api::ApiCoverageLocation::Range(Range::to(range)?),
            CoverageLocation::Position(position) => api::ApiCoverageLocation::Position(Position::to(position)?),
        })
    }

    pub fn to(serialized: &CoverageDetails) -> Result<api::FileCoverageDetail, Error> {
        match serialized {
            CoverageDetails::Statement(statement) => {
                let mut branches = Vec::new();
                for branch in statement.branches.iter().flatten() {
                    let location = branch.location.as_ref().ok_or_else(|| Error::new("A branch coverage has no location"))?;
                    branches.push(api::BranchCoverage::new(branch.count, TestCoverage::to_location(location)?, branch.label.clone()));
                }
                Ok(api::FileCoverageDetail::Statement(api::StatementCoverage::new(statement.count, TestCoverage::to_location(&statement.location)?, branches)))
            }
            CoverageDetails::Declaration(declaration) => Ok(api::FileCoverageDetail::Declaration(api::DeclarationCoverage::new(
                &declaration.name,
                declaration.count,
                TestCoverage::to_location(&declaration.location)?,
            ))),
        }
    }

    pub fn from_details(coverage: &api::FileCoverageDetail) -> Result<CoverageDetails, Error> {
        let executed = match coverage {
            api::FileCoverageDetail::Statement(statement) => statement.executed,
            api::FileCoverageDetail::Declaration(declaration) => declaration.executed,
        };
        if let CoverageCountValue::Number(count) = executed
            && count < 0.0
        {
            return Err(Error::new(format!("Invalid coverage count {}", je_base::dynamic::number_to_js_string(count))));
        }

        Ok(match coverage {
            api::FileCoverageDetail::Statement(statement) => CoverageDetails::Statement(IStatementCoverage {
                count: statement.executed,
                location: TestCoverage::from_location(&statement.location),
                branches: if statement.branches.is_empty() {
                    None
                } else {
                    Some(
                        statement
                            .branches
                            .iter()
                            .map(|branch| IBranchCoverage {
                                count: branch.executed,
                                location: Some(TestCoverage::from_location(&branch.location)),
                                label: branch.label.clone(),
                            })
                            .collect(),
                    )
                },
            }),
            api::FileCoverageDetail::Declaration(declaration) => CoverageDetails::Declaration(IDeclarationCoverage {
                name: declaration.name.clone(),
                count: declaration.executed,
                location: TestCoverage::from_location(&declaration.location),
            }),
        })
    }

    pub fn from_file(controller_id: &str, id: &str, coverage: &api::FileCoverage) -> Result<IFileCoverageSerialized, Error> {
        api::validate_test_coverage_count(Some(&coverage.statement_coverage))?;
        api::validate_test_coverage_count(coverage.branch_coverage.as_ref())?;
        api::validate_test_coverage_count(coverage.declaration_coverage.as_ref())?;

        Ok(IFileCoverageSerialized {
            id: id.to_string(),
            uri: coverage.uri.to_json(),
            statement: TestCoverage::from_coverage_count(&coverage.statement_coverage),
            branch: coverage.branch_coverage.as_ref().map(TestCoverage::from_coverage_count),
            declaration: coverage.declaration_coverage.as_ref().map(TestCoverage::from_coverage_count),
            test_ids: if coverage.includes_tests.is_empty() {
                None
            } else {
                Some(coverage.includes_tests.iter().map(|test| TestId::from_ext_host_test_item(test, controller_id, None).to_string()).collect())
            },
        })
    }
}
