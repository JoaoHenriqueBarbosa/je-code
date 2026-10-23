//! Benchmark da árvore de pares de colchetes (`BracketPairsTree`) num arquivo de TypeScript de 50 mil
//! linhas. Roda com `cargo bench -p je-editor --bench bracket_pairs_tree` (perfil release) e imprime mediana e
//! mínimo de várias execuções de cada cenário.
//!
//! Cenários:
//! - (a) construir a árvore sem informação de tokens (`FastTokenizer`, a árvore inicial), na primeira consulta
//!   de colchetes do modelo.
//! - (b) construir a árvore com tokens (`TextBufferTokenizer`) de um modelo todo tokenizado.
//! - (c) reaproveitar a árvore com tokens depois de uma edição no meio do arquivo (o `BeforeEditPositionMapper`
//!   e o `NodeReader` reaproveitam os nós de fora do trecho editado), na consulta seguinte.
//! - (d) o mesmo de (c) com a árvore inicial, sem tokens.
//!
//! O arquivo é gerado (classes, métodos, objetos, arrays, comentários), de forma determinística. O tokenizador
//! do modelo põe a linha inteira num token comum com colchetes balanceados, sem separar strings e comentários:
//! o que se mede é a árvore, não a gramática. A construção e a consulta (`getBracketPairsInRange` sobre uma
//! linha) estão dentro do tempo medido; a criação do modelo e a tokenização, não.

use std::hint::black_box;
use std::rc::Rc;
use std::time::{Duration, Instant};

use je_base::JsString;
use je_editor::cursor_common::CursorSimpleModel;
use je_editor::edit_operation::SingleEditOperation;
use je_editor::encoded_token_attributes::{metadata_consts as mc, standard_token_type};
use je_editor::language_configuration::LanguageConfiguration;
use je_editor::language_configuration_registry::{
    ILanguageConfigurationService, LanguageConfigurationService,
};
use je_editor::language_service::{ILanguageService, LanguageService};
use je_editor::languages::{
    EncodedTokenizationResult, ITokenizationSupport, State, TokenizationResult,
};
use je_editor::modes_registry::LanguageExtensionPoint;
use je_editor::null_tokenize::null_state;
use je_editor::range::Range;
use je_editor::text_model::{TextModel, TextModelServices, TextSource};
use je_editor::tokenization_registry::tokenization_registry;
use je_editor::undo_redo_service::UndoRedoService;

const TARGET_LINES: usize = 50_000;
const RUNS: usize = 9;
const LANGUAGE_ID: &str = "typescript-bench";

const BLOCK: &[&str] = &[
    "export class Service{n} extends Base<Item> {",
    "  private readonly items: Map<string, Item[]> = new Map();",
    "  constructor(private readonly deps: Deps, options: { a: number, b: string[] }) {",
    "    super(deps);",
    "    // um comentário com ( parêntese e { chave",
    "  }",
    "  async run(input: Input): Promise<Result> {",
    "    const values = [1, 2, 3].map((x) => ({ value: x * 2, label: `v${x}` }));",
    "    if (values.length > 0 && input.enabled) {",
    "      for (const v of values) { this.items.set(v.label, [v]); }",
    "    }",
    "    return { ok: true, values };",
    "  }",
    "}",
];

fn make_file() -> String {
    let mut text = String::new();
    let mut lines = 0;
    let mut n = 0;
    while lines < TARGET_LINES {
        for line in BLOCK {
            text.push_str(&line.replace("{n}", &n.to_string()));
            text.push('\n');
            lines += 1;
        }
        n += 1;
    }
    text
}

/// Um token comum com colchetes balanceados por linha.
struct LineSupport {
    metadata: u32,
}

impl ITokenizationSupport for LineSupport {
    fn get_initial_state(&self) -> State {
        null_state()
    }

    fn tokenize(&self, _line: &JsString, _has_eol: bool, _state: &State) -> TokenizationResult {
        panic!("tokenize não é usado");
    }

    fn tokenize_encoded(
        &self,
        _line: &JsString,
        _has_eol: bool,
        state: &State,
    ) -> EncodedTokenizationResult {
        EncodedTokenizationResult {
            tokens: vec![0, self.metadata],
            font_info: Vec::new(),
            end_state: state.clone(),
        }
    }
}

struct Fixture {
    model: Rc<TextModel>,
    language_service: Rc<LanguageService>,
    configuration_service: Rc<LanguageConfigurationService>,
    registrations: Vec<Rc<dyn je_base::lifecycle::IDisposable>>,
}

impl Fixture {
    fn new(text: &str, with_tokens: bool) -> Self {
        let language_service = LanguageService::new(false);
        let configuration_service = LanguageConfigurationService::new(language_service.clone());
        let mut registrations: Vec<Rc<dyn je_base::lifecycle::IDisposable>> = Vec::new();
        registrations
            .push(language_service.register_language(LanguageExtensionPoint::new(LANGUAGE_ID)));
        registrations.push(configuration_service.register(
            LANGUAGE_ID,
            LanguageConfiguration {
                brackets: Some(vec![
                    ("{".into(), "}".into()),
                    ("[".into(), "]".into()),
                    ("(".into(), ")".into()),
                ]),
                ..Default::default()
            },
            None,
        ));
        if with_tokens {
            let encoded = language_service
                .language_id_codec()
                .encode_language_id(LANGUAGE_ID);
            let metadata = (encoded << mc::LANGUAGEID_OFFSET)
                | (standard_token_type::OTHER << mc::TOKEN_TYPE_OFFSET)
                | mc::BALANCED_BRACKETS_MASK;
            registrations.push(
                tokenization_registry().register(LANGUAGE_ID, Rc::new(LineSupport { metadata })),
            );
        }
        let model = TextModel::new_with_language_service(
            TextSource::from(text),
            LANGUAGE_ID,
            TextModel::default_creation_options(),
            None,
            UndoRedoService::for_tests(),
            Some(TextModelServices {
                language_service: language_service.clone() as Rc<dyn ILanguageService>,
                language_configuration_service: configuration_service.clone()
                    as Rc<dyn ILanguageConfigurationService>,
            }),
        );
        if with_tokens {
            model
                .tokenization()
                .force_tokenization(model.get_line_count());
        }
        Self {
            model,
            language_service,
            configuration_service,
            registrations,
        }
    }

    /// A consulta que obriga a árvore a existir e a aplicar as edições enfileiradas.
    fn query(&self) -> usize {
        let line = self.model.get_line_count() / 2;
        self.model
            .bracket_pairs()
            .get_bracket_pairs_in_range(Range::new(line, 1, line, 1))
            .to_array()
            .len()
    }

    fn edit_in_the_middle(&self) {
        let line = self.model.get_line_count() / 2;
        self.model.apply_edits(&[SingleEditOperation::new(
            Range::new(line, 1, line, 1),
            Some(JsString::from("{ ")),
            false,
        )]);
        if self.model.tokenization().has_tokens() {
            self.model
                .tokenization()
                .force_tokenization(self.model.get_line_count());
        }
    }

    fn dispose(self) {
        use je_base::lifecycle::IDisposable;
        self.model.dispose();
        for registration in self.registrations {
            registration.dispose();
        }
        self.configuration_service.dispose();
        self.language_service.dispose();
    }
}

fn measure(name: &str, mut run: impl FnMut() -> Duration) -> Duration {
    let mut times: Vec<Duration> = (0..RUNS).map(|_| run()).collect();
    times.sort();
    println!(
        "{name:<70} mediana {:>10.3} ms   mínimo {:>10.3} ms",
        times[RUNS / 2].as_secs_f64() * 1e3,
        times[0].as_secs_f64() * 1e3
    );
    times[RUNS / 2]
}

fn main() {
    let text = make_file();
    println!(
        "arquivo: {} linhas, {} unidades",
        TARGET_LINES,
        text.encode_utf16().count()
    );

    let a = measure("(a) construir a árvore sem tokens (FastTokenizer)", || {
        let fixture = Fixture::new(&text, false);
        let start = Instant::now();
        black_box(fixture.query());
        let elapsed = start.elapsed();
        fixture.dispose();
        elapsed
    });

    let b = measure(
        "(b) construir a árvore com tokens (TextBufferTokenizer)",
        || {
            let fixture = Fixture::new(&text, true);
            let start = Instant::now();
            black_box(fixture.query());
            let elapsed = start.elapsed();
            fixture.dispose();
            elapsed
        },
    );

    let c = measure(
        "(c) reaproveitar a árvore com tokens depois de uma edição no meio",
        || {
            let fixture = Fixture::new(&text, true);
            black_box(fixture.query());
            fixture.edit_in_the_middle();
            let start = Instant::now();
            black_box(fixture.query());
            let elapsed = start.elapsed();
            fixture.dispose();
            elapsed
        },
    );

    let d = measure(
        "(d) reaproveitar a árvore sem tokens depois de uma edição no meio",
        || {
            let fixture = Fixture::new(&text, false);
            black_box(fixture.query());
            fixture.edit_in_the_middle();
            let start = Instant::now();
            black_box(fixture.query());
            let elapsed = start.elapsed();
            fixture.dispose();
            elapsed
        },
    );

    println!(
        "razão construção/reaproveitamento com tokens: {:.0}x ((b)/(c))",
        b.as_secs_f64() / c.as_secs_f64()
    );
    println!(
        "razão construção/reaproveitamento sem tokens: {:.0}x ((a)/(d))",
        a.as_secs_f64() / d.as_secs_f64()
    );
}
