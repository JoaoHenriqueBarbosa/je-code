//! O ambiente de linguagens e tema do editor: o catálogo das extensões embutidas, o serviço de linguagem, a
//! configuração de linguagem (colchetes, comentários), a tokenização TextMate e o tema de cores de token.
//!
//! É o que no VS Code o workbench monta antes de abrir o primeiro editor (`LanguageService`,
//! `LanguageConfigurationService`, `TextMateTokenizationFeature`, `WorkbenchThemeService`). Fica aqui, e não no
//! `je-code`, para o binário e os testes de captura abrirem o mesmo editor do mesmo jeito.

use std::cell::Cell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use je_base::async_::{IdleDeadline, RunWhenIdle, install_fake_run_when_idle};
use je_base::lifecycle::{DisposableRef, to_disposable};
use je_base::path::Platform;
use je_base::runtime;
use je_base::uri::URI;
use je_editor::language_configuration_registry::{ILanguageConfigurationService, LanguageConfigurationService};
use je_editor::language_service::{ILanguageService, LanguageService};
use je_editor::text_model::{TextModel, TextModelServices, TextSource};
use je_editor::tokenization_registry::tokenization_registry;
use je_editor::undo_redo_service::UndoRedoService;
use je_languages::extensions::ExtensionsCatalog;
use je_languages::language_configuration_file::LanguageConfigurationFileHandler;
use je_languages::text_mate_tokenization_feature::TextMateTokenizationFeature;
use je_languages::token_theme::{ThemeType, TokenTheme};

/// Onde procurar as extensões embutidas (a pasta `extensions` do VS Code), sem caminho fixo: a variável
/// `JE_BUILTIN_EXTENSIONS` e, a partir da pasta do executável e dos seus ancestrais, `extensions`,
/// `resources/app/extensions` (o layout do VS Code instalado) e `ref/vscode/extensions` (o checkout de
/// desenvolvimento, que fica acima de `target/`).
pub fn find_builtin_extensions() -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os("JE_BUILTIN_EXTENSIONS") {
        return Some(PathBuf::from(dir));
    }
    let exe = std::env::current_exe().ok()?;
    exe.ancestors().skip(1).take(5).find_map(|base| {
        ["extensions", "resources/app/extensions", "ref/vscode/extensions"].iter().map(|rel| base.join(rel)).find(|p| p.join("typescript-basics").is_dir())
    })
}

/// Quanto o trabalho em segundo plano (a tokenização em fatias) roda de cada vez, e quanto espera entre as
/// rajadas. O `runWhenGlobalIdle` padrão do porte agenda com `setTimeout(0)` e prazo de 15 ms, mas o loop do
/// runtime não cede a pintura entre timers vencidos: a cadeia de fatias rodaria até o arquivo acabar. Com a
/// espera de 4 ms entre as rajadas o `runtime::turn` devolve o controle ao loop da janela depois de cada
/// uma, e uma rajada de 6 ms cabe num quadro de 60 fps ao lado da pintura.
pub const IDLE_BURST_MS: u64 = 6;
pub const IDLE_GAP_MS: u64 = 4;

fn cooperative_idle() -> RunWhenIdle {
    Rc::new(|runner, _timeout| {
        let disposed = Rc::new(Cell::new(false));
        let flag = disposed.clone();
        runtime::set_timeout(
            move || {
                if flag.get() {
                    return;
                }
                runner(IdleDeadline::new(true, runtime::date_now() + IDLE_BURST_MS));
            },
            IDLE_GAP_MS,
        );
        to_disposable(move || disposed.set(true))
    })
}

pub struct EditorEnvironment {
    language_service: Rc<LanguageService>,
    service: Rc<dyn ILanguageService>,
    configuration_service: Rc<dyn ILanguageConfigurationService>,
    feature: Rc<TextMateTokenizationFeature>,
    // Mantém o carregador dos `language-configuration.json` vivo.
    _configuration_files: Rc<LanguageConfigurationFileHandler>,
    undo_redo: Rc<UndoRedoService>,
    idle: DisposableRef,
}

impl EditorEnvironment {
    /// Lê as extensões de `extensions_dir` e o tema de cores de token de `theme_path`.
    pub fn load(extensions_dir: &Path, theme_path: &Path) -> Result<Self, String> {
        let catalog = ExtensionsCatalog::load(extensions_dir)?;
        let language_service = LanguageService::new(false);
        language_service.set_dynamic_languages(catalog.languages.clone());
        let service: Rc<dyn ILanguageService> = language_service.clone();
        let configuration_service: Rc<dyn ILanguageConfigurationService> = LanguageConfigurationService::new(service.clone());
        let configuration_files = LanguageConfigurationFileHandler::new(service.clone(), configuration_service.clone());
        let feature = TextMateTokenizationFeature::new(service.clone(), &catalog.grammars, catalog.max_tokenization_line_length.clone());
        let stem = theme_path.file_stem().and_then(|s| s.to_str()).unwrap_or("theme");
        let theme = TokenTheme::load_file(theme_path, ThemeType::Dark, stem)?;
        feature.set_theme(&theme);
        Ok(Self {
            language_service,
            service,
            configuration_service,
            feature,
            _configuration_files: configuration_files,
            undo_redo: UndoRedoService::for_tests(),
            idle: install_fake_run_when_idle(cooperative_idle()),
        })
    }

    /// Troca o tema de cores de token (as gramáticas são reaplicadas e o mapa de cores muda).
    /// Troca o tema das cores de token; `ui_theme` é o `uiTheme` do `package.json` (`vs`, `vs-dark`, `hc-black`...).
    pub fn set_theme(&self, theme_path: &Path, ui_theme: &str) -> Result<(), String> {
        let stem = theme_path.file_stem().and_then(|s| s.to_str()).unwrap_or("theme");
        self.feature.set_theme(&TokenTheme::load_file(theme_path, ThemeType::from_ui_theme(ui_theme), stem)?);
        Ok(())
    }

    /// O mapa de cores de token (`#RRGGBB` por id) do tema atual.
    pub fn color_map(&self) -> Vec<String> {
        tokenization_registry().get_color_map().unwrap_or_default()
    }

    /// O id da linguagem de um arquivo pelo nome (extensões, nomes de arquivo, primeira linha).
    pub fn language_id_for(&self, file_name: &str, first_line: Option<&str>) -> String {
        let uri = URI::file(format!("/{file_name}"), Platform::Posix);
        let first = first_line.map(je_base::js_string::JsString::from);
        self.service.guess_language_id_by_filepath_or_first_line(Some(&uri), first.as_ref()).unwrap_or_else(|| "plaintext".to_owned())
    }

    /// O nome de exibição da linguagem (`TypeScript`).
    pub fn language_name(&self, language_id: &str) -> String {
        self.service.get_language_name(language_id).unwrap_or_else(|| "Plain Text".to_owned())
    }

    /// Abre um modelo de texto com a linguagem pelo nome do arquivo. A fábrica de tokenização da linguagem é
    /// assíncrona no original: resolvida aqui antes de criar o modelo, como o `colorize_fixtures`.
    pub fn open(&self, file_name: &str, text: &str) -> Rc<TextModel> {
        let language_id = self.language_id_for(file_name, text.lines().next());
        self.service.request_basic_language_features(&language_id);
        let _ = tokenization_registry().get_or_create(&language_id);
        runtime::run_microtasks();
        let mut options = TextModel::default_creation_options();
        // `editor.detectIndentation` é verdadeiro por padrão: o arquivo manda na tabulação.
        options.detect_indentation = true;
        let uri = URI::file(format!("/{file_name}"), Platform::Posix);
        let model = TextModel::new_with_language_service(
            TextSource::from(text),
            &language_id,
            options,
            Some(uri),
            self.undo_redo.clone(),
            Some(TextModelServices { language_service: self.service.clone(), language_configuration_service: self.configuration_service.clone() }),
        );
        runtime::run_microtasks();
        model
    }
}

impl Drop for EditorEnvironment {
    fn drop(&mut self) {
        use je_base::lifecycle::IDisposable;
        self.idle.dispose();
        self.feature.dispose();
        self.language_service.dispose();
    }
}
