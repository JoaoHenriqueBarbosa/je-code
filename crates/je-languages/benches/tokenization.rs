//! Benchmark da tokenização TextMate pelo `TextModel` num arquivo TypeScript grande. Roda com
//! `cargo bench -p je-languages --bench tokenization` (perfil release) e imprime mediana e mínimo de várias
//! execuções de cada cenário, em linhas por segundo.
//!
//! O arquivo é `ref/vscode/src/vs/editor/common/model/pieceTreeTextBuffer/pieceTreeBase.ts` repetido até
//! passar de 50 000 linhas.
//!
//! Cenários:
//! - (a) carga: ler e compilar as 83 gramáticas e criar a do TypeScript (uma vez por processo).
//! - (b) tokenização inicial: `forceTokenization(última linha)` num modelo novo, todas as linhas.
//! - (c) o limite inferior de (b): o `tokenizeLine2` do ferriki direto sobre as mesmas linhas, sem modelo,
//!   registro nem armazenamento de tokens (quanto custa o motor de gramática sozinho).
//! - (d) retokenização depois de uma edição inócua na linha 1 (insere uma letra num identificador): o estado
//!   de fim da linha 1 não muda, então só a linha editada é retokenizada. Medida em latência.
//! - (e) retokenização depois de uma edição na linha 1 que muda o estado (abre uma template string com uma
//!   crase): as linhas seguintes mudam de estado e são retokenizadas até o estado voltar a convergir. E o
//!   desfazer (apagar a crase), que cascateia de volta. A vazão é em linhas com tokens alterados.

use std::fs;
use std::path::Path;
use std::rc::Rc;
use std::time::{Duration, Instant};

use je_base::JsString;
use je_base::lifecycle::IDisposable;
use je_base::runtime;
use je_base::uri::URI;
use je_editor::edit_operation::SingleEditOperation;
use je_editor::language_configuration_registry::{ILanguageConfigurationService, LanguageConfigurationService};
use je_editor::language_service::{ILanguageService, LanguageService};
use je_editor::range::Range;
use je_editor::text_model::{TextModel, TextModelServices, TextSource};
use je_editor::tokenization_registry::tokenization_registry;
use je_editor::undo_redo_service::UndoRedoService;
use je_languages::extensions::ExtensionsCatalog;
use je_languages::jsonc;
use je_languages::text_mate_tokenization_feature::TextMateTokenizationFeature;
use je_languages::token_theme::TokenTheme;

const MIN_LINES: usize = 50_000;
const RUNS: usize = 7;

fn median_and_min(mut samples: Vec<Duration>) -> (Duration, Duration) {
    samples.sort();
    (samples[samples.len() / 2], samples[0])
}

fn report(name: &str, lines: usize, samples: Vec<Duration>) {
    let (median, min) = median_and_min(samples);
    println!(
        "{name}: mediana {:.1} ms ({:.0} linhas/s), mínimo {:.1} ms ({:.0} linhas/s)",
        median.as_secs_f64() * 1000.0,
        lines as f64 / median.as_secs_f64(),
        min.as_secs_f64() * 1000.0,
        lines as f64 / min.as_secs_f64()
    );
}

fn main() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let extensions = root.join("ref/vscode/extensions");
    let source = fs::read_to_string(root.join("ref/vscode/src/vs/editor/common/model/pieceTreeTextBuffer/pieceTreeBase.ts"))
        .expect("pieceTreeBase.ts");
    let unit_lines = source.lines().count();
    let copies = MIN_LINES.div_ceil(unit_lines);
    let text = source.repeat(copies);
    let line_count = text.lines().count();
    println!("arquivo: {line_count} linhas, {:.2} MiB ({copies} cópias de {unit_lines} linhas)", text.len() as f64 / 1_048_576.0);

    // (a) carga
    let started = Instant::now();
    let catalog = ExtensionsCatalog::load(&extensions).expect("catálogo");
    let language_service = LanguageService::new(false);
    language_service.set_dynamic_languages(catalog.languages.clone());
    let service: Rc<dyn ILanguageService> = language_service.clone();
    let configuration_service: Rc<dyn ILanguageConfigurationService> = LanguageConfigurationService::new(service.clone());
    let catalog_time = started.elapsed();
    let feature = TextMateTokenizationFeature::new(service.clone(), &catalog.grammars, catalog.max_tokenization_line_length.clone());
    let dark_plus_dir = extensions.join("theme-defaults");
    let manifest = jsonc::parse(&fs::read_to_string(dark_plus_dir.join("package.json")).expect("package.json")).expect("json");
    let theme_entry = manifest["contributes"]["themes"]
        .as_array()
        .and_then(|t| t.iter().find(|e| e["path"].as_str().is_some_and(|p| p.contains("dark_plus"))))
        .expect("tema dark_plus");
    let theme = TokenTheme::load(&dark_plus_dir, theme_entry).expect("tema");
    feature.set_theme(&theme);

    let uri = URI::file("/pieceTreeBase.ts", je_base::path::Platform::Posix);
    let language_id = service.guess_language_id_by_filepath_or_first_line(Some(&uri), None).expect("linguagem");
    assert_eq!(language_id, "typescript");
    let started = Instant::now();
    let _ = tokenization_registry().get_or_create(&language_id);
    runtime::run_microtasks();
    println!(
        "(a) catálogo de {} linguagens e {} gramáticas: {:.1} ms; gramáticas lidas e a do TypeScript criada: {:.1} ms",
        catalog.languages.len(),
        catalog.grammars.len(),
        catalog_time.as_secs_f64() * 1000.0,
        started.elapsed().as_secs_f64() * 1000.0
    );
    assert!(tokenization_registry().get(&language_id).is_some(), "sem suporte de tokenização para {language_id}");
    feature.set_theme(&theme);

    let new_model = || {
        TextModel::new_with_language_service(
            TextSource::from(text.as_str()),
            &language_id,
            TextModel::default_creation_options(),
            Some(uri.clone()),
            UndoRedoService::for_tests(),
            Some(TextModelServices { language_service: service.clone(), language_configuration_service: configuration_service.clone() }),
        )
    };
    let last_line = |m: &TextModel| m.get_line_count();

    // (b) tokenização inicial
    let mut samples = Vec::new();
    for _ in 0..RUNS {
        let model = new_model();
        assert!(!model.is_too_large_for_tokenization());
        let started = Instant::now();
        model.tokenization().force_tokenization(last_line(&model));
        samples.push(started.elapsed());
        model.dispose();
    }
    report("(b) tokenização inicial pelo TextModel", line_count, samples);

    // (c) o motor sozinho
    let grammar = feature.create_tokenizer(&language_id).expect("gramática");
    let lines: Vec<&str> = text.lines().collect();
    let mut samples = Vec::new();
    for _ in 0..RUNS {
        let started = Instant::now();
        let mut state = None;
        for line in &lines {
            let result = grammar.tokenize_line2(line, state, 500).expect("tokenizeLine2");
            state = Some(result.rule_stack);
        }
        samples.push(started.elapsed());
    }
    report("(c) ferriki tokenizeLine2 direto, sem modelo", line_count, samples);

    // (d) edição inócua na linha 1 e (e) edição que muda o estado
    let model = new_model();
    model.tokenization().force_tokenization(last_line(&model));
    let first_word_end = model.get_line_content(1).len() as i32 + 1;
    let edit = |model: &TextModel, range: Range, text: &str| {
        model.apply_edits(&[SingleEditOperation::new(range, Some(JsString::from(text)), false)]);
    };

    let mut samples = Vec::new();
    for _ in 0..RUNS {
        edit(&model, Range::new(1, first_word_end, 1, first_word_end), "x");
        let started = Instant::now();
        model.tokenization().force_tokenization(last_line(&model));
        samples.push(started.elapsed());
    }
    let (median, min) = median_and_min(samples);
    println!(
        "(d) edição inócua na linha 1 (1 linha retokenizada): mediana {:.3} ms, mínimo {:.3} ms",
        median.as_secs_f64() * 1000.0,
        min.as_secs_f64() * 1000.0
    );

    // A linha 1 do arquivo já abre um comentário de bloco, então um `/*` ali não muda o estado. Uma crase
    // abre uma template string que o resto do arquivo não fecha na mesma paridade: o estado muda em toda linha.
    let changed_lines = Rc::new(std::cell::Cell::new(0usize));
    let counter = changed_lines.clone();
    let subscription = model.on_did_change_tokens().listen(move |e| {
        counter.set(counter.get() + e.ranges.iter().map(|r| (r.to_line_number - r.from_line_number + 1) as usize).sum::<usize>());
    });
    let (mut open_samples, mut close_samples) = (Vec::new(), Vec::new());
    let (mut open_lines, mut close_lines) = (0, 0);
    for _ in 0..RUNS {
        edit(&model, Range::new(1, 1, 1, 1), "`");
        changed_lines.set(0);
        let started = Instant::now();
        model.tokenization().force_tokenization(last_line(&model));
        open_samples.push(started.elapsed());
        open_lines = changed_lines.get();

        edit(&model, Range::new(1, 1, 1, 2), "");
        changed_lines.set(0);
        let started = Instant::now();
        model.tokenization().force_tokenization(last_line(&model));
        close_samples.push(started.elapsed());
        close_lines = changed_lines.get();
    }
    subscription.dispose();
    println!("(e) linhas com tokens alterados: {open_lines} ao abrir a crase, {close_lines} ao apagá-la (o número de linhas retokenizadas é este mais a linha que confirma a convergência)");
    report("(e) retokenização depois de abrir uma crase na linha 1 (cascata)", open_lines, open_samples);
    report("(e) retokenização depois de apagar a crase (cascata de volta)", close_lines, close_samples);

    model.dispose();
    feature.dispose();
    language_service.dispose();
}
