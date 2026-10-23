// Arquivo gerado por `scripts/gen_key_codes.py` a partir de `src/vs/base/common/keyCodes.ts`
// (VS Code 1.143.1). Não editar: rode `python3 -I scripts/gen_key_codes.py`.

use crate::key_codes::{KeyCode, KeyCodeMapping, ScanCode};

#[allow(non_upper_case_globals)]
impl KeyCode {
    pub const DependsOnKbLayout: KeyCode = KeyCode(-1);
    pub const Unknown: KeyCode = KeyCode(0);
    pub const Backspace: KeyCode = KeyCode(1);
    pub const Tab: KeyCode = KeyCode(2);
    pub const Enter: KeyCode = KeyCode(3);
    pub const Shift: KeyCode = KeyCode(4);
    pub const Ctrl: KeyCode = KeyCode(5);
    pub const Alt: KeyCode = KeyCode(6);
    pub const PauseBreak: KeyCode = KeyCode(7);
    pub const CapsLock: KeyCode = KeyCode(8);
    pub const Escape: KeyCode = KeyCode(9);
    pub const Space: KeyCode = KeyCode(10);
    pub const PageUp: KeyCode = KeyCode(11);
    pub const PageDown: KeyCode = KeyCode(12);
    pub const End: KeyCode = KeyCode(13);
    pub const Home: KeyCode = KeyCode(14);
    pub const LeftArrow: KeyCode = KeyCode(15);
    pub const UpArrow: KeyCode = KeyCode(16);
    pub const RightArrow: KeyCode = KeyCode(17);
    pub const DownArrow: KeyCode = KeyCode(18);
    pub const Insert: KeyCode = KeyCode(19);
    pub const Delete: KeyCode = KeyCode(20);
    pub const Digit0: KeyCode = KeyCode(21);
    pub const Digit1: KeyCode = KeyCode(22);
    pub const Digit2: KeyCode = KeyCode(23);
    pub const Digit3: KeyCode = KeyCode(24);
    pub const Digit4: KeyCode = KeyCode(25);
    pub const Digit5: KeyCode = KeyCode(26);
    pub const Digit6: KeyCode = KeyCode(27);
    pub const Digit7: KeyCode = KeyCode(28);
    pub const Digit8: KeyCode = KeyCode(29);
    pub const Digit9: KeyCode = KeyCode(30);
    pub const KeyA: KeyCode = KeyCode(31);
    pub const KeyB: KeyCode = KeyCode(32);
    pub const KeyC: KeyCode = KeyCode(33);
    pub const KeyD: KeyCode = KeyCode(34);
    pub const KeyE: KeyCode = KeyCode(35);
    pub const KeyF: KeyCode = KeyCode(36);
    pub const KeyG: KeyCode = KeyCode(37);
    pub const KeyH: KeyCode = KeyCode(38);
    pub const KeyI: KeyCode = KeyCode(39);
    pub const KeyJ: KeyCode = KeyCode(40);
    pub const KeyK: KeyCode = KeyCode(41);
    pub const KeyL: KeyCode = KeyCode(42);
    pub const KeyM: KeyCode = KeyCode(43);
    pub const KeyN: KeyCode = KeyCode(44);
    pub const KeyO: KeyCode = KeyCode(45);
    pub const KeyP: KeyCode = KeyCode(46);
    pub const KeyQ: KeyCode = KeyCode(47);
    pub const KeyR: KeyCode = KeyCode(48);
    pub const KeyS: KeyCode = KeyCode(49);
    pub const KeyT: KeyCode = KeyCode(50);
    pub const KeyU: KeyCode = KeyCode(51);
    pub const KeyV: KeyCode = KeyCode(52);
    pub const KeyW: KeyCode = KeyCode(53);
    pub const KeyX: KeyCode = KeyCode(54);
    pub const KeyY: KeyCode = KeyCode(55);
    pub const KeyZ: KeyCode = KeyCode(56);
    pub const Meta: KeyCode = KeyCode(57);
    pub const ContextMenu: KeyCode = KeyCode(58);
    pub const F1: KeyCode = KeyCode(59);
    pub const F2: KeyCode = KeyCode(60);
    pub const F3: KeyCode = KeyCode(61);
    pub const F4: KeyCode = KeyCode(62);
    pub const F5: KeyCode = KeyCode(63);
    pub const F6: KeyCode = KeyCode(64);
    pub const F7: KeyCode = KeyCode(65);
    pub const F8: KeyCode = KeyCode(66);
    pub const F9: KeyCode = KeyCode(67);
    pub const F10: KeyCode = KeyCode(68);
    pub const F11: KeyCode = KeyCode(69);
    pub const F12: KeyCode = KeyCode(70);
    pub const F13: KeyCode = KeyCode(71);
    pub const F14: KeyCode = KeyCode(72);
    pub const F15: KeyCode = KeyCode(73);
    pub const F16: KeyCode = KeyCode(74);
    pub const F17: KeyCode = KeyCode(75);
    pub const F18: KeyCode = KeyCode(76);
    pub const F19: KeyCode = KeyCode(77);
    pub const F20: KeyCode = KeyCode(78);
    pub const F21: KeyCode = KeyCode(79);
    pub const F22: KeyCode = KeyCode(80);
    pub const F23: KeyCode = KeyCode(81);
    pub const F24: KeyCode = KeyCode(82);
    pub const NumLock: KeyCode = KeyCode(83);
    pub const ScrollLock: KeyCode = KeyCode(84);
    pub const Semicolon: KeyCode = KeyCode(85);
    pub const Equal: KeyCode = KeyCode(86);
    pub const Comma: KeyCode = KeyCode(87);
    pub const Minus: KeyCode = KeyCode(88);
    pub const Period: KeyCode = KeyCode(89);
    pub const Slash: KeyCode = KeyCode(90);
    pub const Backquote: KeyCode = KeyCode(91);
    pub const BracketLeft: KeyCode = KeyCode(92);
    pub const Backslash: KeyCode = KeyCode(93);
    pub const BracketRight: KeyCode = KeyCode(94);
    pub const Quote: KeyCode = KeyCode(95);
    pub const Oem8: KeyCode = KeyCode(96);
    pub const IntlBackslash: KeyCode = KeyCode(97);
    pub const Numpad0: KeyCode = KeyCode(98);
    pub const Numpad1: KeyCode = KeyCode(99);
    pub const Numpad2: KeyCode = KeyCode(100);
    pub const Numpad3: KeyCode = KeyCode(101);
    pub const Numpad4: KeyCode = KeyCode(102);
    pub const Numpad5: KeyCode = KeyCode(103);
    pub const Numpad6: KeyCode = KeyCode(104);
    pub const Numpad7: KeyCode = KeyCode(105);
    pub const Numpad8: KeyCode = KeyCode(106);
    pub const Numpad9: KeyCode = KeyCode(107);
    pub const NumpadMultiply: KeyCode = KeyCode(108);
    pub const NumpadAdd: KeyCode = KeyCode(109);
    pub const NumpadSeparator: KeyCode = KeyCode(110);
    pub const NumpadSubtract: KeyCode = KeyCode(111);
    pub const NumpadDecimal: KeyCode = KeyCode(112);
    pub const NumpadDivide: KeyCode = KeyCode(113);
    pub const KeyInComposition: KeyCode = KeyCode(114);
    pub const AbntC1: KeyCode = KeyCode(115);
    pub const AbntC2: KeyCode = KeyCode(116);
    pub const AudioVolumeMute: KeyCode = KeyCode(117);
    pub const AudioVolumeUp: KeyCode = KeyCode(118);
    pub const AudioVolumeDown: KeyCode = KeyCode(119);
    pub const BrowserSearch: KeyCode = KeyCode(120);
    pub const BrowserHome: KeyCode = KeyCode(121);
    pub const BrowserBack: KeyCode = KeyCode(122);
    pub const BrowserForward: KeyCode = KeyCode(123);
    pub const MediaTrackNext: KeyCode = KeyCode(124);
    pub const MediaTrackPrevious: KeyCode = KeyCode(125);
    pub const MediaStop: KeyCode = KeyCode(126);
    pub const MediaPlayPause: KeyCode = KeyCode(127);
    pub const LaunchMediaPlayer: KeyCode = KeyCode(128);
    pub const LaunchMail: KeyCode = KeyCode(129);
    pub const LaunchApp2: KeyCode = KeyCode(130);
    pub const Clear: KeyCode = KeyCode(131);
    pub const MaxValue: KeyCode = KeyCode(132);
}

#[allow(non_upper_case_globals)]
impl ScanCode {
    pub const DependsOnKbLayout: ScanCode = ScanCode(-1);
    pub const None: ScanCode = ScanCode(0);
    pub const Hyper: ScanCode = ScanCode(1);
    pub const Super: ScanCode = ScanCode(2);
    pub const Fn: ScanCode = ScanCode(3);
    pub const FnLock: ScanCode = ScanCode(4);
    pub const Suspend: ScanCode = ScanCode(5);
    pub const Resume: ScanCode = ScanCode(6);
    pub const Turbo: ScanCode = ScanCode(7);
    pub const Sleep: ScanCode = ScanCode(8);
    pub const WakeUp: ScanCode = ScanCode(9);
    pub const KeyA: ScanCode = ScanCode(10);
    pub const KeyB: ScanCode = ScanCode(11);
    pub const KeyC: ScanCode = ScanCode(12);
    pub const KeyD: ScanCode = ScanCode(13);
    pub const KeyE: ScanCode = ScanCode(14);
    pub const KeyF: ScanCode = ScanCode(15);
    pub const KeyG: ScanCode = ScanCode(16);
    pub const KeyH: ScanCode = ScanCode(17);
    pub const KeyI: ScanCode = ScanCode(18);
    pub const KeyJ: ScanCode = ScanCode(19);
    pub const KeyK: ScanCode = ScanCode(20);
    pub const KeyL: ScanCode = ScanCode(21);
    pub const KeyM: ScanCode = ScanCode(22);
    pub const KeyN: ScanCode = ScanCode(23);
    pub const KeyO: ScanCode = ScanCode(24);
    pub const KeyP: ScanCode = ScanCode(25);
    pub const KeyQ: ScanCode = ScanCode(26);
    pub const KeyR: ScanCode = ScanCode(27);
    pub const KeyS: ScanCode = ScanCode(28);
    pub const KeyT: ScanCode = ScanCode(29);
    pub const KeyU: ScanCode = ScanCode(30);
    pub const KeyV: ScanCode = ScanCode(31);
    pub const KeyW: ScanCode = ScanCode(32);
    pub const KeyX: ScanCode = ScanCode(33);
    pub const KeyY: ScanCode = ScanCode(34);
    pub const KeyZ: ScanCode = ScanCode(35);
    pub const Digit1: ScanCode = ScanCode(36);
    pub const Digit2: ScanCode = ScanCode(37);
    pub const Digit3: ScanCode = ScanCode(38);
    pub const Digit4: ScanCode = ScanCode(39);
    pub const Digit5: ScanCode = ScanCode(40);
    pub const Digit6: ScanCode = ScanCode(41);
    pub const Digit7: ScanCode = ScanCode(42);
    pub const Digit8: ScanCode = ScanCode(43);
    pub const Digit9: ScanCode = ScanCode(44);
    pub const Digit0: ScanCode = ScanCode(45);
    pub const Enter: ScanCode = ScanCode(46);
    pub const Escape: ScanCode = ScanCode(47);
    pub const Backspace: ScanCode = ScanCode(48);
    pub const Tab: ScanCode = ScanCode(49);
    pub const Space: ScanCode = ScanCode(50);
    pub const Minus: ScanCode = ScanCode(51);
    pub const Equal: ScanCode = ScanCode(52);
    pub const BracketLeft: ScanCode = ScanCode(53);
    pub const BracketRight: ScanCode = ScanCode(54);
    pub const Backslash: ScanCode = ScanCode(55);
    pub const IntlHash: ScanCode = ScanCode(56);
    pub const Semicolon: ScanCode = ScanCode(57);
    pub const Quote: ScanCode = ScanCode(58);
    pub const Backquote: ScanCode = ScanCode(59);
    pub const Comma: ScanCode = ScanCode(60);
    pub const Period: ScanCode = ScanCode(61);
    pub const Slash: ScanCode = ScanCode(62);
    pub const CapsLock: ScanCode = ScanCode(63);
    pub const F1: ScanCode = ScanCode(64);
    pub const F2: ScanCode = ScanCode(65);
    pub const F3: ScanCode = ScanCode(66);
    pub const F4: ScanCode = ScanCode(67);
    pub const F5: ScanCode = ScanCode(68);
    pub const F6: ScanCode = ScanCode(69);
    pub const F7: ScanCode = ScanCode(70);
    pub const F8: ScanCode = ScanCode(71);
    pub const F9: ScanCode = ScanCode(72);
    pub const F10: ScanCode = ScanCode(73);
    pub const F11: ScanCode = ScanCode(74);
    pub const F12: ScanCode = ScanCode(75);
    pub const PrintScreen: ScanCode = ScanCode(76);
    pub const ScrollLock: ScanCode = ScanCode(77);
    pub const Pause: ScanCode = ScanCode(78);
    pub const Insert: ScanCode = ScanCode(79);
    pub const Home: ScanCode = ScanCode(80);
    pub const PageUp: ScanCode = ScanCode(81);
    pub const Delete: ScanCode = ScanCode(82);
    pub const End: ScanCode = ScanCode(83);
    pub const PageDown: ScanCode = ScanCode(84);
    pub const ArrowRight: ScanCode = ScanCode(85);
    pub const ArrowLeft: ScanCode = ScanCode(86);
    pub const ArrowDown: ScanCode = ScanCode(87);
    pub const ArrowUp: ScanCode = ScanCode(88);
    pub const NumLock: ScanCode = ScanCode(89);
    pub const NumpadDivide: ScanCode = ScanCode(90);
    pub const NumpadMultiply: ScanCode = ScanCode(91);
    pub const NumpadSubtract: ScanCode = ScanCode(92);
    pub const NumpadAdd: ScanCode = ScanCode(93);
    pub const NumpadEnter: ScanCode = ScanCode(94);
    pub const Numpad1: ScanCode = ScanCode(95);
    pub const Numpad2: ScanCode = ScanCode(96);
    pub const Numpad3: ScanCode = ScanCode(97);
    pub const Numpad4: ScanCode = ScanCode(98);
    pub const Numpad5: ScanCode = ScanCode(99);
    pub const Numpad6: ScanCode = ScanCode(100);
    pub const Numpad7: ScanCode = ScanCode(101);
    pub const Numpad8: ScanCode = ScanCode(102);
    pub const Numpad9: ScanCode = ScanCode(103);
    pub const Numpad0: ScanCode = ScanCode(104);
    pub const NumpadDecimal: ScanCode = ScanCode(105);
    pub const IntlBackslash: ScanCode = ScanCode(106);
    pub const ContextMenu: ScanCode = ScanCode(107);
    pub const Power: ScanCode = ScanCode(108);
    pub const NumpadEqual: ScanCode = ScanCode(109);
    pub const F13: ScanCode = ScanCode(110);
    pub const F14: ScanCode = ScanCode(111);
    pub const F15: ScanCode = ScanCode(112);
    pub const F16: ScanCode = ScanCode(113);
    pub const F17: ScanCode = ScanCode(114);
    pub const F18: ScanCode = ScanCode(115);
    pub const F19: ScanCode = ScanCode(116);
    pub const F20: ScanCode = ScanCode(117);
    pub const F21: ScanCode = ScanCode(118);
    pub const F22: ScanCode = ScanCode(119);
    pub const F23: ScanCode = ScanCode(120);
    pub const F24: ScanCode = ScanCode(121);
    pub const Open: ScanCode = ScanCode(122);
    pub const Help: ScanCode = ScanCode(123);
    pub const Select: ScanCode = ScanCode(124);
    pub const Again: ScanCode = ScanCode(125);
    pub const Undo: ScanCode = ScanCode(126);
    pub const Cut: ScanCode = ScanCode(127);
    pub const Copy: ScanCode = ScanCode(128);
    pub const Paste: ScanCode = ScanCode(129);
    pub const Find: ScanCode = ScanCode(130);
    pub const AudioVolumeMute: ScanCode = ScanCode(131);
    pub const AudioVolumeUp: ScanCode = ScanCode(132);
    pub const AudioVolumeDown: ScanCode = ScanCode(133);
    pub const NumpadComma: ScanCode = ScanCode(134);
    pub const IntlRo: ScanCode = ScanCode(135);
    pub const KanaMode: ScanCode = ScanCode(136);
    pub const IntlYen: ScanCode = ScanCode(137);
    pub const Convert: ScanCode = ScanCode(138);
    pub const NonConvert: ScanCode = ScanCode(139);
    pub const Lang1: ScanCode = ScanCode(140);
    pub const Lang2: ScanCode = ScanCode(141);
    pub const Lang3: ScanCode = ScanCode(142);
    pub const Lang4: ScanCode = ScanCode(143);
    pub const Lang5: ScanCode = ScanCode(144);
    pub const Abort: ScanCode = ScanCode(145);
    pub const Props: ScanCode = ScanCode(146);
    pub const NumpadParenLeft: ScanCode = ScanCode(147);
    pub const NumpadParenRight: ScanCode = ScanCode(148);
    pub const NumpadBackspace: ScanCode = ScanCode(149);
    pub const NumpadMemoryStore: ScanCode = ScanCode(150);
    pub const NumpadMemoryRecall: ScanCode = ScanCode(151);
    pub const NumpadMemoryClear: ScanCode = ScanCode(152);
    pub const NumpadMemoryAdd: ScanCode = ScanCode(153);
    pub const NumpadMemorySubtract: ScanCode = ScanCode(154);
    pub const NumpadClear: ScanCode = ScanCode(155);
    pub const NumpadClearEntry: ScanCode = ScanCode(156);
    pub const ControlLeft: ScanCode = ScanCode(157);
    pub const ShiftLeft: ScanCode = ScanCode(158);
    pub const AltLeft: ScanCode = ScanCode(159);
    pub const MetaLeft: ScanCode = ScanCode(160);
    pub const ControlRight: ScanCode = ScanCode(161);
    pub const ShiftRight: ScanCode = ScanCode(162);
    pub const AltRight: ScanCode = ScanCode(163);
    pub const MetaRight: ScanCode = ScanCode(164);
    pub const BrightnessUp: ScanCode = ScanCode(165);
    pub const BrightnessDown: ScanCode = ScanCode(166);
    pub const MediaPlay: ScanCode = ScanCode(167);
    pub const MediaRecord: ScanCode = ScanCode(168);
    pub const MediaFastForward: ScanCode = ScanCode(169);
    pub const MediaRewind: ScanCode = ScanCode(170);
    pub const MediaTrackNext: ScanCode = ScanCode(171);
    pub const MediaTrackPrevious: ScanCode = ScanCode(172);
    pub const MediaStop: ScanCode = ScanCode(173);
    pub const Eject: ScanCode = ScanCode(174);
    pub const MediaPlayPause: ScanCode = ScanCode(175);
    pub const MediaSelect: ScanCode = ScanCode(176);
    pub const LaunchMail: ScanCode = ScanCode(177);
    pub const LaunchApp2: ScanCode = ScanCode(178);
    pub const LaunchApp1: ScanCode = ScanCode(179);
    pub const SelectTask: ScanCode = ScanCode(180);
    pub const LaunchScreenSaver: ScanCode = ScanCode(181);
    pub const BrowserSearch: ScanCode = ScanCode(182);
    pub const BrowserHome: ScanCode = ScanCode(183);
    pub const BrowserBack: ScanCode = ScanCode(184);
    pub const BrowserForward: ScanCode = ScanCode(185);
    pub const BrowserStop: ScanCode = ScanCode(186);
    pub const BrowserRefresh: ScanCode = ScanCode(187);
    pub const BrowserFavorites: ScanCode = ScanCode(188);
    pub const ZoomToggle: ScanCode = ScanCode(189);
    pub const MailReply: ScanCode = ScanCode(190);
    pub const MailForward: ScanCode = ScanCode(191);
    pub const MailSend: ScanCode = ScanCode(192);
    pub const MaxValue: ScanCode = ScanCode(193);
}

/// A tabela `mappings` do original, na ordem dela.
pub(crate) static MAPPINGS: [KeyCodeMapping; 229] = [
    KeyCodeMapping { immutable: true, scan_code: ScanCode::None, scan_code_str: "None", key_code: KeyCode::Unknown, key_code_str: "unknown", event_key_code: 0, vkey: "VK_UNKNOWN", us_user_settings_label: "", general_user_settings_label: "" },
    KeyCodeMapping { immutable: true, scan_code: ScanCode::Hyper, scan_code_str: "Hyper", key_code: KeyCode::Unknown, key_code_str: "", event_key_code: 0, vkey: "", us_user_settings_label: "", general_user_settings_label: "" },
    KeyCodeMapping { immutable: true, scan_code: ScanCode::Super, scan_code_str: "Super", key_code: KeyCode::Unknown, key_code_str: "", event_key_code: 0, vkey: "", us_user_settings_label: "", general_user_settings_label: "" },
    KeyCodeMapping { immutable: true, scan_code: ScanCode::Fn, scan_code_str: "Fn", key_code: KeyCode::Unknown, key_code_str: "", event_key_code: 0, vkey: "", us_user_settings_label: "", general_user_settings_label: "" },
    KeyCodeMapping { immutable: true, scan_code: ScanCode::FnLock, scan_code_str: "FnLock", key_code: KeyCode::Unknown, key_code_str: "", event_key_code: 0, vkey: "", us_user_settings_label: "", general_user_settings_label: "" },
    KeyCodeMapping { immutable: true, scan_code: ScanCode::Suspend, scan_code_str: "Suspend", key_code: KeyCode::Unknown, key_code_str: "", event_key_code: 0, vkey: "", us_user_settings_label: "", general_user_settings_label: "" },
    KeyCodeMapping { immutable: true, scan_code: ScanCode::Resume, scan_code_str: "Resume", key_code: KeyCode::Unknown, key_code_str: "", event_key_code: 0, vkey: "", us_user_settings_label: "", general_user_settings_label: "" },
    KeyCodeMapping { immutable: true, scan_code: ScanCode::Turbo, scan_code_str: "Turbo", key_code: KeyCode::Unknown, key_code_str: "", event_key_code: 0, vkey: "", us_user_settings_label: "", general_user_settings_label: "" },
    KeyCodeMapping { immutable: true, scan_code: ScanCode::Sleep, scan_code_str: "Sleep", key_code: KeyCode::Unknown, key_code_str: "", event_key_code: 0, vkey: "VK_SLEEP", us_user_settings_label: "", general_user_settings_label: "" },
    KeyCodeMapping { immutable: true, scan_code: ScanCode::WakeUp, scan_code_str: "WakeUp", key_code: KeyCode::Unknown, key_code_str: "", event_key_code: 0, vkey: "", us_user_settings_label: "", general_user_settings_label: "" },
    KeyCodeMapping { immutable: false, scan_code: ScanCode::KeyA, scan_code_str: "KeyA", key_code: KeyCode::KeyA, key_code_str: "A", event_key_code: 65, vkey: "VK_A", us_user_settings_label: "", general_user_settings_label: "" },
    KeyCodeMapping { immutable: false, scan_code: ScanCode::KeyB, scan_code_str: "KeyB", key_code: KeyCode::KeyB, key_code_str: "B", event_key_code: 66, vkey: "VK_B", us_user_settings_label: "", general_user_settings_label: "" },
    KeyCodeMapping { immutable: false, scan_code: ScanCode::KeyC, scan_code_str: "KeyC", key_code: KeyCode::KeyC, key_code_str: "C", event_key_code: 67, vkey: "VK_C", us_user_settings_label: "", general_user_settings_label: "" },
    KeyCodeMapping { immutable: false, scan_code: ScanCode::KeyD, scan_code_str: "KeyD", key_code: KeyCode::KeyD, key_code_str: "D", event_key_code: 68, vkey: "VK_D", us_user_settings_label: "", general_user_settings_label: "" },
    KeyCodeMapping { immutable: false, scan_code: ScanCode::KeyE, scan_code_str: "KeyE", key_code: KeyCode::KeyE, key_code_str: "E", event_key_code: 69, vkey: "VK_E", us_user_settings_label: "", general_user_settings_label: "" },
    KeyCodeMapping { immutable: false, scan_code: ScanCode::KeyF, scan_code_str: "KeyF", key_code: KeyCode::KeyF, key_code_str: "F", event_key_code: 70, vkey: "VK_F", us_user_settings_label: "", general_user_settings_label: "" },
    KeyCodeMapping { immutable: false, scan_code: ScanCode::KeyG, scan_code_str: "KeyG", key_code: KeyCode::KeyG, key_code_str: "G", event_key_code: 71, vkey: "VK_G", us_user_settings_label: "", general_user_settings_label: "" },
    KeyCodeMapping { immutable: false, scan_code: ScanCode::KeyH, scan_code_str: "KeyH", key_code: KeyCode::KeyH, key_code_str: "H", event_key_code: 72, vkey: "VK_H", us_user_settings_label: "", general_user_settings_label: "" },
    KeyCodeMapping { immutable: false, scan_code: ScanCode::KeyI, scan_code_str: "KeyI", key_code: KeyCode::KeyI, key_code_str: "I", event_key_code: 73, vkey: "VK_I", us_user_settings_label: "", general_user_settings_label: "" },
    KeyCodeMapping { immutable: false, scan_code: ScanCode::KeyJ, scan_code_str: "KeyJ", key_code: KeyCode::KeyJ, key_code_str: "J", event_key_code: 74, vkey: "VK_J", us_user_settings_label: "", general_user_settings_label: "" },
    KeyCodeMapping { immutable: false, scan_code: ScanCode::KeyK, scan_code_str: "KeyK", key_code: KeyCode::KeyK, key_code_str: "K", event_key_code: 75, vkey: "VK_K", us_user_settings_label: "", general_user_settings_label: "" },
    KeyCodeMapping { immutable: false, scan_code: ScanCode::KeyL, scan_code_str: "KeyL", key_code: KeyCode::KeyL, key_code_str: "L", event_key_code: 76, vkey: "VK_L", us_user_settings_label: "", general_user_settings_label: "" },
    KeyCodeMapping { immutable: false, scan_code: ScanCode::KeyM, scan_code_str: "KeyM", key_code: KeyCode::KeyM, key_code_str: "M", event_key_code: 77, vkey: "VK_M", us_user_settings_label: "", general_user_settings_label: "" },
    KeyCodeMapping { immutable: false, scan_code: ScanCode::KeyN, scan_code_str: "KeyN", key_code: KeyCode::KeyN, key_code_str: "N", event_key_code: 78, vkey: "VK_N", us_user_settings_label: "", general_user_settings_label: "" },
    KeyCodeMapping { immutable: false, scan_code: ScanCode::KeyO, scan_code_str: "KeyO", key_code: KeyCode::KeyO, key_code_str: "O", event_key_code: 79, vkey: "VK_O", us_user_settings_label: "", general_user_settings_label: "" },
    KeyCodeMapping { immutable: false, scan_code: ScanCode::KeyP, scan_code_str: "KeyP", key_code: KeyCode::KeyP, key_code_str: "P", event_key_code: 80, vkey: "VK_P", us_user_settings_label: "", general_user_settings_label: "" },
    KeyCodeMapping { immutable: false, scan_code: ScanCode::KeyQ, scan_code_str: "KeyQ", key_code: KeyCode::KeyQ, key_code_str: "Q", event_key_code: 81, vkey: "VK_Q", us_user_settings_label: "", general_user_settings_label: "" },
    KeyCodeMapping { immutable: false, scan_code: ScanCode::KeyR, scan_code_str: "KeyR", key_code: KeyCode::KeyR, key_code_str: "R", event_key_code: 82, vkey: "VK_R", us_user_settings_label: "", general_user_settings_label: "" },
    KeyCodeMapping { immutable: false, scan_code: ScanCode::KeyS, scan_code_str: "KeyS", key_code: KeyCode::KeyS, key_code_str: "S", event_key_code: 83, vkey: "VK_S", us_user_settings_label: "", general_user_settings_label: "" },
    KeyCodeMapping { immutable: false, scan_code: ScanCode::KeyT, scan_code_str: "KeyT", key_code: KeyCode::KeyT, key_code_str: "T", event_key_code: 84, vkey: "VK_T", us_user_settings_label: "", general_user_settings_label: "" },
    KeyCodeMapping { immutable: false, scan_code: ScanCode::KeyU, scan_code_str: "KeyU", key_code: KeyCode::KeyU, key_code_str: "U", event_key_code: 85, vkey: "VK_U", us_user_settings_label: "", general_user_settings_label: "" },
    KeyCodeMapping { immutable: false, scan_code: ScanCode::KeyV, scan_code_str: "KeyV", key_code: KeyCode::KeyV, key_code_str: "V", event_key_code: 86, vkey: "VK_V", us_user_settings_label: "", general_user_settings_label: "" },
    KeyCodeMapping { immutable: false, scan_code: ScanCode::KeyW, scan_code_str: "KeyW", key_code: KeyCode::KeyW, key_code_str: "W", event_key_code: 87, vkey: "VK_W", us_user_settings_label: "", general_user_settings_label: "" },
    KeyCodeMapping { immutable: false, scan_code: ScanCode::KeyX, scan_code_str: "KeyX", key_code: KeyCode::KeyX, key_code_str: "X", event_key_code: 88, vkey: "VK_X", us_user_settings_label: "", general_user_settings_label: "" },
    KeyCodeMapping { immutable: false, scan_code: ScanCode::KeyY, scan_code_str: "KeyY", key_code: KeyCode::KeyY, key_code_str: "Y", event_key_code: 89, vkey: "VK_Y", us_user_settings_label: "", general_user_settings_label: "" },
    KeyCodeMapping { immutable: false, scan_code: ScanCode::KeyZ, scan_code_str: "KeyZ", key_code: KeyCode::KeyZ, key_code_str: "Z", event_key_code: 90, vkey: "VK_Z", us_user_settings_label: "", general_user_settings_label: "" },
    KeyCodeMapping { immutable: false, scan_code: ScanCode::Digit1, scan_code_str: "Digit1", key_code: KeyCode::Digit1, key_code_str: "1", event_key_code: 49, vkey: "VK_1", us_user_settings_label: "", general_user_settings_label: "" },
    KeyCodeMapping { immutable: false, scan_code: ScanCode::Digit2, scan_code_str: "Digit2", key_code: KeyCode::Digit2, key_code_str: "2", event_key_code: 50, vkey: "VK_2", us_user_settings_label: "", general_user_settings_label: "" },
    KeyCodeMapping { immutable: false, scan_code: ScanCode::Digit3, scan_code_str: "Digit3", key_code: KeyCode::Digit3, key_code_str: "3", event_key_code: 51, vkey: "VK_3", us_user_settings_label: "", general_user_settings_label: "" },
    KeyCodeMapping { immutable: false, scan_code: ScanCode::Digit4, scan_code_str: "Digit4", key_code: KeyCode::Digit4, key_code_str: "4", event_key_code: 52, vkey: "VK_4", us_user_settings_label: "", general_user_settings_label: "" },
    KeyCodeMapping { immutable: false, scan_code: ScanCode::Digit5, scan_code_str: "Digit5", key_code: KeyCode::Digit5, key_code_str: "5", event_key_code: 53, vkey: "VK_5", us_user_settings_label: "", general_user_settings_label: "" },
    KeyCodeMapping { immutable: false, scan_code: ScanCode::Digit6, scan_code_str: "Digit6", key_code: KeyCode::Digit6, key_code_str: "6", event_key_code: 54, vkey: "VK_6", us_user_settings_label: "", general_user_settings_label: "" },
    KeyCodeMapping { immutable: false, scan_code: ScanCode::Digit7, scan_code_str: "Digit7", key_code: KeyCode::Digit7, key_code_str: "7", event_key_code: 55, vkey: "VK_7", us_user_settings_label: "", general_user_settings_label: "" },
    KeyCodeMapping { immutable: false, scan_code: ScanCode::Digit8, scan_code_str: "Digit8", key_code: KeyCode::Digit8, key_code_str: "8", event_key_code: 56, vkey: "VK_8", us_user_settings_label: "", general_user_settings_label: "" },
    KeyCodeMapping { immutable: false, scan_code: ScanCode::Digit9, scan_code_str: "Digit9", key_code: KeyCode::Digit9, key_code_str: "9", event_key_code: 57, vkey: "VK_9", us_user_settings_label: "", general_user_settings_label: "" },
    KeyCodeMapping { immutable: false, scan_code: ScanCode::Digit0, scan_code_str: "Digit0", key_code: KeyCode::Digit0, key_code_str: "0", event_key_code: 48, vkey: "VK_0", us_user_settings_label: "", general_user_settings_label: "" },
    KeyCodeMapping { immutable: true, scan_code: ScanCode::Enter, scan_code_str: "Enter", key_code: KeyCode::Enter, key_code_str: "Enter", event_key_code: 13, vkey: "VK_RETURN", us_user_settings_label: "", general_user_settings_label: "" },
    KeyCodeMapping { immutable: true, scan_code: ScanCode::Escape, scan_code_str: "Escape", key_code: KeyCode::Escape, key_code_str: "Escape", event_key_code: 27, vkey: "VK_ESCAPE", us_user_settings_label: "", general_user_settings_label: "" },
    KeyCodeMapping { immutable: true, scan_code: ScanCode::Backspace, scan_code_str: "Backspace", key_code: KeyCode::Backspace, key_code_str: "Backspace", event_key_code: 8, vkey: "VK_BACK", us_user_settings_label: "", general_user_settings_label: "" },
    KeyCodeMapping { immutable: true, scan_code: ScanCode::Tab, scan_code_str: "Tab", key_code: KeyCode::Tab, key_code_str: "Tab", event_key_code: 9, vkey: "VK_TAB", us_user_settings_label: "", general_user_settings_label: "" },
    KeyCodeMapping { immutable: true, scan_code: ScanCode::Space, scan_code_str: "Space", key_code: KeyCode::Space, key_code_str: "Space", event_key_code: 32, vkey: "VK_SPACE", us_user_settings_label: "", general_user_settings_label: "" },
    KeyCodeMapping { immutable: false, scan_code: ScanCode::Minus, scan_code_str: "Minus", key_code: KeyCode::Minus, key_code_str: "-", event_key_code: 189, vkey: "VK_OEM_MINUS", us_user_settings_label: "-", general_user_settings_label: "OEM_MINUS" },
    KeyCodeMapping { immutable: false, scan_code: ScanCode::Equal, scan_code_str: "Equal", key_code: KeyCode::Equal, key_code_str: "=", event_key_code: 187, vkey: "VK_OEM_PLUS", us_user_settings_label: "=", general_user_settings_label: "OEM_PLUS" },
    KeyCodeMapping { immutable: false, scan_code: ScanCode::BracketLeft, scan_code_str: "BracketLeft", key_code: KeyCode::BracketLeft, key_code_str: "[", event_key_code: 219, vkey: "VK_OEM_4", us_user_settings_label: "[", general_user_settings_label: "OEM_4" },
    KeyCodeMapping { immutable: false, scan_code: ScanCode::BracketRight, scan_code_str: "BracketRight", key_code: KeyCode::BracketRight, key_code_str: "]", event_key_code: 221, vkey: "VK_OEM_6", us_user_settings_label: "]", general_user_settings_label: "OEM_6" },
    KeyCodeMapping { immutable: false, scan_code: ScanCode::Backslash, scan_code_str: "Backslash", key_code: KeyCode::Backslash, key_code_str: "\\", event_key_code: 220, vkey: "VK_OEM_5", us_user_settings_label: "\\", general_user_settings_label: "OEM_5" },
    KeyCodeMapping { immutable: false, scan_code: ScanCode::IntlHash, scan_code_str: "IntlHash", key_code: KeyCode::Unknown, key_code_str: "", event_key_code: 0, vkey: "", us_user_settings_label: "", general_user_settings_label: "" },
    KeyCodeMapping { immutable: false, scan_code: ScanCode::Semicolon, scan_code_str: "Semicolon", key_code: KeyCode::Semicolon, key_code_str: ";", event_key_code: 186, vkey: "VK_OEM_1", us_user_settings_label: ";", general_user_settings_label: "OEM_1" },
    KeyCodeMapping { immutable: false, scan_code: ScanCode::Quote, scan_code_str: "Quote", key_code: KeyCode::Quote, key_code_str: "'", event_key_code: 222, vkey: "VK_OEM_7", us_user_settings_label: "'", general_user_settings_label: "OEM_7" },
    KeyCodeMapping { immutable: false, scan_code: ScanCode::Backquote, scan_code_str: "Backquote", key_code: KeyCode::Backquote, key_code_str: "`", event_key_code: 192, vkey: "VK_OEM_3", us_user_settings_label: "`", general_user_settings_label: "OEM_3" },
    KeyCodeMapping { immutable: false, scan_code: ScanCode::Comma, scan_code_str: "Comma", key_code: KeyCode::Comma, key_code_str: ",", event_key_code: 188, vkey: "VK_OEM_COMMA", us_user_settings_label: ",", general_user_settings_label: "OEM_COMMA" },
    KeyCodeMapping { immutable: false, scan_code: ScanCode::Period, scan_code_str: "Period", key_code: KeyCode::Period, key_code_str: ".", event_key_code: 190, vkey: "VK_OEM_PERIOD", us_user_settings_label: ".", general_user_settings_label: "OEM_PERIOD" },
    KeyCodeMapping { immutable: false, scan_code: ScanCode::Slash, scan_code_str: "Slash", key_code: KeyCode::Slash, key_code_str: "/", event_key_code: 191, vkey: "VK_OEM_2", us_user_settings_label: "/", general_user_settings_label: "OEM_2" },
    KeyCodeMapping { immutable: true, scan_code: ScanCode::CapsLock, scan_code_str: "CapsLock", key_code: KeyCode::CapsLock, key_code_str: "CapsLock", event_key_code: 20, vkey: "VK_CAPITAL", us_user_settings_label: "", general_user_settings_label: "" },
    KeyCodeMapping { immutable: true, scan_code: ScanCode::F1, scan_code_str: "F1", key_code: KeyCode::F1, key_code_str: "F1", event_key_code: 112, vkey: "VK_F1", us_user_settings_label: "", general_user_settings_label: "" },
    KeyCodeMapping { immutable: true, scan_code: ScanCode::F2, scan_code_str: "F2", key_code: KeyCode::F2, key_code_str: "F2", event_key_code: 113, vkey: "VK_F2", us_user_settings_label: "", general_user_settings_label: "" },
    KeyCodeMapping { immutable: true, scan_code: ScanCode::F3, scan_code_str: "F3", key_code: KeyCode::F3, key_code_str: "F3", event_key_code: 114, vkey: "VK_F3", us_user_settings_label: "", general_user_settings_label: "" },
    KeyCodeMapping { immutable: true, scan_code: ScanCode::F4, scan_code_str: "F4", key_code: KeyCode::F4, key_code_str: "F4", event_key_code: 115, vkey: "VK_F4", us_user_settings_label: "", general_user_settings_label: "" },
    KeyCodeMapping { immutable: true, scan_code: ScanCode::F5, scan_code_str: "F5", key_code: KeyCode::F5, key_code_str: "F5", event_key_code: 116, vkey: "VK_F5", us_user_settings_label: "", general_user_settings_label: "" },
    KeyCodeMapping { immutable: true, scan_code: ScanCode::F6, scan_code_str: "F6", key_code: KeyCode::F6, key_code_str: "F6", event_key_code: 117, vkey: "VK_F6", us_user_settings_label: "", general_user_settings_label: "" },
    KeyCodeMapping { immutable: true, scan_code: ScanCode::F7, scan_code_str: "F7", key_code: KeyCode::F7, key_code_str: "F7", event_key_code: 118, vkey: "VK_F7", us_user_settings_label: "", general_user_settings_label: "" },
    KeyCodeMapping { immutable: true, scan_code: ScanCode::F8, scan_code_str: "F8", key_code: KeyCode::F8, key_code_str: "F8", event_key_code: 119, vkey: "VK_F8", us_user_settings_label: "", general_user_settings_label: "" },
    KeyCodeMapping { immutable: true, scan_code: ScanCode::F9, scan_code_str: "F9", key_code: KeyCode::F9, key_code_str: "F9", event_key_code: 120, vkey: "VK_F9", us_user_settings_label: "", general_user_settings_label: "" },
    KeyCodeMapping { immutable: true, scan_code: ScanCode::F10, scan_code_str: "F10", key_code: KeyCode::F10, key_code_str: "F10", event_key_code: 121, vkey: "VK_F10", us_user_settings_label: "", general_user_settings_label: "" },
    KeyCodeMapping { immutable: true, scan_code: ScanCode::F11, scan_code_str: "F11", key_code: KeyCode::F11, key_code_str: "F11", event_key_code: 122, vkey: "VK_F11", us_user_settings_label: "", general_user_settings_label: "" },
    KeyCodeMapping { immutable: true, scan_code: ScanCode::F12, scan_code_str: "F12", key_code: KeyCode::F12, key_code_str: "F12", event_key_code: 123, vkey: "VK_F12", us_user_settings_label: "", general_user_settings_label: "" },
    KeyCodeMapping { immutable: true, scan_code: ScanCode::PrintScreen, scan_code_str: "PrintScreen", key_code: KeyCode::Unknown, key_code_str: "", event_key_code: 0, vkey: "", us_user_settings_label: "", general_user_settings_label: "" },
    KeyCodeMapping { immutable: true, scan_code: ScanCode::ScrollLock, scan_code_str: "ScrollLock", key_code: KeyCode::ScrollLock, key_code_str: "ScrollLock", event_key_code: 145, vkey: "VK_SCROLL", us_user_settings_label: "", general_user_settings_label: "" },
    KeyCodeMapping { immutable: true, scan_code: ScanCode::Pause, scan_code_str: "Pause", key_code: KeyCode::PauseBreak, key_code_str: "PauseBreak", event_key_code: 19, vkey: "VK_PAUSE", us_user_settings_label: "", general_user_settings_label: "" },
    KeyCodeMapping { immutable: true, scan_code: ScanCode::Insert, scan_code_str: "Insert", key_code: KeyCode::Insert, key_code_str: "Insert", event_key_code: 45, vkey: "VK_INSERT", us_user_settings_label: "", general_user_settings_label: "" },
    KeyCodeMapping { immutable: true, scan_code: ScanCode::Home, scan_code_str: "Home", key_code: KeyCode::Home, key_code_str: "Home", event_key_code: 36, vkey: "VK_HOME", us_user_settings_label: "", general_user_settings_label: "" },
    KeyCodeMapping { immutable: true, scan_code: ScanCode::PageUp, scan_code_str: "PageUp", key_code: KeyCode::PageUp, key_code_str: "PageUp", event_key_code: 33, vkey: "VK_PRIOR", us_user_settings_label: "", general_user_settings_label: "" },
    KeyCodeMapping { immutable: true, scan_code: ScanCode::Delete, scan_code_str: "Delete", key_code: KeyCode::Delete, key_code_str: "Del", event_key_code: 46, vkey: "VK_DELETE", us_user_settings_label: "Delete", general_user_settings_label: "" },
    KeyCodeMapping { immutable: true, scan_code: ScanCode::End, scan_code_str: "End", key_code: KeyCode::End, key_code_str: "End", event_key_code: 35, vkey: "VK_END", us_user_settings_label: "", general_user_settings_label: "" },
    KeyCodeMapping { immutable: true, scan_code: ScanCode::PageDown, scan_code_str: "PageDown", key_code: KeyCode::PageDown, key_code_str: "PageDown", event_key_code: 34, vkey: "VK_NEXT", us_user_settings_label: "", general_user_settings_label: "" },
    KeyCodeMapping { immutable: true, scan_code: ScanCode::ArrowRight, scan_code_str: "ArrowRight", key_code: KeyCode::RightArrow, key_code_str: "RightArrow", event_key_code: 39, vkey: "VK_RIGHT", us_user_settings_label: "Right", general_user_settings_label: "" },
    KeyCodeMapping { immutable: true, scan_code: ScanCode::ArrowLeft, scan_code_str: "ArrowLeft", key_code: KeyCode::LeftArrow, key_code_str: "LeftArrow", event_key_code: 37, vkey: "VK_LEFT", us_user_settings_label: "Left", general_user_settings_label: "" },
    KeyCodeMapping { immutable: true, scan_code: ScanCode::ArrowDown, scan_code_str: "ArrowDown", key_code: KeyCode::DownArrow, key_code_str: "DownArrow", event_key_code: 40, vkey: "VK_DOWN", us_user_settings_label: "Down", general_user_settings_label: "" },
    KeyCodeMapping { immutable: true, scan_code: ScanCode::ArrowUp, scan_code_str: "ArrowUp", key_code: KeyCode::UpArrow, key_code_str: "UpArrow", event_key_code: 38, vkey: "VK_UP", us_user_settings_label: "Up", general_user_settings_label: "" },
    KeyCodeMapping { immutable: true, scan_code: ScanCode::NumLock, scan_code_str: "NumLock", key_code: KeyCode::NumLock, key_code_str: "NumLock", event_key_code: 144, vkey: "VK_NUMLOCK", us_user_settings_label: "", general_user_settings_label: "" },
    KeyCodeMapping { immutable: true, scan_code: ScanCode::NumpadDivide, scan_code_str: "NumpadDivide", key_code: KeyCode::NumpadDivide, key_code_str: "NumPad_Divide", event_key_code: 111, vkey: "VK_DIVIDE", us_user_settings_label: "", general_user_settings_label: "" },
    KeyCodeMapping { immutable: true, scan_code: ScanCode::NumpadMultiply, scan_code_str: "NumpadMultiply", key_code: KeyCode::NumpadMultiply, key_code_str: "NumPad_Multiply", event_key_code: 106, vkey: "VK_MULTIPLY", us_user_settings_label: "", general_user_settings_label: "" },
    KeyCodeMapping { immutable: true, scan_code: ScanCode::NumpadSubtract, scan_code_str: "NumpadSubtract", key_code: KeyCode::NumpadSubtract, key_code_str: "NumPad_Subtract", event_key_code: 109, vkey: "VK_SUBTRACT", us_user_settings_label: "", general_user_settings_label: "" },
    KeyCodeMapping { immutable: true, scan_code: ScanCode::NumpadAdd, scan_code_str: "NumpadAdd", key_code: KeyCode::NumpadAdd, key_code_str: "NumPad_Add", event_key_code: 107, vkey: "VK_ADD", us_user_settings_label: "", general_user_settings_label: "" },
    KeyCodeMapping { immutable: true, scan_code: ScanCode::NumpadEnter, scan_code_str: "NumpadEnter", key_code: KeyCode::Enter, key_code_str: "", event_key_code: 0, vkey: "", us_user_settings_label: "", general_user_settings_label: "" },
    KeyCodeMapping { immutable: true, scan_code: ScanCode::Numpad1, scan_code_str: "Numpad1", key_code: KeyCode::Numpad1, key_code_str: "NumPad1", event_key_code: 97, vkey: "VK_NUMPAD1", us_user_settings_label: "", general_user_settings_label: "" },
    KeyCodeMapping { immutable: true, scan_code: ScanCode::Numpad2, scan_code_str: "Numpad2", key_code: KeyCode::Numpad2, key_code_str: "NumPad2", event_key_code: 98, vkey: "VK_NUMPAD2", us_user_settings_label: "", general_user_settings_label: "" },
    KeyCodeMapping { immutable: true, scan_code: ScanCode::Numpad3, scan_code_str: "Numpad3", key_code: KeyCode::Numpad3, key_code_str: "NumPad3", event_key_code: 99, vkey: "VK_NUMPAD3", us_user_settings_label: "", general_user_settings_label: "" },
    KeyCodeMapping { immutable: true, scan_code: ScanCode::Numpad4, scan_code_str: "Numpad4", key_code: KeyCode::Numpad4, key_code_str: "NumPad4", event_key_code: 100, vkey: "VK_NUMPAD4", us_user_settings_label: "", general_user_settings_label: "" },
    KeyCodeMapping { immutable: true, scan_code: ScanCode::Numpad5, scan_code_str: "Numpad5", key_code: KeyCode::Numpad5, key_code_str: "NumPad5", event_key_code: 101, vkey: "VK_NUMPAD5", us_user_settings_label: "", general_user_settings_label: "" },
    KeyCodeMapping { immutable: true, scan_code: ScanCode::Numpad6, scan_code_str: "Numpad6", key_code: KeyCode::Numpad6, key_code_str: "NumPad6", event_key_code: 102, vkey: "VK_NUMPAD6", us_user_settings_label: "", general_user_settings_label: "" },
    KeyCodeMapping { immutable: true, scan_code: ScanCode::Numpad7, scan_code_str: "Numpad7", key_code: KeyCode::Numpad7, key_code_str: "NumPad7", event_key_code: 103, vkey: "VK_NUMPAD7", us_user_settings_label: "", general_user_settings_label: "" },
    KeyCodeMapping { immutable: true, scan_code: ScanCode::Numpad8, scan_code_str: "Numpad8", key_code: KeyCode::Numpad8, key_code_str: "NumPad8", event_key_code: 104, vkey: "VK_NUMPAD8", us_user_settings_label: "", general_user_settings_label: "" },
    KeyCodeMapping { immutable: true, scan_code: ScanCode::Numpad9, scan_code_str: "Numpad9", key_code: KeyCode::Numpad9, key_code_str: "NumPad9", event_key_code: 105, vkey: "VK_NUMPAD9", us_user_settings_label: "", general_user_settings_label: "" },
    KeyCodeMapping { immutable: true, scan_code: ScanCode::Numpad0, scan_code_str: "Numpad0", key_code: KeyCode::Numpad0, key_code_str: "NumPad0", event_key_code: 96, vkey: "VK_NUMPAD0", us_user_settings_label: "", general_user_settings_label: "" },
    KeyCodeMapping { immutable: true, scan_code: ScanCode::NumpadDecimal, scan_code_str: "NumpadDecimal", key_code: KeyCode::NumpadDecimal, key_code_str: "NumPad_Decimal", event_key_code: 110, vkey: "VK_DECIMAL", us_user_settings_label: "", general_user_settings_label: "" },
    KeyCodeMapping { immutable: false, scan_code: ScanCode::IntlBackslash, scan_code_str: "IntlBackslash", key_code: KeyCode::IntlBackslash, key_code_str: "OEM_102", event_key_code: 226, vkey: "VK_OEM_102", us_user_settings_label: "", general_user_settings_label: "" },
    KeyCodeMapping { immutable: true, scan_code: ScanCode::ContextMenu, scan_code_str: "ContextMenu", key_code: KeyCode::ContextMenu, key_code_str: "ContextMenu", event_key_code: 93, vkey: "", us_user_settings_label: "", general_user_settings_label: "" },
    KeyCodeMapping { immutable: true, scan_code: ScanCode::Power, scan_code_str: "Power", key_code: KeyCode::Unknown, key_code_str: "", event_key_code: 0, vkey: "", us_user_settings_label: "", general_user_settings_label: "" },
    KeyCodeMapping { immutable: true, scan_code: ScanCode::NumpadEqual, scan_code_str: "NumpadEqual", key_code: KeyCode::Unknown, key_code_str: "", event_key_code: 0, vkey: "", us_user_settings_label: "", general_user_settings_label: "" },
    KeyCodeMapping { immutable: true, scan_code: ScanCode::F13, scan_code_str: "F13", key_code: KeyCode::F13, key_code_str: "F13", event_key_code: 124, vkey: "VK_F13", us_user_settings_label: "", general_user_settings_label: "" },
    KeyCodeMapping { immutable: true, scan_code: ScanCode::F14, scan_code_str: "F14", key_code: KeyCode::F14, key_code_str: "F14", event_key_code: 125, vkey: "VK_F14", us_user_settings_label: "", general_user_settings_label: "" },
    KeyCodeMapping { immutable: true, scan_code: ScanCode::F15, scan_code_str: "F15", key_code: KeyCode::F15, key_code_str: "F15", event_key_code: 126, vkey: "VK_F15", us_user_settings_label: "", general_user_settings_label: "" },
    KeyCodeMapping { immutable: true, scan_code: ScanCode::F16, scan_code_str: "F16", key_code: KeyCode::F16, key_code_str: "F16", event_key_code: 127, vkey: "VK_F16", us_user_settings_label: "", general_user_settings_label: "" },
    KeyCodeMapping { immutable: true, scan_code: ScanCode::F17, scan_code_str: "F17", key_code: KeyCode::F17, key_code_str: "F17", event_key_code: 128, vkey: "VK_F17", us_user_settings_label: "", general_user_settings_label: "" },
    KeyCodeMapping { immutable: true, scan_code: ScanCode::F18, scan_code_str: "F18", key_code: KeyCode::F18, key_code_str: "F18", event_key_code: 129, vkey: "VK_F18", us_user_settings_label: "", general_user_settings_label: "" },
    KeyCodeMapping { immutable: true, scan_code: ScanCode::F19, scan_code_str: "F19", key_code: KeyCode::F19, key_code_str: "F19", event_key_code: 130, vkey: "VK_F19", us_user_settings_label: "", general_user_settings_label: "" },
    KeyCodeMapping { immutable: true, scan_code: ScanCode::F20, scan_code_str: "F20", key_code: KeyCode::F20, key_code_str: "F20", event_key_code: 131, vkey: "VK_F20", us_user_settings_label: "", general_user_settings_label: "" },
    KeyCodeMapping { immutable: true, scan_code: ScanCode::F21, scan_code_str: "F21", key_code: KeyCode::F21, key_code_str: "F21", event_key_code: 132, vkey: "VK_F21", us_user_settings_label: "", general_user_settings_label: "" },
    KeyCodeMapping { immutable: true, scan_code: ScanCode::F22, scan_code_str: "F22", key_code: KeyCode::F22, key_code_str: "F22", event_key_code: 133, vkey: "VK_F22", us_user_settings_label: "", general_user_settings_label: "" },
    KeyCodeMapping { immutable: true, scan_code: ScanCode::F23, scan_code_str: "F23", key_code: KeyCode::F23, key_code_str: "F23", event_key_code: 134, vkey: "VK_F23", us_user_settings_label: "", general_user_settings_label: "" },
    KeyCodeMapping { immutable: true, scan_code: ScanCode::F24, scan_code_str: "F24", key_code: KeyCode::F24, key_code_str: "F24", event_key_code: 135, vkey: "VK_F24", us_user_settings_label: "", general_user_settings_label: "" },
    KeyCodeMapping { immutable: true, scan_code: ScanCode::Open, scan_code_str: "Open", key_code: KeyCode::Unknown, key_code_str: "", event_key_code: 0, vkey: "", us_user_settings_label: "", general_user_settings_label: "" },
    KeyCodeMapping { immutable: true, scan_code: ScanCode::Help, scan_code_str: "Help", key_code: KeyCode::Unknown, key_code_str: "", event_key_code: 0, vkey: "", us_user_settings_label: "", general_user_settings_label: "" },
    KeyCodeMapping { immutable: true, scan_code: ScanCode::Select, scan_code_str: "Select", key_code: KeyCode::Unknown, key_code_str: "", event_key_code: 0, vkey: "", us_user_settings_label: "", general_user_settings_label: "" },
    KeyCodeMapping { immutable: true, scan_code: ScanCode::Again, scan_code_str: "Again", key_code: KeyCode::Unknown, key_code_str: "", event_key_code: 0, vkey: "", us_user_settings_label: "", general_user_settings_label: "" },
    KeyCodeMapping { immutable: true, scan_code: ScanCode::Undo, scan_code_str: "Undo", key_code: KeyCode::Unknown, key_code_str: "", event_key_code: 0, vkey: "", us_user_settings_label: "", general_user_settings_label: "" },
    KeyCodeMapping { immutable: true, scan_code: ScanCode::Cut, scan_code_str: "Cut", key_code: KeyCode::Unknown, key_code_str: "", event_key_code: 0, vkey: "", us_user_settings_label: "", general_user_settings_label: "" },
    KeyCodeMapping { immutable: true, scan_code: ScanCode::Copy, scan_code_str: "Copy", key_code: KeyCode::Unknown, key_code_str: "", event_key_code: 0, vkey: "", us_user_settings_label: "", general_user_settings_label: "" },
    KeyCodeMapping { immutable: true, scan_code: ScanCode::Paste, scan_code_str: "Paste", key_code: KeyCode::Unknown, key_code_str: "", event_key_code: 0, vkey: "", us_user_settings_label: "", general_user_settings_label: "" },
    KeyCodeMapping { immutable: true, scan_code: ScanCode::Find, scan_code_str: "Find", key_code: KeyCode::Unknown, key_code_str: "", event_key_code: 0, vkey: "", us_user_settings_label: "", general_user_settings_label: "" },
    KeyCodeMapping { immutable: true, scan_code: ScanCode::AudioVolumeMute, scan_code_str: "AudioVolumeMute", key_code: KeyCode::AudioVolumeMute, key_code_str: "AudioVolumeMute", event_key_code: 173, vkey: "VK_VOLUME_MUTE", us_user_settings_label: "", general_user_settings_label: "" },
    KeyCodeMapping { immutable: true, scan_code: ScanCode::AudioVolumeUp, scan_code_str: "AudioVolumeUp", key_code: KeyCode::AudioVolumeUp, key_code_str: "AudioVolumeUp", event_key_code: 175, vkey: "VK_VOLUME_UP", us_user_settings_label: "", general_user_settings_label: "" },
    KeyCodeMapping { immutable: true, scan_code: ScanCode::AudioVolumeDown, scan_code_str: "AudioVolumeDown", key_code: KeyCode::AudioVolumeDown, key_code_str: "AudioVolumeDown", event_key_code: 174, vkey: "VK_VOLUME_DOWN", us_user_settings_label: "", general_user_settings_label: "" },
    KeyCodeMapping { immutable: true, scan_code: ScanCode::NumpadComma, scan_code_str: "NumpadComma", key_code: KeyCode::NumpadSeparator, key_code_str: "NumPad_Separator", event_key_code: 108, vkey: "VK_SEPARATOR", us_user_settings_label: "", general_user_settings_label: "" },
    KeyCodeMapping { immutable: false, scan_code: ScanCode::IntlRo, scan_code_str: "IntlRo", key_code: KeyCode::AbntC1, key_code_str: "ABNT_C1", event_key_code: 193, vkey: "VK_ABNT_C1", us_user_settings_label: "", general_user_settings_label: "" },
    KeyCodeMapping { immutable: true, scan_code: ScanCode::KanaMode, scan_code_str: "KanaMode", key_code: KeyCode::Unknown, key_code_str: "", event_key_code: 0, vkey: "", us_user_settings_label: "", general_user_settings_label: "" },
    KeyCodeMapping { immutable: false, scan_code: ScanCode::IntlYen, scan_code_str: "IntlYen", key_code: KeyCode::Unknown, key_code_str: "", event_key_code: 0, vkey: "", us_user_settings_label: "", general_user_settings_label: "" },
    KeyCodeMapping { immutable: true, scan_code: ScanCode::Convert, scan_code_str: "Convert", key_code: KeyCode::Unknown, key_code_str: "", event_key_code: 0, vkey: "", us_user_settings_label: "", general_user_settings_label: "" },
    KeyCodeMapping { immutable: true, scan_code: ScanCode::NonConvert, scan_code_str: "NonConvert", key_code: KeyCode::Unknown, key_code_str: "", event_key_code: 0, vkey: "", us_user_settings_label: "", general_user_settings_label: "" },
    KeyCodeMapping { immutable: true, scan_code: ScanCode::Lang1, scan_code_str: "Lang1", key_code: KeyCode::Unknown, key_code_str: "", event_key_code: 0, vkey: "", us_user_settings_label: "", general_user_settings_label: "" },
    KeyCodeMapping { immutable: true, scan_code: ScanCode::Lang2, scan_code_str: "Lang2", key_code: KeyCode::Unknown, key_code_str: "", event_key_code: 0, vkey: "", us_user_settings_label: "", general_user_settings_label: "" },
    KeyCodeMapping { immutable: true, scan_code: ScanCode::Lang3, scan_code_str: "Lang3", key_code: KeyCode::Unknown, key_code_str: "", event_key_code: 0, vkey: "", us_user_settings_label: "", general_user_settings_label: "" },
    KeyCodeMapping { immutable: true, scan_code: ScanCode::Lang4, scan_code_str: "Lang4", key_code: KeyCode::Unknown, key_code_str: "", event_key_code: 0, vkey: "", us_user_settings_label: "", general_user_settings_label: "" },
    KeyCodeMapping { immutable: true, scan_code: ScanCode::Lang5, scan_code_str: "Lang5", key_code: KeyCode::Unknown, key_code_str: "", event_key_code: 0, vkey: "", us_user_settings_label: "", general_user_settings_label: "" },
    KeyCodeMapping { immutable: true, scan_code: ScanCode::Abort, scan_code_str: "Abort", key_code: KeyCode::Unknown, key_code_str: "", event_key_code: 0, vkey: "", us_user_settings_label: "", general_user_settings_label: "" },
    KeyCodeMapping { immutable: true, scan_code: ScanCode::Props, scan_code_str: "Props", key_code: KeyCode::Unknown, key_code_str: "", event_key_code: 0, vkey: "", us_user_settings_label: "", general_user_settings_label: "" },
    KeyCodeMapping { immutable: true, scan_code: ScanCode::NumpadParenLeft, scan_code_str: "NumpadParenLeft", key_code: KeyCode::Unknown, key_code_str: "", event_key_code: 0, vkey: "", us_user_settings_label: "", general_user_settings_label: "" },
    KeyCodeMapping { immutable: true, scan_code: ScanCode::NumpadParenRight, scan_code_str: "NumpadParenRight", key_code: KeyCode::Unknown, key_code_str: "", event_key_code: 0, vkey: "", us_user_settings_label: "", general_user_settings_label: "" },
    KeyCodeMapping { immutable: true, scan_code: ScanCode::NumpadBackspace, scan_code_str: "NumpadBackspace", key_code: KeyCode::Unknown, key_code_str: "", event_key_code: 0, vkey: "", us_user_settings_label: "", general_user_settings_label: "" },
    KeyCodeMapping { immutable: true, scan_code: ScanCode::NumpadMemoryStore, scan_code_str: "NumpadMemoryStore", key_code: KeyCode::Unknown, key_code_str: "", event_key_code: 0, vkey: "", us_user_settings_label: "", general_user_settings_label: "" },
    KeyCodeMapping { immutable: true, scan_code: ScanCode::NumpadMemoryRecall, scan_code_str: "NumpadMemoryRecall", key_code: KeyCode::Unknown, key_code_str: "", event_key_code: 0, vkey: "", us_user_settings_label: "", general_user_settings_label: "" },
    KeyCodeMapping { immutable: true, scan_code: ScanCode::NumpadMemoryClear, scan_code_str: "NumpadMemoryClear", key_code: KeyCode::Unknown, key_code_str: "", event_key_code: 0, vkey: "", us_user_settings_label: "", general_user_settings_label: "" },
    KeyCodeMapping { immutable: true, scan_code: ScanCode::NumpadMemoryAdd, scan_code_str: "NumpadMemoryAdd", key_code: KeyCode::Unknown, key_code_str: "", event_key_code: 0, vkey: "", us_user_settings_label: "", general_user_settings_label: "" },
    KeyCodeMapping { immutable: true, scan_code: ScanCode::NumpadMemorySubtract, scan_code_str: "NumpadMemorySubtract", key_code: KeyCode::Unknown, key_code_str: "", event_key_code: 0, vkey: "", us_user_settings_label: "", general_user_settings_label: "" },
    KeyCodeMapping { immutable: true, scan_code: ScanCode::NumpadClear, scan_code_str: "NumpadClear", key_code: KeyCode::Clear, key_code_str: "Clear", event_key_code: 12, vkey: "VK_CLEAR", us_user_settings_label: "", general_user_settings_label: "" },
    KeyCodeMapping { immutable: true, scan_code: ScanCode::NumpadClearEntry, scan_code_str: "NumpadClearEntry", key_code: KeyCode::Unknown, key_code_str: "", event_key_code: 0, vkey: "", us_user_settings_label: "", general_user_settings_label: "" },
    KeyCodeMapping { immutable: true, scan_code: ScanCode::None, scan_code_str: "", key_code: KeyCode::Ctrl, key_code_str: "Ctrl", event_key_code: 17, vkey: "VK_CONTROL", us_user_settings_label: "", general_user_settings_label: "" },
    KeyCodeMapping { immutable: true, scan_code: ScanCode::None, scan_code_str: "", key_code: KeyCode::Shift, key_code_str: "Shift", event_key_code: 16, vkey: "VK_SHIFT", us_user_settings_label: "", general_user_settings_label: "" },
    KeyCodeMapping { immutable: true, scan_code: ScanCode::None, scan_code_str: "", key_code: KeyCode::Alt, key_code_str: "Alt", event_key_code: 18, vkey: "VK_MENU", us_user_settings_label: "", general_user_settings_label: "" },
    KeyCodeMapping { immutable: true, scan_code: ScanCode::None, scan_code_str: "", key_code: KeyCode::Meta, key_code_str: "Meta", event_key_code: 91, vkey: "VK_COMMAND", us_user_settings_label: "", general_user_settings_label: "" },
    KeyCodeMapping { immutable: true, scan_code: ScanCode::ControlLeft, scan_code_str: "ControlLeft", key_code: KeyCode::Ctrl, key_code_str: "", event_key_code: 0, vkey: "VK_LCONTROL", us_user_settings_label: "", general_user_settings_label: "" },
    KeyCodeMapping { immutable: true, scan_code: ScanCode::ShiftLeft, scan_code_str: "ShiftLeft", key_code: KeyCode::Shift, key_code_str: "", event_key_code: 0, vkey: "VK_LSHIFT", us_user_settings_label: "", general_user_settings_label: "" },
    KeyCodeMapping { immutable: true, scan_code: ScanCode::AltLeft, scan_code_str: "AltLeft", key_code: KeyCode::Alt, key_code_str: "", event_key_code: 0, vkey: "VK_LMENU", us_user_settings_label: "", general_user_settings_label: "" },
    KeyCodeMapping { immutable: true, scan_code: ScanCode::MetaLeft, scan_code_str: "MetaLeft", key_code: KeyCode::Meta, key_code_str: "", event_key_code: 0, vkey: "VK_LWIN", us_user_settings_label: "", general_user_settings_label: "" },
    KeyCodeMapping { immutable: true, scan_code: ScanCode::ControlRight, scan_code_str: "ControlRight", key_code: KeyCode::Ctrl, key_code_str: "", event_key_code: 0, vkey: "VK_RCONTROL", us_user_settings_label: "", general_user_settings_label: "" },
    KeyCodeMapping { immutable: true, scan_code: ScanCode::ShiftRight, scan_code_str: "ShiftRight", key_code: KeyCode::Shift, key_code_str: "", event_key_code: 0, vkey: "VK_RSHIFT", us_user_settings_label: "", general_user_settings_label: "" },
    KeyCodeMapping { immutable: true, scan_code: ScanCode::AltRight, scan_code_str: "AltRight", key_code: KeyCode::Alt, key_code_str: "", event_key_code: 0, vkey: "VK_RMENU", us_user_settings_label: "", general_user_settings_label: "" },
    KeyCodeMapping { immutable: true, scan_code: ScanCode::MetaRight, scan_code_str: "MetaRight", key_code: KeyCode::Meta, key_code_str: "", event_key_code: 0, vkey: "VK_RWIN", us_user_settings_label: "", general_user_settings_label: "" },
    KeyCodeMapping { immutable: true, scan_code: ScanCode::BrightnessUp, scan_code_str: "BrightnessUp", key_code: KeyCode::Unknown, key_code_str: "", event_key_code: 0, vkey: "", us_user_settings_label: "", general_user_settings_label: "" },
    KeyCodeMapping { immutable: true, scan_code: ScanCode::BrightnessDown, scan_code_str: "BrightnessDown", key_code: KeyCode::Unknown, key_code_str: "", event_key_code: 0, vkey: "", us_user_settings_label: "", general_user_settings_label: "" },
    KeyCodeMapping { immutable: true, scan_code: ScanCode::MediaPlay, scan_code_str: "MediaPlay", key_code: KeyCode::Unknown, key_code_str: "", event_key_code: 0, vkey: "", us_user_settings_label: "", general_user_settings_label: "" },
    KeyCodeMapping { immutable: true, scan_code: ScanCode::MediaRecord, scan_code_str: "MediaRecord", key_code: KeyCode::Unknown, key_code_str: "", event_key_code: 0, vkey: "", us_user_settings_label: "", general_user_settings_label: "" },
    KeyCodeMapping { immutable: true, scan_code: ScanCode::MediaFastForward, scan_code_str: "MediaFastForward", key_code: KeyCode::Unknown, key_code_str: "", event_key_code: 0, vkey: "", us_user_settings_label: "", general_user_settings_label: "" },
    KeyCodeMapping { immutable: true, scan_code: ScanCode::MediaRewind, scan_code_str: "MediaRewind", key_code: KeyCode::Unknown, key_code_str: "", event_key_code: 0, vkey: "", us_user_settings_label: "", general_user_settings_label: "" },
    KeyCodeMapping { immutable: true, scan_code: ScanCode::MediaTrackNext, scan_code_str: "MediaTrackNext", key_code: KeyCode::MediaTrackNext, key_code_str: "MediaTrackNext", event_key_code: 176, vkey: "VK_MEDIA_NEXT_TRACK", us_user_settings_label: "", general_user_settings_label: "" },
    KeyCodeMapping { immutable: true, scan_code: ScanCode::MediaTrackPrevious, scan_code_str: "MediaTrackPrevious", key_code: KeyCode::MediaTrackPrevious, key_code_str: "MediaTrackPrevious", event_key_code: 177, vkey: "VK_MEDIA_PREV_TRACK", us_user_settings_label: "", general_user_settings_label: "" },
    KeyCodeMapping { immutable: true, scan_code: ScanCode::MediaStop, scan_code_str: "MediaStop", key_code: KeyCode::MediaStop, key_code_str: "MediaStop", event_key_code: 178, vkey: "VK_MEDIA_STOP", us_user_settings_label: "", general_user_settings_label: "" },
    KeyCodeMapping { immutable: true, scan_code: ScanCode::Eject, scan_code_str: "Eject", key_code: KeyCode::Unknown, key_code_str: "", event_key_code: 0, vkey: "", us_user_settings_label: "", general_user_settings_label: "" },
    KeyCodeMapping { immutable: true, scan_code: ScanCode::MediaPlayPause, scan_code_str: "MediaPlayPause", key_code: KeyCode::MediaPlayPause, key_code_str: "MediaPlayPause", event_key_code: 179, vkey: "VK_MEDIA_PLAY_PAUSE", us_user_settings_label: "", general_user_settings_label: "" },
    KeyCodeMapping { immutable: true, scan_code: ScanCode::MediaSelect, scan_code_str: "MediaSelect", key_code: KeyCode::LaunchMediaPlayer, key_code_str: "LaunchMediaPlayer", event_key_code: 181, vkey: "VK_MEDIA_LAUNCH_MEDIA_SELECT", us_user_settings_label: "", general_user_settings_label: "" },
    KeyCodeMapping { immutable: true, scan_code: ScanCode::LaunchMail, scan_code_str: "LaunchMail", key_code: KeyCode::LaunchMail, key_code_str: "LaunchMail", event_key_code: 180, vkey: "VK_MEDIA_LAUNCH_MAIL", us_user_settings_label: "", general_user_settings_label: "" },
    KeyCodeMapping { immutable: true, scan_code: ScanCode::LaunchApp2, scan_code_str: "LaunchApp2", key_code: KeyCode::LaunchApp2, key_code_str: "LaunchApp2", event_key_code: 183, vkey: "VK_MEDIA_LAUNCH_APP2", us_user_settings_label: "", general_user_settings_label: "" },
    KeyCodeMapping { immutable: true, scan_code: ScanCode::LaunchApp1, scan_code_str: "LaunchApp1", key_code: KeyCode::Unknown, key_code_str: "", event_key_code: 0, vkey: "VK_MEDIA_LAUNCH_APP1", us_user_settings_label: "", general_user_settings_label: "" },
    KeyCodeMapping { immutable: true, scan_code: ScanCode::SelectTask, scan_code_str: "SelectTask", key_code: KeyCode::Unknown, key_code_str: "", event_key_code: 0, vkey: "", us_user_settings_label: "", general_user_settings_label: "" },
    KeyCodeMapping { immutable: true, scan_code: ScanCode::LaunchScreenSaver, scan_code_str: "LaunchScreenSaver", key_code: KeyCode::Unknown, key_code_str: "", event_key_code: 0, vkey: "", us_user_settings_label: "", general_user_settings_label: "" },
    KeyCodeMapping { immutable: true, scan_code: ScanCode::BrowserSearch, scan_code_str: "BrowserSearch", key_code: KeyCode::BrowserSearch, key_code_str: "BrowserSearch", event_key_code: 170, vkey: "VK_BROWSER_SEARCH", us_user_settings_label: "", general_user_settings_label: "" },
    KeyCodeMapping { immutable: true, scan_code: ScanCode::BrowserHome, scan_code_str: "BrowserHome", key_code: KeyCode::BrowserHome, key_code_str: "BrowserHome", event_key_code: 172, vkey: "VK_BROWSER_HOME", us_user_settings_label: "", general_user_settings_label: "" },
    KeyCodeMapping { immutable: true, scan_code: ScanCode::BrowserBack, scan_code_str: "BrowserBack", key_code: KeyCode::BrowserBack, key_code_str: "BrowserBack", event_key_code: 166, vkey: "VK_BROWSER_BACK", us_user_settings_label: "", general_user_settings_label: "" },
    KeyCodeMapping { immutable: true, scan_code: ScanCode::BrowserForward, scan_code_str: "BrowserForward", key_code: KeyCode::BrowserForward, key_code_str: "BrowserForward", event_key_code: 167, vkey: "VK_BROWSER_FORWARD", us_user_settings_label: "", general_user_settings_label: "" },
    KeyCodeMapping { immutable: true, scan_code: ScanCode::BrowserStop, scan_code_str: "BrowserStop", key_code: KeyCode::Unknown, key_code_str: "", event_key_code: 0, vkey: "VK_BROWSER_STOP", us_user_settings_label: "", general_user_settings_label: "" },
    KeyCodeMapping { immutable: true, scan_code: ScanCode::BrowserRefresh, scan_code_str: "BrowserRefresh", key_code: KeyCode::Unknown, key_code_str: "", event_key_code: 0, vkey: "VK_BROWSER_REFRESH", us_user_settings_label: "", general_user_settings_label: "" },
    KeyCodeMapping { immutable: true, scan_code: ScanCode::BrowserFavorites, scan_code_str: "BrowserFavorites", key_code: KeyCode::Unknown, key_code_str: "", event_key_code: 0, vkey: "VK_BROWSER_FAVORITES", us_user_settings_label: "", general_user_settings_label: "" },
    KeyCodeMapping { immutable: true, scan_code: ScanCode::ZoomToggle, scan_code_str: "ZoomToggle", key_code: KeyCode::Unknown, key_code_str: "", event_key_code: 0, vkey: "", us_user_settings_label: "", general_user_settings_label: "" },
    KeyCodeMapping { immutable: true, scan_code: ScanCode::MailReply, scan_code_str: "MailReply", key_code: KeyCode::Unknown, key_code_str: "", event_key_code: 0, vkey: "", us_user_settings_label: "", general_user_settings_label: "" },
    KeyCodeMapping { immutable: true, scan_code: ScanCode::MailForward, scan_code_str: "MailForward", key_code: KeyCode::Unknown, key_code_str: "", event_key_code: 0, vkey: "", us_user_settings_label: "", general_user_settings_label: "" },
    KeyCodeMapping { immutable: true, scan_code: ScanCode::MailSend, scan_code_str: "MailSend", key_code: KeyCode::Unknown, key_code_str: "", event_key_code: 0, vkey: "", us_user_settings_label: "", general_user_settings_label: "" },
    KeyCodeMapping { immutable: true, scan_code: ScanCode::None, scan_code_str: "", key_code: KeyCode::KeyInComposition, key_code_str: "KeyInComposition", event_key_code: 229, vkey: "", us_user_settings_label: "", general_user_settings_label: "" },
    KeyCodeMapping { immutable: true, scan_code: ScanCode::None, scan_code_str: "", key_code: KeyCode::AbntC2, key_code_str: "ABNT_C2", event_key_code: 194, vkey: "VK_ABNT_C2", us_user_settings_label: "", general_user_settings_label: "" },
    KeyCodeMapping { immutable: true, scan_code: ScanCode::None, scan_code_str: "", key_code: KeyCode::Oem8, key_code_str: "OEM_8", event_key_code: 223, vkey: "VK_OEM_8", us_user_settings_label: "", general_user_settings_label: "" },
    KeyCodeMapping { immutable: true, scan_code: ScanCode::None, scan_code_str: "", key_code: KeyCode::Unknown, key_code_str: "", event_key_code: 0, vkey: "VK_KANA", us_user_settings_label: "", general_user_settings_label: "" },
    KeyCodeMapping { immutable: true, scan_code: ScanCode::None, scan_code_str: "", key_code: KeyCode::Unknown, key_code_str: "", event_key_code: 0, vkey: "VK_HANGUL", us_user_settings_label: "", general_user_settings_label: "" },
    KeyCodeMapping { immutable: true, scan_code: ScanCode::None, scan_code_str: "", key_code: KeyCode::Unknown, key_code_str: "", event_key_code: 0, vkey: "VK_JUNJA", us_user_settings_label: "", general_user_settings_label: "" },
    KeyCodeMapping { immutable: true, scan_code: ScanCode::None, scan_code_str: "", key_code: KeyCode::Unknown, key_code_str: "", event_key_code: 0, vkey: "VK_FINAL", us_user_settings_label: "", general_user_settings_label: "" },
    KeyCodeMapping { immutable: true, scan_code: ScanCode::None, scan_code_str: "", key_code: KeyCode::Unknown, key_code_str: "", event_key_code: 0, vkey: "VK_HANJA", us_user_settings_label: "", general_user_settings_label: "" },
    KeyCodeMapping { immutable: true, scan_code: ScanCode::None, scan_code_str: "", key_code: KeyCode::Unknown, key_code_str: "", event_key_code: 0, vkey: "VK_KANJI", us_user_settings_label: "", general_user_settings_label: "" },
    KeyCodeMapping { immutable: true, scan_code: ScanCode::None, scan_code_str: "", key_code: KeyCode::Unknown, key_code_str: "", event_key_code: 0, vkey: "VK_CONVERT", us_user_settings_label: "", general_user_settings_label: "" },
    KeyCodeMapping { immutable: true, scan_code: ScanCode::None, scan_code_str: "", key_code: KeyCode::Unknown, key_code_str: "", event_key_code: 0, vkey: "VK_NONCONVERT", us_user_settings_label: "", general_user_settings_label: "" },
    KeyCodeMapping { immutable: true, scan_code: ScanCode::None, scan_code_str: "", key_code: KeyCode::Unknown, key_code_str: "", event_key_code: 0, vkey: "VK_ACCEPT", us_user_settings_label: "", general_user_settings_label: "" },
    KeyCodeMapping { immutable: true, scan_code: ScanCode::None, scan_code_str: "", key_code: KeyCode::Unknown, key_code_str: "", event_key_code: 0, vkey: "VK_MODECHANGE", us_user_settings_label: "", general_user_settings_label: "" },
    KeyCodeMapping { immutable: true, scan_code: ScanCode::None, scan_code_str: "", key_code: KeyCode::Unknown, key_code_str: "", event_key_code: 0, vkey: "VK_SELECT", us_user_settings_label: "", general_user_settings_label: "" },
    KeyCodeMapping { immutable: true, scan_code: ScanCode::None, scan_code_str: "", key_code: KeyCode::Unknown, key_code_str: "", event_key_code: 0, vkey: "VK_PRINT", us_user_settings_label: "", general_user_settings_label: "" },
    KeyCodeMapping { immutable: true, scan_code: ScanCode::None, scan_code_str: "", key_code: KeyCode::Unknown, key_code_str: "", event_key_code: 0, vkey: "VK_EXECUTE", us_user_settings_label: "", general_user_settings_label: "" },
    KeyCodeMapping { immutable: true, scan_code: ScanCode::None, scan_code_str: "", key_code: KeyCode::Unknown, key_code_str: "", event_key_code: 0, vkey: "VK_SNAPSHOT", us_user_settings_label: "", general_user_settings_label: "" },
    KeyCodeMapping { immutable: true, scan_code: ScanCode::None, scan_code_str: "", key_code: KeyCode::Unknown, key_code_str: "", event_key_code: 0, vkey: "VK_HELP", us_user_settings_label: "", general_user_settings_label: "" },
    KeyCodeMapping { immutable: true, scan_code: ScanCode::None, scan_code_str: "", key_code: KeyCode::Unknown, key_code_str: "", event_key_code: 0, vkey: "VK_APPS", us_user_settings_label: "", general_user_settings_label: "" },
    KeyCodeMapping { immutable: true, scan_code: ScanCode::None, scan_code_str: "", key_code: KeyCode::Unknown, key_code_str: "", event_key_code: 0, vkey: "VK_PROCESSKEY", us_user_settings_label: "", general_user_settings_label: "" },
    KeyCodeMapping { immutable: true, scan_code: ScanCode::None, scan_code_str: "", key_code: KeyCode::Unknown, key_code_str: "", event_key_code: 0, vkey: "VK_PACKET", us_user_settings_label: "", general_user_settings_label: "" },
    KeyCodeMapping { immutable: true, scan_code: ScanCode::None, scan_code_str: "", key_code: KeyCode::Unknown, key_code_str: "", event_key_code: 0, vkey: "VK_DBE_SBCSCHAR", us_user_settings_label: "", general_user_settings_label: "" },
    KeyCodeMapping { immutable: true, scan_code: ScanCode::None, scan_code_str: "", key_code: KeyCode::Unknown, key_code_str: "", event_key_code: 0, vkey: "VK_DBE_DBCSCHAR", us_user_settings_label: "", general_user_settings_label: "" },
    KeyCodeMapping { immutable: true, scan_code: ScanCode::None, scan_code_str: "", key_code: KeyCode::Unknown, key_code_str: "", event_key_code: 0, vkey: "VK_ATTN", us_user_settings_label: "", general_user_settings_label: "" },
    KeyCodeMapping { immutable: true, scan_code: ScanCode::None, scan_code_str: "", key_code: KeyCode::Unknown, key_code_str: "", event_key_code: 0, vkey: "VK_CRSEL", us_user_settings_label: "", general_user_settings_label: "" },
    KeyCodeMapping { immutable: true, scan_code: ScanCode::None, scan_code_str: "", key_code: KeyCode::Unknown, key_code_str: "", event_key_code: 0, vkey: "VK_EXSEL", us_user_settings_label: "", general_user_settings_label: "" },
    KeyCodeMapping { immutable: true, scan_code: ScanCode::None, scan_code_str: "", key_code: KeyCode::Unknown, key_code_str: "", event_key_code: 0, vkey: "VK_EREOF", us_user_settings_label: "", general_user_settings_label: "" },
    KeyCodeMapping { immutable: true, scan_code: ScanCode::None, scan_code_str: "", key_code: KeyCode::Unknown, key_code_str: "", event_key_code: 0, vkey: "VK_PLAY", us_user_settings_label: "", general_user_settings_label: "" },
    KeyCodeMapping { immutable: true, scan_code: ScanCode::None, scan_code_str: "", key_code: KeyCode::Unknown, key_code_str: "", event_key_code: 0, vkey: "VK_ZOOM", us_user_settings_label: "", general_user_settings_label: "" },
    KeyCodeMapping { immutable: true, scan_code: ScanCode::None, scan_code_str: "", key_code: KeyCode::Unknown, key_code_str: "", event_key_code: 0, vkey: "VK_NONAME", us_user_settings_label: "", general_user_settings_label: "" },
    KeyCodeMapping { immutable: true, scan_code: ScanCode::None, scan_code_str: "", key_code: KeyCode::Unknown, key_code_str: "", event_key_code: 0, vkey: "VK_PA1", us_user_settings_label: "", general_user_settings_label: "" },
    KeyCodeMapping { immutable: true, scan_code: ScanCode::None, scan_code_str: "", key_code: KeyCode::Unknown, key_code_str: "", event_key_code: 0, vkey: "VK_OEM_CLEAR", us_user_settings_label: "", general_user_settings_label: "" },
];
