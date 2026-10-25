//! Porte de `src/vs/platform/terminal/test/common/capabilities/commandDetection/promptInputModel.test.ts` (suíte
//! `PromptInputModel`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - O `Terminal` do `@xterm/headless` é o [`HeadlessXterm`] de `common::headless_xterm`, um emulador que escreve as
//!   mesmas sequências do teste (cores, atributos, caracteres largos, rolagem, `ICH`, `ECH`, `EL`...) com a semântica
//!   do xterm.js. `xterm.write(data, r)` é síncrono ([`HeadlessXterm::write`] interpreta o texto e dispara
//!   `onCursorMove` e `onWriteParsed`), então `writePromise` é [`Fixture::write`].
//! - Todo caso roda sob o relógio virtual de [`async_test`] (o original só usa `runWithFakedTimers` em alguns), e o
//!   `timeout(0)` do `assertPromptInput` é o `timeout` virtual. O guarda de vazamentos é o do `async_test`.
//! - `fireCommandStart` dispara o `StartedCommand` com um marcador na linha do cursor (`xterm.registerMarker()`),
//!   e `fireCommandExecuted` e `fireCommandFinished` disparam o mesmo literal sem marcador (o `null!` do original,
//!   que o modelo nunca lê).
//! - Os índices do cursor e do texto fantasma contam unidades UTF-16 como o `indexOf` do JavaScript, e o `assertPromptInput`
//!   compara o `value` como texto.
//! - O caso do relógio do sistema que volta atrás move o `Date` do loop virtual para 10 s antes, como o
//!   `useFakeTimers({ toFake: ['Date'] })` do sinon, e o restaura no fim.
//! - O `onDidInterrupt` seguido do `onDidFinishInput` do original (a promessa que o segundo resolve) é conferido por duas
//!   flags: a segunda só vale depois da primeira.
//! - `test.skip` é `#[ignore]`.

mod common;

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use common::files::async_test;
use common::headless_xterm::HeadlessXterm;
use je_base::async_::timeout;
use je_base::event::Emitter;
use je_base::lifecycle::DisposableStore;
use je_base::runtime;
use je_platform::log::NullLogService;
use je_platform::terminal::capabilities::capabilities::ITerminalCommand;
use je_platform::terminal::capabilities::command_detection::prompt_input_model::{IPromptInputModel, IPromptInputModelState, PromptInputModel};
use je_platform::terminal::capabilities::command_detection::terminal_command::StartedCommand;
use je_platform::terminal::terminal::{GeneralShellType, PosixShellType, ShellType};
use je_platform::terminal::xterm_headless::IHeadlessTerminal;

/// O `setup` da suíte: o terminal, os emissores de comando e o `PromptInputModel`.
struct Fixture {
    xterm: Rc<HeadlessXterm>,
    model: Rc<PromptInputModel>,
    on_command_start: Rc<Emitter<Rc<dyn ITerminalCommand>>>,
    on_command_executed: Rc<Emitter<Rc<dyn ITerminalCommand>>>,
    on_command_finished: Rc<Emitter<Rc<dyn ITerminalCommand>>>,
    _on_command_start_changed: Rc<Emitter<()>>,
}

impl Fixture {
    fn new(d: &Rc<DisposableStore>) -> Self {
        let xterm = d.add_rc(HeadlessXterm::new());
        let on_command_start: Rc<Emitter<Rc<dyn ITerminalCommand>>> = d.add_rc(Emitter::new());
        let on_command_start_changed: Rc<Emitter<()>> = d.add_rc(Emitter::new());
        let on_command_executed: Rc<Emitter<Rc<dyn ITerminalCommand>>> = d.add_rc(Emitter::new());
        let on_command_finished: Rc<Emitter<Rc<dyn ITerminalCommand>>> = d.add_rc(Emitter::new());
        let model = d.add_rc(PromptInputModel::new(
            xterm.clone(),
            on_command_start.event(),
            on_command_start_changed.event(),
            on_command_executed.event(),
            on_command_finished.event(),
            NullLogService::shared(),
        ));
        Self { xterm, model, on_command_start, on_command_executed, on_command_finished, _on_command_start_changed: on_command_start_changed }
    }

    /// `writePromise(data)`.
    fn write(&self, data: &str) {
        self.xterm.write(data);
    }

    /// O comando que os emissores disparam: `{ marker } as ITerminalCommand`.
    fn command(marker: Option<Rc<dyn je_platform::terminal::capabilities::capabilities::IMarker>>) -> Rc<dyn ITerminalCommand> {
        Rc::new(StartedCommand { marker, mark_properties: None })
    }

    fn fire_command_start(&self) {
        self.on_command_start.fire(Self::command(self.xterm.register_marker(0)));
    }

    fn fire_command_executed(&self) {
        self.on_command_executed.fire(Self::command(None));
    }

    fn fire_command_finished(&self) {
        self.on_command_finished.fire(Self::command(None));
    }

    fn set_continuation_prompt(&self, prompt: &str) {
        self.model.set_continuation_prompt(prompt);
    }

    async fn assert_prompt_input(&self, value_with_cursor: &str) {
        timeout(0).await.unwrap();

        if self.model.cursor_index() != -1 && !value_with_cursor.contains('|') {
            panic!("assertPromptInput must contain | character");
        }

        let actual_value_with_cursor = self.model.get_combined_string(None).to_string_lossy();
        assert_eq!(actual_value_with_cursor, value_with_cursor.replace('\n', "\u{23CE}"));

        // This is required to ensure the cursor index is correctly resolved for non-ascii characters
        let value: String = value_with_cursor.chars().filter(|c| !matches!(c, '|' | '[' | ']')).collect();
        let cursor_index = value_with_cursor.find('|').map_or(-1, |byte_index| i32::try_from(value_with_cursor[..byte_index].encode_utf16().count()).unwrap());
        assert_eq!(self.model.value().to_string_lossy(), value);
        assert_eq!(self.model.cursor_index(), cursor_index, "value={}", self.model.value());
        let ghost_text_index = self.model.ghost_text_index();
        assert!(
            ghost_text_index == -1 || cursor_index <= ghost_text_index,
            "cursorIndex ({cursor_index}) must be before ghostTextIndex ({ghost_text_index})"
        );
    }
}

/// Um caso da suíte: `$t` é o [`Fixture`] e `$d` o store do guarda de vazamentos.
macro_rules! prompt_test {
    ($(#[$meta:meta])* $name:ident, |$t:ident, $d:ident| $body:block) => {
        $(#[$meta])*
        #[test]
        fn $name() {
            async_test(|$d| async move {
                let $t = Fixture::new(&$d);
                let _ = &$d;
                $body
            });
        }
    };
}

/// A hora do relógio de parede em milissegundos desde 1970 (o `Date.now()` real).
fn epoch_ms() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |elapsed| u64::try_from(elapsed.as_millis()).unwrap_or(0))
}

prompt_test!(basic_input_and_execute, |t, d| {
    t.write("$ ");
    t.fire_command_start();
    t.assert_prompt_input("|").await;

    t.write("foo bar");
    t.assert_prompt_input("foo bar|").await;

    t.write("\r\n");
    t.fire_command_executed();
    t.assert_prompt_input("foo bar").await;

    t.write("(command output)\r\n$ ");
    t.fire_command_start();
    t.assert_prompt_input("|").await;
});

prompt_test!(should_not_fire_on_did_change_input_events_when_nothing_changes, |t, d| {
    let events: Rc<RefCell<Vec<IPromptInputModelState>>> = Rc::default();
    let recorded = events.clone();
    d.add(t.model.on_did_change_input().listen(move |e| recorded.borrow_mut().push(e.clone())));

    t.write("$ ");
    t.fire_command_start();
    t.assert_prompt_input("|").await;

    t.write("foo");
    t.assert_prompt_input("foo|").await;

    t.write(" bar");
    t.assert_prompt_input("foo bar|").await;

    t.write("\r\n");
    t.fire_command_executed();
    t.assert_prompt_input("foo bar").await;

    t.write("$ ");
    t.fire_command_start();
    t.assert_prompt_input("|").await;

    t.write("foo bar");
    t.assert_prompt_input("foo bar|").await;

    for pair in events.borrow().windows(2) {
        assert_ne!(pair[0], pair[1], "not adjacent events should fire with the same value");
    }
});

prompt_test!(should_fire_on_did_interrupt_followed_by_on_did_finish_when_ctrl_c_is_pressed, |t, d| {
    t.write("$ ");
    t.fire_command_start();
    t.assert_prompt_input("|").await;

    t.write("foo");
    t.assert_prompt_input("foo|").await;

    let interrupted = Rc::new(Cell::new(false));
    let finished = Rc::new(Cell::new(false));
    let on_interrupt = interrupted.clone();
    d.add(t.model.on_did_interrupt().listen(move |_| on_interrupt.set(true)));
    // Fire onDidFinishInput immediately after onDidInterrupt
    let (after_interrupt, on_finish) = (interrupted.clone(), finished.clone());
    d.add(t.model.on_did_finish_input().listen(move |_| {
        if after_interrupt.get() {
            on_finish.set(true);
        }
    }));
    t.xterm.input("\x03");
    t.write("^C");
    t.fire_command_executed();
    assert!(finished.get(), "o onDidFinishInput deve disparar logo depois do onDidInterrupt");
});

prompt_test!(should_clear_value_when_command_finishes, |t, d| {
    t.write("$ ");
    t.fire_command_start();
    t.assert_prompt_input("|").await;

    t.write("echo hello");
    t.assert_prompt_input("echo hello|").await;

    t.fire_command_executed();
    assert_eq!(t.model.value().to_string_lossy(), "echo hello");

    t.fire_command_finished();
    assert_eq!(t.model.value().to_string_lossy(), "");
});

prompt_test!(should_update_input_when_the_system_clock_moves_backwards, |t, d| {
    t.write("$ ");
    t.fire_command_start();
    t.assert_prompt_input("|").await;

    t.write("foo");
    t.assert_prompt_input("foo|").await;

    runtime::set_system_time(epoch_ms().saturating_sub(10_000));
    t.write(" bar");
    t.assert_prompt_input("foo bar|").await;
    runtime::set_system_time(epoch_ms());
});

prompt_test!(cursor_navigation, |t, d| {
    t.write("$ ");
    t.fire_command_start();
    t.assert_prompt_input("|").await;

    t.write("foo bar");
    t.assert_prompt_input("foo bar|").await;

    t.write("\x1b[3D");
    t.assert_prompt_input("foo |bar").await;

    t.write("\x1b[4D");
    t.assert_prompt_input("|foo bar").await;

    t.write("\x1b[3C");
    t.assert_prompt_input("foo| bar").await;

    t.write("\x1b[4C");
    t.assert_prompt_input("foo bar|").await;

    t.write("\x1b[D");
    t.assert_prompt_input("foo ba|r").await;

    t.write("\x1b[C");
    t.assert_prompt_input("foo bar|").await;
});

mod ghost_text {
    use super::*;

    prompt_test!(basic_ghost_text, |t, d| {
        t.write("$ ");
        t.fire_command_start();
        t.assert_prompt_input("|").await;

        t.write("foo\x1b[2m bar\x1b[0m\x1b[4D");
        t.assert_prompt_input("foo|[ bar]").await;

        t.write("\x1b[2D");
        t.assert_prompt_input("f|oo[ bar]").await;
    });

    prompt_test!(trailing_whitespace, |t, d| {
        t.write("$ ");
        t.fire_command_start();
        t.assert_prompt_input("|").await;
        t.write("foo    ");
        t.write("\x1b[4D");
        t.assert_prompt_input("foo|    ").await;
    });

    prompt_test!(basic_ghost_text_one_word, |t, d| {
        t.write("$ ");
        t.fire_command_start();
        t.assert_prompt_input("|").await;

        t.write("pw\x1b[2md\x1b[1D");
        t.assert_prompt_input("pw|[d]").await;
    });

    prompt_test!(ghost_text_with_cursor_navigation, |t, d| {
        t.write("$ ");
        t.fire_command_start();
        t.assert_prompt_input("|").await;

        t.write("foo\x1b[2m bar\x1b[0m\x1b[4D");
        t.assert_prompt_input("foo|[ bar]").await;

        t.write("\x1b[2D");
        t.assert_prompt_input("f|oo[ bar]").await;

        t.write("\x1b[C");
        t.assert_prompt_input("fo|o[ bar]").await;

        t.write("\x1b[C");
        t.assert_prompt_input("foo|[ bar]").await;
    });

    prompt_test!(ghost_text_with_different_foreground_colors_only, |t, d| {
        t.write("$ ");
        t.fire_command_start();
        t.assert_prompt_input("|").await;

        t.write("foo\x1b[38;2;255;0;0m bar\x1b[0m\x1b[4D");
        t.assert_prompt_input("foo|[ bar]").await;

        t.write("\x1b[2D");
        t.assert_prompt_input("f|oo[ bar]").await;
    });

    prompt_test!(no_ghost_text_when_foreground_color_matches_earlier_text, |t, d| {
        t.write("$ ");
        t.fire_command_start();
        t.assert_prompt_input("|").await;

        t.write(concat!(
            "\x1b[38;2;255;0;0mred1\x1b[0m ", // Red "red1"
            "\x1b[38;2;0;255;0mgreen\x1b[0m ", // Green "green"
            "\x1b[38;2;255;0;0mred2\x1b[0m"    // Red "red2" (same as red1)
        ));

        t.assert_prompt_input("red1 green red2|").await; // No ghost text expected
    });

    prompt_test!(ghost_text_detected_when_foreground_color_is_unique_at_the_end, |t, d| {
        t.write("$ ");
        t.fire_command_start();
        t.assert_prompt_input("|").await;

        t.write(concat!(
            "\x1b[38;2;255;0;0mcmd\x1b[0m ",  // Red "cmd"
            "\x1b[38;2;0;255;0marg\x1b[0m ",  // Green "arg"
            "\x1b[38;2;0;0;255mfinal\x1b[5D"  // Blue "final" (ghost text)
        ));

        t.assert_prompt_input("cmd arg |[final]").await;
    });

    prompt_test!(no_ghost_text_when_background_color_matches_earlier_text, |t, d| {
        t.write("$ ");
        t.fire_command_start();
        t.assert_prompt_input("|").await;

        t.write(concat!(
            "\x1b[48;2;255;0;0mred_bg1\x1b[0m ",  // Red background
            "\x1b[48;2;0;255;0mgreen_bg\x1b[0m ", // Green background
            "\x1b[48;2;255;0;0mred_bg2\x1b[0m"    // Red background again
        ));

        t.assert_prompt_input("red_bg1 green_bg red_bg2|").await; // No ghost text expected
    });

    prompt_test!(ghost_text_detected_when_background_color_is_unique_at_the_end, |t, d| {
        t.write("$ ");
        t.fire_command_start();
        t.assert_prompt_input("|").await;

        t.write(concat!(
            "\x1b[48;2;255;0;0mred_bg\x1b[0m ",  // Red background
            "\x1b[48;2;0;255;0mgreen_bg\x1b[0m ", // Green background
            "\x1b[48;2;0;0;255mblue_bg\x1b[7D"    // Blue background (ghost text)
        ));

        t.assert_prompt_input("red_bg green_bg |[blue_bg]").await;
    });

    prompt_test!(ghost_text_detected_when_bold_style_is_unique_at_the_end, |t, d| {
        t.write("$ ");
        t.fire_command_start();
        t.assert_prompt_input("|").await;

        t.write(concat!(
            "text ",
            "\x1b[1mBOLD\x1b[4D" // Bold "BOLD" (ghost text)
        ));

        t.assert_prompt_input("text |[BOLD]").await;
    });

    prompt_test!(no_ghost_text_when_earlier_text_has_the_same_bold_style, |t, d| {
        t.write("$ ");
        t.fire_command_start();
        t.assert_prompt_input("|").await;

        t.write(concat!(
            "\x1b[1mBOLD1\x1b[0m ", // Bold "BOLD1"
            "normal ",
            "\x1b[1mBOLD2\x1b[0m"   // Bold "BOLD2" (same style as "BOLD1")
        ));

        t.assert_prompt_input("BOLD1 normal BOLD2|").await; // No ghost text expected
    });

    prompt_test!(ghost_text_detected_when_italic_style_is_unique_at_the_end, |t, d| {
        t.write("$ ");
        t.fire_command_start();
        t.assert_prompt_input("|").await;

        t.write(concat!(
            "text ",
            "\x1b[3mITALIC\x1b[6D" // Italic "ITALIC" (ghost text)
        ));

        t.assert_prompt_input("text |[ITALIC]").await;
    });

    prompt_test!(no_ghost_text_when_earlier_text_has_the_same_italic_style, |t, d| {
        t.write("$ ");
        t.fire_command_start();
        t.assert_prompt_input("|").await;

        t.write(concat!(
            "\x1b[3mITALIC1\x1b[0m ", // Italic "ITALIC1"
            "normal ",
            "\x1b[3mITALIC2\x1b[0m"   // Italic "ITALIC2" (same style as "ITALIC1")
        ));

        t.assert_prompt_input("ITALIC1 normal ITALIC2|").await; // No ghost text expected
    });

    prompt_test!(ghost_text_detected_when_underline_style_is_unique_at_the_end, |t, d| {
        t.write("$ ");
        t.fire_command_start();
        t.assert_prompt_input("|").await;

        t.write(concat!(
            "text ",
            "\x1b[4mUNDERLINE\x1b[9D" // Underlined "UNDERLINE" (ghost text)
        ));

        t.assert_prompt_input("text |[UNDERLINE]").await;
    });

    prompt_test!(no_ghost_text_when_earlier_text_has_the_same_underline_style, |t, d| {
        t.write("$ ");
        t.fire_command_start();
        t.assert_prompt_input("|").await;

        t.write(concat!(
            "\x1b[4mUNDERLINE1\x1b[0m ", // Underlined "UNDERLINE1"
            "normal ",
            "\x1b[4mUNDERLINE2\x1b[0m"   // Underlined "UNDERLINE2" (same style as "UNDERLINE1")
        ));

        t.assert_prompt_input("UNDERLINE1 normal UNDERLINE2|").await; // No ghost text expected
    });

    prompt_test!(ghost_text_detected_when_strikethrough_style_is_unique_at_the_end, |t, d| {
        t.write("$ ");
        t.fire_command_start();
        t.assert_prompt_input("|").await;

        t.write(concat!(
            "text ",
            "\x1b[9mSTRIKE\x1b[6D" // Strikethrough "STRIKE" (ghost text)
        ));

        t.assert_prompt_input("text |[STRIKE]").await;
    });

    prompt_test!(no_ghost_text_when_earlier_text_has_the_same_strikethrough_style, |t, d| {
        t.write("$ ");
        t.fire_command_start();
        t.assert_prompt_input("|").await;

        t.write(concat!(
            "\x1b[9mSTRIKE1\x1b[0m ", // Strikethrough "STRIKE1"
            "normal ",
            "\x1b[9mSTRIKE2\x1b[0m"   // Strikethrough "STRIKE2" (same style as "STRIKE1")
        ));

        t.assert_prompt_input("STRIKE1 normal STRIKE2|").await; // No ghost text expected
    });

    mod with_wrapping {
        use super::*;

        prompt_test!(fish_ghost_text_in_long_line_with_wrapped_content, |t, d| {
            t.model.set_shell_type(Some(ShellType::Posix(PosixShellType::Fish)));
            t.write("$ ");
            t.fire_command_start();
            t.assert_prompt_input("|").await;

            // Write a command with ghost text that will wrap
            t.write("find . -name");
            t.assert_prompt_input("find . -name|").await;

            // Add ghost text with dim style
            t.write("\x1b[2m test\x1b[0m\x1b[4D");
            t.assert_prompt_input("find . -name |[test]").await;

            // Move cursor within the ghost text
            t.write("\x1b[C");
            t.assert_prompt_input("find . -name t|[est]").await;

            // Accept ghost text
            t.write("\x1b[C\x1b[C\x1b[C\x1b[C\x1b[C");
            t.assert_prompt_input("find . -name test|").await;
        });

        prompt_test!(pwsh_ghost_text_in_long_line_with_wrapped_content, |t, d| {
            t.model.set_shell_type(Some(ShellType::General(GeneralShellType::PowerShell)));
            t.write("$ ");
            t.fire_command_start();
            t.assert_prompt_input("|").await;

            // Write a command with ghost text that will wrap
            t.write("find . -name");
            t.assert_prompt_input("find . -name|").await;

            // Add ghost text with dim style
            t.write("\x1b[2m test\x1b[0m\x1b[4D");
            t.assert_prompt_input("find . -name |[test]").await;

            // Move cursor within the ghost text
            t.write("\x1b[C");
            t.assert_prompt_input("find . -name t|[est]").await;

            // Accept ghost text
            t.write("\x1b[C\x1b[C\x1b[C\x1b[C\x1b[C");
            t.assert_prompt_input("find . -name test|").await;
        });
    }

    prompt_test!(does_not_detect_right_prompt_as_ghost_text, |t, d| {
        t.write("$ ");
        t.fire_command_start();
        t.assert_prompt_input("|").await;
        t.write(&format!("cmd{}\x1b[38;2;255;0;0mRP\x1b[0m\x1b[8D", " ".repeat(6)));
        t.assert_prompt_input(&format!("cmd|{}RP", " ".repeat(6))).await;
    });
}

prompt_test!(wide_input_korean, |t, d| {
    t.write("$ ");
    t.fire_command_start();
    t.assert_prompt_input("|").await;

    t.write("안영");
    t.assert_prompt_input("안영|").await;

    t.write("\r\n컴퓨터");
    t.assert_prompt_input("안영\n컴퓨터|").await;

    t.write("\r\n사람");
    t.assert_prompt_input("안영\n컴퓨터\n사람|").await;

    t.write("\x1b[G");
    t.assert_prompt_input("안영\n컴퓨터\n|사람").await;

    t.write("\x1b[A");
    t.assert_prompt_input("안영\n|컴퓨터\n사람").await;

    t.write("\x1b[4C");
    t.assert_prompt_input("안영\n컴퓨|터\n사람").await;

    t.write("\x1b[1;4H");
    t.assert_prompt_input("안|영\n컴퓨터\n사람").await;

    t.write("\x1b[D");
    t.assert_prompt_input("|안영\n컴퓨터\n사람").await;
});

/// Os emoji do caso `emoji input`, escritos pelos pontos de código (a sequência `✌️` é o `U+270C` com o seletor de
/// variação `U+FE0F`).
const PEACE: &str = "\u{270C}\u{FE0F}";
const THUMBS_UP: &str = "\u{1F44D}";
const SUNGLASSES: &str = "\u{1F60E}";
const CONFUSED: &str = "\u{1F615}";
const SWEAT_SMILE: &str = "\u{1F605}";
const THINKING: &str = "\u{1F914}";
const SHRUG: &str = "\u{1F937}";
const WEARY: &str = "\u{1F629}";

prompt_test!(emoji_input, |t, d| {
    t.write("$ ");
    t.fire_command_start();
    t.assert_prompt_input("|").await;

    t.write(&format!("{PEACE}{THUMBS_UP}"));
    t.assert_prompt_input(&format!("{PEACE}{THUMBS_UP}|")).await;

    t.write(&format!("\r\n{SUNGLASSES}{CONFUSED}{SWEAT_SMILE}"));
    t.assert_prompt_input(&format!("{PEACE}{THUMBS_UP}\n{SUNGLASSES}{CONFUSED}{SWEAT_SMILE}|")).await;

    t.write(&format!("\r\n{THINKING}{SHRUG}{WEARY}"));
    t.assert_prompt_input(&format!("{PEACE}{THUMBS_UP}\n{SUNGLASSES}{CONFUSED}{SWEAT_SMILE}\n{THINKING}{SHRUG}{WEARY}|")).await;

    t.write("\x1b[G");
    t.assert_prompt_input(&format!("{PEACE}{THUMBS_UP}\n{SUNGLASSES}{CONFUSED}{SWEAT_SMILE}\n|{THINKING}{SHRUG}{WEARY}")).await;

    t.write("\x1b[A");
    t.assert_prompt_input(&format!("{PEACE}{THUMBS_UP}\n|{SUNGLASSES}{CONFUSED}{SWEAT_SMILE}\n{THINKING}{SHRUG}{WEARY}")).await;

    t.write("\x1b[2C");
    t.assert_prompt_input(&format!("{PEACE}{THUMBS_UP}\n{SUNGLASSES}{CONFUSED}|{SWEAT_SMILE}\n{THINKING}{SHRUG}{WEARY}")).await;

    t.write("\x1b[1;4H");
    t.assert_prompt_input(&format!("{PEACE}|{THUMBS_UP}\n{SUNGLASSES}{CONFUSED}{SWEAT_SMILE}\n{THINKING}{SHRUG}{WEARY}")).await;

    t.write("\x1b[D");
    t.assert_prompt_input(&format!("|{PEACE}{THUMBS_UP}\n{SUNGLASSES}{CONFUSED}{SWEAT_SMILE}\n{THINKING}{SHRUG}{WEARY}")).await;
});

mod trailing_whitespace {
    use super::*;

    prompt_test!(cursor_index_calculation_with_whitespace, |t, d| {
        t.write("$ ");
        t.fire_command_start();
        t.assert_prompt_input("|").await;

        t.write("echo   ");
        t.assert_prompt_input("echo   |").await;

        t.write("\x1b[3D");
        t.assert_prompt_input("echo|   ").await;

        t.write("\x1b[C");
        t.assert_prompt_input("echo |  ").await;

        t.write("\x1b[C");
        t.assert_prompt_input("echo  | ").await;

        t.write("\x1b[C");
        t.assert_prompt_input("echo   |").await;
    });

    prompt_test!(cursor_index_should_not_exceed_command_line_length, |t, d| {
        t.write("$ ");
        t.fire_command_start();
        t.assert_prompt_input("|").await;

        t.write("cmd");
        t.assert_prompt_input("cmd|").await;

        t.write("\x1b[10C");
        t.assert_prompt_input("cmd|").await;
    });

    prompt_test!(whitespace_preservation_in_cursor_calculation, |t, d| {
        t.write("$ ");
        t.fire_command_start();
        t.assert_prompt_input("|").await;

        t.write("ls   -la");
        t.assert_prompt_input("ls   -la|").await;

        t.write("\x1b[3D");
        t.assert_prompt_input("ls   |-la").await;

        t.write("\x1b[3D");
        t.assert_prompt_input("ls|   -la").await;

        t.write("\x1b[2C");
        t.assert_prompt_input("ls  | -la").await;
    });

    prompt_test!(delete_whitespace_with_backspace, |t, d| {
        t.write("$ ");
        t.fire_command_start();
        t.assert_prompt_input("|").await;

        t.write(" ");
        t.assert_prompt_input(" |").await;

        t.xterm.input("\x7F"); // Backspace
        t.write("\x1b[D");
        t.assert_prompt_input("|").await;

        t.xterm.input(&" ".repeat(4));
        t.write(&" ".repeat(4));
        t.assert_prompt_input("    |").await;

        t.xterm.input(&"\x1b[D".repeat(2)); // Left
        t.write("\x1b[2D");
        t.assert_prompt_input("  |  ").await;

        t.xterm.input("\x7F"); // Backspace
        t.write("\x1b[D");
        t.assert_prompt_input(" |  ").await;

        t.xterm.input("\x7F"); // Backspace
        t.write("\x1b[D");
        t.assert_prompt_input("|  ").await;

        t.xterm.input(" ");
        t.write(" ");
        t.assert_prompt_input(" |  ").await;

        t.xterm.input(" ");
        t.write(" ");
        t.assert_prompt_input("  |  ").await;

        t.xterm.input("\x1b[C"); // Right
        t.write("\x1b[C");
        t.assert_prompt_input("   | ").await;

        t.xterm.input("a");
        t.write("a");
        t.assert_prompt_input("   a| ").await;

        t.xterm.input("\x7F"); // Backspace
        t.write("\x1b[D\x1b[K");
        t.assert_prompt_input("   | ").await;

        t.xterm.input(&"\x1b[D".repeat(2)); // Left
        t.write("\x1b[2D");
        t.assert_prompt_input(" |   ").await;

        t.xterm.input("\x1b[3~"); // Delete
        t.write("");
        t.assert_prompt_input(" |  ").await;
    });

    // Isto não funciona direito, mas pouco importa: só acontece quando há muito espaço em branco no fim da entrada
    // do prompt.
    prompt_test!(
        #[ignore = "o original usa test.skip: não funciona direito, mas só acontece com muito espaço em branco no fim da entrada"]
        track_whitespace_when_conpty_deletes_whitespace_unexpectedly,
        |t, d| {
            t.write("$ ");
            t.fire_command_start();
            t.assert_prompt_input("|").await;

            t.xterm.input("ls");
            t.write("ls");
            t.assert_prompt_input("ls|").await;

            t.xterm.input(&" ".repeat(4));
            t.write(&" ".repeat(4));
            t.assert_prompt_input("ls    |").await;

            t.xterm.input(" ");
            t.write("\x1b[4D\x1b[5X\x1b[5C"); // Cursor left x(N-1), delete xN, cursor right xN
            t.assert_prompt_input("ls     |").await;
        }
    );

    prompt_test!(track_whitespace_beyond_cursor, |t, d| {
        t.write("$ ");
        t.fire_command_start();
        t.assert_prompt_input("|").await;

        t.write(&" ".repeat(8));
        t.assert_prompt_input(&format!("{}|", " ".repeat(8))).await;

        t.write("\x1b[4D");
        t.assert_prompt_input(&format!("{}|{}", " ".repeat(4), " ".repeat(4))).await;
    });
}

mod multi_line {
    use super::*;

    prompt_test!(basic_2_line, |t, d| {
        t.write("$ ");
        t.fire_command_start();
        t.assert_prompt_input("|").await;

        t.write("echo \"a");
        t.assert_prompt_input("echo \"a|").await;

        t.write("\n\r∙ ");
        t.set_continuation_prompt("∙ ");
        t.assert_prompt_input("echo \"a\n|").await;

        t.write("b");
        t.assert_prompt_input("echo \"a\nb|").await;
    });

    prompt_test!(basic_3_line, |t, d| {
        t.write("$ ");
        t.fire_command_start();
        t.assert_prompt_input("|").await;

        t.write("echo \"a");
        t.assert_prompt_input("echo \"a|").await;

        t.write("\n\r∙ ");
        t.set_continuation_prompt("∙ ");
        t.assert_prompt_input("echo \"a\n|").await;

        t.write("b");
        t.assert_prompt_input("echo \"a\nb|").await;

        t.write("\n\r∙ ");
        t.set_continuation_prompt("∙ ");
        t.assert_prompt_input("echo \"a\nb\n|").await;

        t.write("c");
        t.assert_prompt_input("echo \"a\nb\nc|").await;
    });

    prompt_test!(navigate_left_in_multi_line, |t, d| {
        t.write("$ ");
        t.fire_command_start();
        t.assert_prompt_input("|").await;

        t.write("echo \"a");
        t.assert_prompt_input("echo \"a|").await;

        t.write("\n\r∙ ");
        t.set_continuation_prompt("∙ ");
        t.assert_prompt_input("echo \"a\n|").await;

        t.write("b");
        t.assert_prompt_input("echo \"a\nb|").await;

        t.write("\x1b[D");
        t.assert_prompt_input("echo \"a\n|b").await;

        t.write("\x1b[@c");
        t.assert_prompt_input("echo \"a\nc|b").await;

        t.write("\x1b[K\n\r∙ ");
        t.assert_prompt_input("echo \"a\nc\n|").await;

        t.write("b");
        t.assert_prompt_input("echo \"a\nc\nb|").await;

        t.write(" foo");
        t.assert_prompt_input("echo \"a\nc\nb foo|").await;

        t.write("\x1b[3D");
        t.assert_prompt_input("echo \"a\nc\nb |foo").await;
    });

    prompt_test!(navigate_up_in_multi_line, |t, d| {
        t.write("$ ");
        t.fire_command_start();
        t.assert_prompt_input("|").await;

        t.write("echo \"foo");
        t.assert_prompt_input("echo \"foo|").await;

        t.write("\n\r∙ ");
        t.set_continuation_prompt("∙ ");
        t.assert_prompt_input("echo \"foo\n|").await;

        t.write("bar");
        t.assert_prompt_input("echo \"foo\nbar|").await;

        t.write("\n\r∙ ");
        t.set_continuation_prompt("∙ ");
        t.assert_prompt_input("echo \"foo\nbar\n|").await;

        t.write("baz");
        t.assert_prompt_input("echo \"foo\nbar\nbaz|").await;

        t.write("\x1b[A");
        t.assert_prompt_input("echo \"foo\nbar|\nbaz").await;

        t.write("\x1b[D");
        t.assert_prompt_input("echo \"foo\nba|r\nbaz").await;

        t.write("\x1b[D");
        t.assert_prompt_input("echo \"foo\nb|ar\nbaz").await;

        t.write("\x1b[D");
        t.assert_prompt_input("echo \"foo\n|bar\nbaz").await;

        t.write("\x1b[1;9H");
        t.assert_prompt_input("echo \"|foo\nbar\nbaz").await;

        t.write("\x1b[C");
        t.assert_prompt_input("echo \"f|oo\nbar\nbaz").await;

        t.write("\x1b[C");
        t.assert_prompt_input("echo \"fo|o\nbar\nbaz").await;

        t.write("\x1b[C");
        t.assert_prompt_input("echo \"foo|\nbar\nbaz").await;
    });

    prompt_test!(navigating_up_when_first_line_contains_invalid_stale_trailing_whitespace, |t, d| {
        t.write("$ ");
        t.fire_command_start();
        t.assert_prompt_input("|").await;

        t.write("echo \"foo      \x1b[6D");
        t.assert_prompt_input("echo \"foo|").await;

        t.write("\n\r∙ ");
        t.set_continuation_prompt("∙ ");
        t.assert_prompt_input("echo \"foo\n|").await;

        t.write("bar");
        t.assert_prompt_input("echo \"foo\nbar|").await;

        t.write("\x1b[D");
        t.assert_prompt_input("echo \"foo\nba|r").await;

        t.write("\x1b[D");
        t.assert_prompt_input("echo \"foo\nb|ar").await;

        t.write("\x1b[D");
        t.assert_prompt_input("echo \"foo\n|bar").await;
    });
}

mod multi_line_wrapped_no_continuation_prompt {
    use super::*;

    prompt_test!(basic_wrapped_line, |t, d| {
        t.xterm.resize(5, 10);

        t.write("$ ");
        t.fire_command_start();
        t.assert_prompt_input("|").await;

        t.write("ech");
        t.assert_prompt_input("ech|").await;

        t.write("o ");
        t.assert_prompt_input("echo |").await;

        t.write("\"a\"");
        // HACK: Trailing whitespace is due to flaky detection in wrapped lines (but it doesn't matter much)
        t.assert_prompt_input("echo \"a\"| ").await;
        t.write("\n\r b");
        t.assert_prompt_input("echo \"a\"\n b|").await;
        t.write("\n\r c");
        t.assert_prompt_input("echo \"a\"\n b\n c|").await;
    });
}

mod multi_line_wrapped_continuation_prompt {
    use super::*;

    prompt_test!(basic_wrapped_line, |t, d| {
        t.xterm.resize(5, 10);
        t.model.set_continuation_prompt("∙ ");
        t.write("$ ");
        t.fire_command_start();
        t.assert_prompt_input("|").await;

        t.write("ech");
        t.assert_prompt_input("ech|").await;

        t.write("o ");
        t.assert_prompt_input("echo |").await;

        t.write("\"a\"");
        // HACK: Trailing whitespace is due to flaky detection in wrapped lines (but it doesn't matter much)
        t.assert_prompt_input("echo \"a\"| ").await;
        t.write("\n\r∙ ");
        t.assert_prompt_input("echo \"a\"\n|").await;
        t.write("b");
        t.assert_prompt_input("echo \"a\"\nb|").await;
        t.write("\n\r∙ ");
        t.assert_prompt_input("echo \"a\"\nb\n|").await;
        t.write("c");
        t.assert_prompt_input("echo \"a\"\nb\nc|").await;
        t.write("\n\r∙ ");
        t.assert_prompt_input("echo \"a\"\nb\nc\n|").await;
    });
}

mod multi_line_wrapped_fish {
    use super::*;

    prompt_test!(forward_slash_continuation, |t, d| {
        t.model.set_shell_type(Some(ShellType::Posix(PosixShellType::Fish)));
        t.write("$ ");
        t.assert_prompt_input("|").await;
        t.write("[I] meganrogge@Megans-MacBook-Pro ~ (main|BISECTING)>");
        t.fire_command_start();

        t.write("ech\\");
        t.assert_prompt_input("ech\\|").await;
        t.write("\no bye");
        t.assert_prompt_input("echo bye|").await;
    });

    prompt_test!(newline_with_no_continuation, |t, d| {
        t.model.set_shell_type(Some(ShellType::Posix(PosixShellType::Fish)));
        t.write("$ ");
        t.assert_prompt_input("|").await;
        t.write("[I] meganrogge@Megans-MacBook-Pro ~ (main|BISECTING)>");
        t.fire_command_start();
        t.assert_prompt_input("|").await;

        t.write("echo \"hi");
        t.assert_prompt_input("echo \"hi|").await;
        t.write("\nand bye\nwhy\"");
        t.assert_prompt_input("echo \"hi\nand bye\nwhy\"|").await;
    });
}

// To "record a session" for these tests:
// - Enable debug logging
// - Open and clear Terminal output channel
// - Open terminal and perform the test
// - Extract all "parsing data" lines from the terminal
mod recorded_sessions {
    use super::*;

    fn replay_events<S: AsRef<str>>(t: &Fixture, events: &[S]) {
        for data in events {
            t.write(data.as_ref());
        }
    }

    /// Os eventos que o pwsh 7.4.2 e o starship 1.10.2 escrevem ao abrir o terminal: limpar a tela, esvaziar as linhas
    /// (`line_feeds` quebras, que rolam a tela), os valores da integração de shell e o prompt de duas linhas.
    fn pwsh_startup(title: &str, line_feeds: usize, prompt: &str) -> Vec<String> {
        vec![
            format!("\x1b[?25l\x1b[2J\x1b[m\x1b[H\x1b]0;{title}\x07\x1b[?25h"),
            format!("\x1b[?25l{}\x1b[K\x1b[H\x1b[?25h", "\x1b[K\r\n".repeat(line_feeds)),
            "\x1b]633;P;IsWindows=True\x07".to_string(),
            "\x1b]633;P;ContinuationPrompt=\x1b[38;5;8m∙\x1b[0m \x07".to_string(),
            "\x1b]633;A\x07\x1b]633;P;Cwd=C:\\Github\\microsoft\\vscode\x07\x1b]633;B\x07".to_string(),
            prompt.to_string(),
        ]
    }

    /// O prompt do starship: a linha de status (hora, pasta, ramo, estado do git, versão do node) e o `❯`.
    fn starship_prompt(time: &str, branch: &str, git_status: &str) -> String {
        format!(
            "\x1b[34m\r\n\x1b[38;2;17;17;17m\x1b[44m{time} \x1b[34m\x1b[41m\u{e0b0} \x1b[38;2;17;17;17mvscode \x1b[31m\x1b[43m\u{e0b0} \x1b[38;2;17;17;17m\u{e0a0} {branch} \x1b[33m\x1b[46m\u{e0b0} \x1b[38;2;17;17;17m{git_status} \x1b[36m\x1b[49m\u{e0b0} \x1b[mvia \x1b[32m\x1b[1m\u{e718} v18.18.2 \r\n❯\x1b[m "
        )
    }

    const WINDOWS_APPS_TITLE: &str = "C:\\Program Files\\WindowsApps\\Microsoft.PowerShell_7.4.2.0_x64__8wekyb3d8bbwe\\pwsh.exe";

    mod windows_11_pwsh_7_4_2_starship_1_10_2 {
        use super::*;

        prompt_test!(input_with_ignored_ghost_text, |t, d| {
            replay_events(&t, &pwsh_startup(WINDOWS_APPS_TITLE, 25, &starship_prompt("03:13:47", "tyriar/prompt_input_model", "$⇡")));
            t.fire_command_start();
            t.assert_prompt_input("|").await;

            replay_events(
                &t,
                &[
                    "\x1b[?25l\x1b[93mf\x1b[97m\x1b[2m\x1b[3makecommand\x1b[3;4H\x1b[?25h",
                    "\x1b[m",
                    "\x1b[93mfo\x1b[9X",
                    "\x1b[m",
                    "\x1b[?25l\x1b[93m\x1b[3;3Hfoo\x1b[?25h",
                    "\x1b[m",
                ],
            );
            t.assert_prompt_input("foo|").await;
        });

        prompt_test!(input_with_accepted_and_run_ghost_text, |t, d| {
            replay_events(&t, &pwsh_startup(WINDOWS_APPS_TITLE, 25, &starship_prompt("03:41:36", "tyriar/prompt_input_model", "$")));
            t.model.set_continuation_prompt("∙ ");
            t.fire_command_start();
            t.assert_prompt_input("|").await;

            replay_events(&t, &["\x1b[?25l\x1b[93me\x1b[97m\x1b[2m\x1b[3mcho \"hello world\"\x1b[3;4H\x1b[?25h", "\x1b[m"]);
            t.assert_prompt_input("e|[cho \"hello world\"]").await;

            replay_events(&t, &["\x1b[?25l\x1b[93mec\x1b[97m\x1b[2m\x1b[3mho \"hello world\"\x1b[3;5H\x1b[?25h", "\x1b[m"]);
            t.assert_prompt_input("ec|[ho \"hello world\"]").await;

            replay_events(&t, &["\x1b[?25l\x1b[93m\x1b[3;3Hech\x1b[97m\x1b[2m\x1b[3mo \"hello world\"\x1b[3;6H\x1b[?25h", "\x1b[m"]);
            t.assert_prompt_input("ech|[o \"hello world\"]").await;

            replay_events(&t, &["\x1b[?25l\x1b[93m\x1b[3;3Hecho\x1b[97m\x1b[2m\x1b[3m \"hello world\"\x1b[3;7H\x1b[?25h", "\x1b[m"]);
            t.assert_prompt_input("echo|[ \"hello world\"]").await;

            replay_events(&t, &["\x1b[?25l\x1b[93m\x1b[3;3Hecho \x1b[97m\x1b[2m\x1b[3m\"hello world\"\x1b[3;8H\x1b[?25h", "\x1b[m"]);
            t.assert_prompt_input("echo |[\"hello world\"]").await;

            replay_events(&t, &["\x1b[?25l\x1b[93m\x1b[3;3Hecho \x1b[36m\"hello world\"\x1b[?25h", "\x1b[m"]);
            t.assert_prompt_input("echo \"hello world\"|").await;

            replay_events(&t, &["\x1b]633;E;echo \"hello world\";ff464d39-bc80-4bae-9ead-b1cafc4adf6f\x07\x1b]633;C\x07"]);
            t.fire_command_executed();
            t.assert_prompt_input("echo \"hello world\"").await;

            replay_events(&t, &["\r\n", "hello world\r\n"]);
            t.assert_prompt_input("echo \"hello world\"").await;

            replay_events(
                &t,
                &[
                    "\x1b]633;D;0\x07\x1b]633;A\x07\x1b]633;P;Cwd=C:\\Github\\microsoft\\vscode\x07\x1b]633;B\x07".to_string(),
                    starship_prompt("03:41:42", "tyriar/prompt_input_model", "$"),
                ],
            );
            t.fire_command_start();
            t.assert_prompt_input("|").await;
        });

        prompt_test!(input_go_to_start_ctrl_home_delete_word_in_front_ctrl_delete, |t, d| {
            // O título do original usa `\P`, `\W`, `\M` e `\p` num literal de aspas simples, que o JavaScript lê como a letra sozinha.
            replay_events(
                &t,
                &pwsh_startup(
                    "C:Program FilesWindowsAppsMicrosoft.PowerShell_7.4.2.0_x64__8wekyb3d8bbwepwsh.exe",
                    28,
                    &starship_prompt("16:07:06", "tyriar/210662", "$!"),
                ),
            );
            t.fire_command_start();
            t.assert_prompt_input("|").await;

            replay_events(
                &t,
                &[
                    "\x1b[?25l\x1b[93mG\x1b[97m\x1b[2m\x1b[3mit push\x1b[3;4H\x1b[?25h",
                    "\x1b[m",
                    "\x1b[?25l\x1b[93mGe\x1b[97m\x1b[2m\x1b[3mt-ChildItem -Path a\x1b[3;5H\x1b[?25h",
                    "\x1b[m",
                    "\x1b[?25l\x1b[93m\x1b[3;3HGet\x1b[97m\x1b[2m\x1b[3m-ChildItem -Path a\x1b[3;6H\x1b[?25h",
                ],
            );
            t.assert_prompt_input("Get|[-ChildItem -Path a]").await;

            replay_events(&t, &["\x1b[m", "\x1b[?25l\x1b[3;3H\x1b[?25h", "\x1b[21X"]);

            // Don't force a sync, the prompt input model should update by itself
            timeout(0).await.unwrap();
            let actual_value_with_cursor = t.model.get_combined_string(None).to_string_lossy();
            assert_eq!(actual_value_with_cursor, "|".replace('\n', "\u{23CE}"));
        });
    }
}
