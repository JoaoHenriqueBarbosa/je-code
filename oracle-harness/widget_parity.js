// Lado do oráculo da verificação visual dos widgets (linhas H26 e H11 de docs/bench-report.md).
//
// O que faz: para cada "caso" (um widget, ou um grupo de estados do mesmo widget), injeta no workbench do VS Code
// real (oráculo, CDP) um overlay `position: fixed` com o DOM EXATO que o código TypeScript do widget cria em
// `ref/vscode/src/vs/base/browser/ui/<widget>/`, com os mesmos estilos inline que o método `style()` / `applyStyles()`
// aplicaria (as cores entram como `var(--vscode-...)`, exatamente como `defaultStyles.ts` faz com `asCssVariable`),
// de modo que o CSS REAL do VS Code, já carregado no workbench, pinta. Captura o overlay com `Page.captureScreenshot`
// (clip, escala 1) e grava um PNG por caso e tema, mais um `<caso>-<tema>.tsv` com os retângulos (em pixels) dos
// nós marcados `data-parity="<nome>"`: é com esses retângulos que `gallery_parity.rs` posiciona os widgets do
// je-code nas MESMAS coordenadas.
//
// Uso:
//   node oracle-harness/widget_parity.js <porta> <pasta_de_saida> <dark|light|both> [--modern=on|off] [caso ...]
//   node oracle-harness/widget_parity.js 9333 wip/scratch/parity both            (todos os casos, os dois temas)
//   node oracle-harness/widget_parity.js 9333 wip/scratch/parity dark button     (só o caso `button`)
//   node oracle-harness/widget_parity.js --list                                   (lista os casos)
//
// O oráculo vem de `oracle-harness/run-oracle.sh <segundos>` (porta 9333, 2026 Dark por padrão). A troca de tema
// escreve `workbench.colorTheme` em `wip/scratch/oracle-ud/User/settings.json` (o workbench aplica ao vivo) e
// espera a classe do tema mudar. Depois da execução o tema é devolvido ao que estava.
//
// `--modern=off` (padrão) desliga `workbench.experimental.modernUI` durante a captura: o 2026 Dark/Light do VS Code
// 1.143.1 liga a classe `modern-ui` no workbench, e as folhas `modernUI/browser/media/*.css` (padding, fontRamp,
// roundedCorners...) sobrescrevem o CSS base dos widgets. O porte lê o CSS BASE de `vs/base/browser/ui/**` (é o que
// os `.css` do widget especificam), então é contra ele que o padrão compara; `--modern=on` mantém o workbench como
// o VS Code entrega (use outra pasta de saída, e `gallery_parity --oracle <pasta>` para comparar).
//
// Comparação: `python3 -I oracle-harness/imgdiff.py <oraculo.png> <je.png> [diff.png]` (ou `parity_report.py`, que
// percorre a pasta inteira). Nada aqui depende de dado da máquina de quem roda.
'use strict';

const fs = require('fs');
const path = require('path');

/** Tudo que roda DENTRO da página do oráculo (serializado por `toString`): a biblioteca de DOM e os casos. */
function pageSide() {
	const workbench = document.querySelector('.monaco-workbench');
	const v = name => `var(--vscode-${name})`;

	// `dom.$` do VS Code: 'div.a.b#id' com atributos e filhos.
	function $(selector, attrs, ...children) {
		const m = /^([a-z0-9]*)((?:[.#][\w-]+)*)/i.exec(selector);
		const el = document.createElement(m[1] || 'div');
		for (const part of m[2].match(/[.#][\w-]+/g) || []) {
			if (part[0] === '#') { el.id = part.slice(1); } else { el.classList.add(part.slice(1)); }
		}
		for (const [k, val] of Object.entries(attrs || {})) {
			if (val !== undefined) { el.setAttribute(k, val); }
		}
		for (const c of children) { el.append(c); }
		return el;
	}
	const codicon = id => $(`span.codicon.codicon-${id}`);
	/** `renderLabelWithIcons`: `$(id)` vira um span codicon, o resto um texto. */
	function labelWithIcons(text) {
		const out = [];
		for (const part of text.split(/(\$\([\w-]+\))/)) {
			const m = /^\$\(([\w-]+)\)$/.exec(part);
			if (m) { out.push(codicon(m[1])); } else if (part !== '') { out.push(part); }
		}
		return out;
	}
	/** Um contêiner absoluto na posição do overlay. */
	function at(parent, x, y, width, child, name) {
		const box = $('div');
		box.style.cssText = `position:absolute;left:${x}px;top:${y}px;${width ? `width:${width}px;` : ''}`;
		if (name) { box.dataset.parityBox = name; }
		box.append(child);
		parent.append(box);
		return box;
	}
	/** Um texto solto (rótulo de legenda) numa posição. */
	function caption(parent, x, y, text) {
		const el = $('div');
		el.textContent = text;
		el.style.cssText = `position:absolute;left:${x}px;top:${y}px;font-size:10px;line-height:12px;white-space:nowrap;color:${v('descriptionForeground')};`;
		parent.append(el);
	}
	const mark = (el, name) => { el.dataset.parity = name; return el; };

	// ---------------------------------------------------------------- estilos padrão (defaultStyles.ts)
	const defaultButtonStyles = {
		buttonForeground: v('button-foreground'), buttonSeparator: v('button-separator'), buttonBackground: v('button-background'),
		buttonHoverBackground: v('button-hoverBackground'), buttonSecondaryForeground: v('button-secondaryForeground'),
		buttonSecondaryBackground: v('button-secondaryBackground'), buttonSecondaryHoverBackground: v('button-secondaryHoverBackground'),
		buttonSecondaryBorder: v('button-secondaryBorder'), buttonBorder: v('button-border'),
	};
	const defaultToggleStyles = {
		inputActiveOptionBorder: v('inputOption-activeBorder'), inputActiveOptionForeground: v('inputOption-activeForeground'),
		inputActiveOptionBackground: v('inputOption-activeBackground'),
	};
	const defaultCheckboxStyles = {
		checkboxBackground: v('checkbox-background'), checkboxBorder: v('checkbox-border'), checkboxForeground: v('checkbox-foreground'),
		checkboxDisabledBackground: v('checkbox-disabled-background'), checkboxDisabledForeground: v('checkbox-disabled-foreground'),
	};
	const defaultInputBoxStyles = {
		inputBackground: v('input-background'), inputForeground: v('input-foreground'), inputBorder: v('input-border'),
		info: { border: v('inputValidation-infoBorder'), background: v('inputValidation-infoBackground'), foreground: v('inputValidation-infoForeground') },
		warning: { border: v('inputValidation-warningBorder'), background: v('inputValidation-warningBackground'), foreground: v('inputValidation-warningForeground') },
		error: { border: v('inputValidation-errorBorder'), background: v('inputValidation-errorBackground'), foreground: v('inputValidation-errorForeground') },
	};
	const defaultCountBadgeStyles = { badgeBackground: v('badge-background'), badgeForeground: v('badge-foreground'), badgeBorder: v('contrastBorder') };
	const defaultKeybindingLabelStyles = {
		keybindingLabelBackground: v('keybindingLabel-background'), keybindingLabelForeground: v('keybindingLabel-foreground'),
		keybindingLabelBorder: v('keybindingLabel-border'), keybindingLabelBottomBorder: v('keybindingLabel-bottomBorder'), keybindingLabelShadow: v('widget-shadow'),
	};
	const defaultProgressBarStyles = { progressBarBackground: v('progressBar-background') };

	// ---------------------------------------------------------------- widgets (espelham o TypeScript)

	/** `Button` (button.ts): `a.monaco-button`, rótulo com ícones, `updateStyles(hover)`. */
	function Button(container, o = {}) {
		const styles = defaultButtonStyles;
		const el = $('a.monaco-button', { tabindex: '0', role: 'button' });
		el.classList.toggle('secondary', !!o.secondary);
		el.classList.toggle('small', !!o.small);
		const background = o.secondary ? styles.buttonSecondaryBackground : styles.buttonBackground;
		const foreground = o.secondary ? styles.buttonSecondaryForeground : styles.buttonForeground;
		const border = o.secondary ? styles.buttonSecondaryBorder : styles.buttonBorder;
		el.style.color = foreground || '';
		el.style.backgroundColor = background || '';
		if (border) { el.style.border = `1px solid ${border}`; }
		container.append(el);
		// set enabled(...)
		if (o.disabled) { el.classList.add('disabled'); el.setAttribute('aria-disabled', 'true'); } else { el.setAttribute('aria-disabled', 'false'); }
		// set label(...)
		el.classList.add('monaco-text-button');
		for (let seg of labelWithIcons(o.label)) {
			if (typeof seg === 'string') {
				seg = seg.trim();
				if (seg === '') { continue; }
				const node = $('span'); node.textContent = seg; el.append(node);
			} else { el.append(seg); }
		}
		if (o.hover && !o.disabled) {
			// updateStyles(true)
			const bg = o.secondary ? styles.buttonSecondaryHoverBackground : styles.buttonHoverBackground;
			el.style.backgroundColor = bg || '';
			el.style.color = foreground || '';
			el.style.border = border ? `1px solid ${border}` : '';
		}
		return el;
	}

	/** `Toggle` (toggle.ts), com o `applyStyles` do original. */
	function Toggle(o) {
		const classes = ['monaco-custom-toggle'];
		if (o.icon) { classes.push('codicon', `codicon-${o.icon}`); }
		if (o.actionClassName) { classes.push(...o.actionClassName.split(' ')); }
		if (o.isChecked) { classes.push('checked'); }
		const dom = $('div');
		dom.classList.add(...classes);
		dom.tabIndex = 0;
		dom.setAttribute('role', 'checkbox');
		dom.setAttribute('aria-checked', String(!!o.isChecked));
		const styles = o.styles;
		if (o.isChecked && styles.inputActiveOptionBorder) {
			dom.style.setProperty('--monaco-custom-toggle-border-color', styles.inputActiveOptionBorder);
		}
		dom.style.color = (o.isChecked && styles.inputActiveOptionForeground) || 'inherit';
		dom.style.backgroundColor = (o.isChecked && styles.inputActiveOptionBackground) || '';
		if (o.disabled) { dom.setAttribute('aria-disabled', 'true'); dom.classList.add('disabled'); dom.tabIndex = -1; }
		return dom;
	}

	/** `Checkbox` (toggle.ts): Toggle com `monaco-checkbox` e o `applyStyles` do `BaseCheckbox`. */
	function Checkbox(checked, disabled) {
		const styles = defaultCheckboxStyles;
		const unthemed = { inputActiveOptionBorder: '#007ACC00', inputActiveOptionForeground: '#FFFFFF', inputActiveOptionBackground: '#0E639C50' };
		const dom = Toggle({ icon: 'check', actionClassName: 'monaco-checkbox', isChecked: checked, styles: unthemed, disabled });
		if (!checked) { dom.classList.remove('codicon', 'codicon-check'); }
		const enabled = !disabled;
		dom.style.color = (enabled ? styles.checkboxForeground : styles.checkboxDisabledForeground) || '';
		dom.style.backgroundColor = (enabled ? styles.checkboxBackground : styles.checkboxDisabledBackground) || '';
		dom.style.borderColor = (enabled ? styles.checkboxBorder : styles.checkboxDisabledBackground) || '';
		const size = 18;
		dom.style.width = dom.style.height = dom.style.fontSize = `${size}px`;
		dom.style.fontSize = `${size - 2}px`;
		return dom;
	}

	/** `InputBox` (inputBox.ts): `.monaco-inputbox.idle > .ibwrapper > input.input`, com `applyStyles` e `showMessage`. */
	function InputBox(container, o = {}) {
		const styles = defaultInputBoxStyles;
		const element = $('.monaco-inputbox.idle');
		container.append(element);
		const wrapper = $('.ibwrapper');
		element.append(wrapper);
		const input = $('input.input.empty', { autocorrect: 'off', autocapitalize: 'off', spellcheck: 'false', wrap: 'off' });
		input.type = o.type || 'text';
		wrapper.append(input);
		if (o.placeholder) { input.setAttribute('placeholder', o.placeholder); }
		if (o.value) { input.value = o.value; input.classList.toggle('empty', !o.value); }
		if (o.disabled) { input.disabled = true; }
		element.style.backgroundColor = styles.inputBackground ?? '';
		element.style.color = styles.inputForeground ?? '';
		input.style.backgroundColor = 'inherit';
		input.style.color = styles.inputForeground ?? '';
		element.style.border = `1px solid ${styles.inputBorder || 'transparent'}`;
		if (o.focus) { element.classList.add('synthetic-focus'); }
		if (o.messageType) {
			const kind = o.messageType;
			element.classList.remove('idle');
			element.classList.add(kind);
			element.style.border = `1px solid ${styles[kind].border || 'transparent'}`;
		}
		return element;
	}

	/** O bloco da mensagem do `InputBox` (`_showMessage`), logo abaixo do campo, como o ContextView o posiciona. */
	function InputMessage(parent, anchor, kind, text) {
		const r = anchor.getBoundingClientRect();
		const container = $('.monaco-inputbox-container');
		container.style.width = `${r.width}px`;
		const span = $(`span.monaco-inputbox-message.${kind}`);
		span.textContent = text;
		const s = defaultInputBoxStyles[kind];
		span.style.backgroundColor = s.background ?? '';
		span.style.color = s.foreground ?? '';
		span.style.border = s.border ? `1px solid ${s.border}` : '';
		container.append(span);
		const box = $('div');
		box.style.cssText = `position:absolute;left:${r.left}px;top:${r.bottom}px;`;
		box.append(container);
		parent.append(box);
		return span;
	}

	/** `CountBadge` (countBadge.ts). */
	function CountBadge(count, long) {
		const el = $('.monaco-count-badge');
		if (long) { el.classList.add('long'); }
		el.textContent = String(count);
		el.style.backgroundColor = defaultCountBadgeStyles.badgeBackground ?? '';
		el.style.color = defaultCountBadgeStyles.badgeForeground ?? '';
		if (defaultCountBadgeStyles.badgeBorder) { el.style.border = `1px solid ${defaultCountBadgeStyles.badgeBorder}`; }
		return el;
	}

	/** `KeybindingLabel` (keybindingLabel.ts) com os rótulos de modificador do Linux/Windows/macOS. */
	const modifierLabels = {
		linux: { ctrl: 'Ctrl', shift: 'Shift', alt: 'Alt', meta: 'Super', sep: '+' },
		windows: { ctrl: 'Ctrl', shift: 'Shift', alt: 'Alt', meta: 'Windows', sep: '+' },
		mac: { ctrl: '⌃', shift: '⇧', alt: '⌥', meta: '⌘', sep: '' },
	};
	function KeybindingLabel(os, chords, matches, unbound) {
		const s = defaultKeybindingLabelStyles;
		const dom = $('.monaco-keybinding');
		if (s.keybindingLabelForeground) { dom.style.color = s.keybindingLabelForeground; }
		const key = (label, highlight) => {
			const k = $('span.monaco-keybinding-key' + (highlight ? '.highlight' : ''));
			k.textContent = label;
			if (s.keybindingLabelBackground) { k.style.backgroundColor = s.keybindingLabelBackground; }
			if (s.keybindingLabelBorder) { k.style.borderColor = s.keybindingLabelBorder; }
			if (s.keybindingLabelBottomBorder) { k.style.borderBottomColor = s.keybindingLabelBottomBorder; }
			if (s.keybindingLabelShadow) { k.style.boxShadow = `inset 0 -1px 0 ${s.keybindingLabelShadow}`; }
			return k;
		};
		const ml = modifierLabels[os];
		const renderKey = (label, highlight, sep) => {
			dom.append(key(label, highlight));
			if (sep) { const sp = $('span.monaco-keybinding-key-separator'); sp.textContent = sep; dom.append(sp); }
		};
		chords.forEach((c, i) => {
			if (i > 0) { const sp = $('span.monaco-keybinding-key-chord-separator'); sp.textContent = ' '; dom.append(sp); }
			const m = (i === 0 ? matches?.first : matches?.chord) || {};
			if (c.ctrl) { renderKey(ml.ctrl, !!m.ctrl, ml.sep); }
			if (c.shift) { renderKey(ml.shift, !!m.shift, ml.sep); }
			if (c.alt) { renderKey(ml.alt, !!m.alt, ml.sep); }
			if (c.meta) { renderKey(ml.meta, !!m.meta, ml.sep); }
			if (c.key) { renderKey(c.key, !!m.key, ''); }
		});
		if (!chords.length && unbound) { dom.append(key('Unbound', false)); }
		return dom;
	}

	/** `ProgressBar` (progressbar.ts) já criado: `worked` em porcentagem, ou parado (sem `active`). */
	function ProgressBar(container, worked) {
		const element = $('div.monaco-progress-container', { role: 'progressbar', 'aria-valuemin': '0' });
		container.append(element);
		const bit = $('div.progress-bit');
		bit.style.backgroundColor = defaultProgressBarStyles.progressBarBackground || '#0E70C0';
		element.append(bit);
		if (worked !== undefined) {
			// doSetWorked: `total(100)` e `discrete`/`active`.
			element.classList.add('active', 'discrete');
			bit.style.width = `${worked}%`;
		}
		return element;
	}

	/** `HighlightedLabel` (highlightedLabel.ts). `highlights` em unidades UTF-16 como o original. */
	function HighlightedLabel(container, text, highlights = [], supportIcons = false) {
		const dom = $('span.monaco-highlighted-label');
		container.append(dom);
		const children = [];
		let pos = 0;
		const parts = s => supportIcons ? labelWithIcons(s) : [s];
		for (const h of highlights) {
			if (h.end === h.start) { continue; }
			if (pos < h.start) { children.push(...parts(text.substring(pos, h.start))); pos = h.start; }
			const element = $('span.highlight');
			element.append(...parts(text.substring(pos, h.end)));
			children.push(element);
			pos = h.end;
		}
		if (pos < text.length) { children.push(...parts(text.substring(pos))); }
		dom.replaceChildren(...children);
		return dom;
	}

	/** `IconLabel` (iconLabel.ts) com realces e descrição. */
	function IconLabel(container, o) {
		const classes = ['monaco-icon-label'];
		if (o.bold) { classes.push('bold'); }
		if (o.italic) { classes.push('italic'); }
		if (o.strikethrough) { classes.push('strikethrough'); }
		const root = $('.monaco-icon-label');
		container.append(root);
		const labelContainer = $('.monaco-icon-label-container');
		root.append(labelContainer);
		const nameContainer = $('span.monaco-icon-name-container');
		labelContainer.append(nameContainer);
		if (o.iconPath) {
			const iconNode = $('.monaco-icon-label-iconpath');
			iconNode.className = `monaco-icon-label-iconpath codicon codicon-${o.iconPath}`;
			iconNode.style.backgroundRepeat = 'no-repeat';
			iconNode.style.backgroundPosition = 'center';
			iconNode.style.backgroundSize = 'contain';
			root.prepend(iconNode);
		}
		root.className = classes.join(' ');
		labelContainer.className = 'monaco-icon-label-container' + (o.disabledCommand ? ' disabled' : '');
		const name = $('a.label-name');
		nameContainer.append(name);
		HighlightedLabel(name, o.label, o.matches || [], !!o.supportIcons);
		if (o.suffix) {
			const suffixContainer = $('span.monaco-icon-suffix-container');
			nameContainer.after(suffixContainer);
			const sfx = $('span.label-suffix'); sfx.textContent = o.suffix; suffixContainer.append(sfx);
		}
		if (o.description) {
			const dc = $('span.monaco-icon-description-container');
			labelContainer.append(dc);
			const d = $('span.label-description');
			dc.append(d);
			HighlightedLabel(d, o.description, o.descriptionMatches || [], !!o.supportIcons);
		}
		return root;
	}


	/** `ActionBar` (actionbar.ts) com `ActionViewItem`s: `ul.actions-container > li.action-item > a.action-label`. */
	function ActionBar(container, items, o = {}) {
		const domNode = $('div.monaco-action-bar');
		if (o.vertical) { domNode.classList.add('vertical'); }
		const list = $('ul.actions-container', { role: 'toolbar' });
		if (o.highlightToggledItems) { list.classList.add('highlight-toggled'); }
		domNode.append(list);
		const elements = [];
		for (const it of items) {
			const li = $('li.action-item', { role: 'presentation' });
			const label = $('a.action-label');
			li.append(label);
			if (it.separator) {
				label.classList.add('separator');
			} else {
				if (o.icon) { label.classList.add('codicon', `codicon-${it.icon}`); }
				if (o.label) { label.textContent = it.label; }
				if (it.keybinding && o.label) {
					const kb = $('span.keybinding'); kb.textContent = it.keybinding; li.append(kb);
				}
				if (it.disabled) { label.classList.add('disabled'); li.classList.add('disabled'); }
				if (it.checked !== undefined) { label.classList.toggle('checked', it.checked); }
			}
			if (it.hover) { label.classList.add('parity-hover'); }
			if (o.name) { mark(label, `${o.name}-${elements.length}`); }
			list.append(li);
			elements.push({ li, label });
		}
		container.append(domNode);
		return { domNode, list, elements };
	}

	/** `FindInput` (findInput.ts): a caixa de texto e os `.controls` com os toggles de caixa, palavra e regex. */
	function FindInput(container, o = {}) {
		const domNode = $('div.monaco-findInput');
		const box = InputBox(domNode, { placeholder: o.placeholder ?? 'Find', value: o.value, focus: o.focus, messageType: o.messageType });
		const controls = $('div.controls');
		controls.style.display = o.common === false ? 'none' : '';
		const toggles = [];
		const mk = (icon, checked) => {
			const t = Toggle({ icon, isChecked: checked, styles: defaultToggleStyles });
			controls.append(t);
			toggles.push(t);
			return t;
		};
		if (o.common !== false) {
			// A ordem do DOM é a do original: caixa, palavra inteira, regex (`controls.append` na sequência do construtor).
			mk('case-sensitive', !!o.case);
			mk('whole-word', !!o.words);
			mk('regex', !!o.regex);
		}
		for (const icon of o.extra || []) { mk(icon, true); }
		domNode.append(controls);
		// `updateInputBoxPadding`: `paddingRight` = soma de `width()` (22 px cada toggle).
		const input = box.querySelector('input');
		input.style.width = `calc(100% - ${toggles.length * 22}px)`;
		if (o.disabled) { domNode.classList.add('disabled'); input.disabled = true; }
		container.append(domNode);
		return { domNode, box, controls, toggles };
	}


	/** `BreadcrumbsWidget` (breadcrumbsWidget.ts) com itens de `IconLabel` simples, como os do editor. */
	function Breadcrumbs(container, names, o) {
		const scroll = $('div.monaco-scrollable-element', { role: 'presentation' });
		scroll.style.cssText = 'position: relative; overflow: hidden;';
		const dom = $('div.monaco-breadcrumbs', { tabindex: '0', role: 'list' });
		dom.style.cssText = `overflow: hidden; width: ${o.width}px; height: ${o.height}px;`;
		scroll.append(dom);
		const style = document.createElement('style');
		style.textContent = [
			`.monaco-breadcrumbs { background-color: ${v('breadcrumb-background')}}`,
			`.monaco-breadcrumbs .monaco-breadcrumb-item { color: ${v('breadcrumb-foreground')}}`,
			`.monaco-breadcrumbs .monaco-breadcrumb-item.focused { color: ${v('breadcrumb-focusForeground')}}`,
			`.monaco-breadcrumbs .monaco-breadcrumb-item.focused.selected { color: ${v('breadcrumb-activeSelectionForeground')}}`,
			`.monaco-breadcrumbs:not(.disabled) .monaco-breadcrumb-item:hover:not(.focused):not(.selected) { color: ${v('breadcrumb-focusForeground')}}`,
		].join('\n') + '\n';
		dom.append(style);
		const items = [];
		names.forEach((name, i) => {
			const item = $('div');
			const label = $('.monaco-icon-label');
			const lc = $('.monaco-icon-label-container');
			const nc = $('span.monaco-icon-name-container');
			const a = $('a.label-name'); a.textContent = name;
			nc.append(a); lc.append(nc); label.append(lc); item.append(label);
			item.tabIndex = -1;
			item.setAttribute('role', 'listitem');
			item.classList.add('monaco-breadcrumb-item');
			item.append($('div.codicon.codicon-breadcrumb-separator'));
			if (o.selected === i) { item.classList.add('selected'); }
			if (o.focused === i) { item.classList.add('focused'); }
			if (o.hover === i) { item.classList.add('parity-hover'); }
			dom.append(item);
			items.push(item);
		});
		container.append(scroll);
		return { scroll, dom, items };
	}


	/** `SelectBox` fechada (selectBoxCustom.ts, `render`): `div.select-container > select.monaco-select-box`. */
	function SelectBox(container, options, selected, o = {}) {
		const select = $('select.monaco-select-box');
		for (const opt of options) {
			const option = document.createElement('option');
			option.textContent = opt.text;
			option.disabled = !!opt.disabled;
			select.append(option);
		}
		select.selectedIndex = selected;
		container.classList.add('select-container');
		container.append(select);
		select.style.backgroundColor = v('dropdown-background');
		select.style.color = v('dropdown-foreground');
		select.style.borderColor = v('dropdown-border');
		if (o.disabled) { select.disabled = true; }
		return select;
	}

	// ---------------------------------------------------------------- casos
	const cases = {};
	const addCase = (name, w, h, bg, build) => { cases[name] = { w, h, bg, build }; };
	const cell = (root, x, y, w, node, name) => at(root, x, y, w, mark(node, name), undefined);

	addCase('button', 520, 112, 'editor-background', root => {
		const b = (x, y, w, name, o) => { const host = $('div'); Button(host, o); at(root, x, y, w, mark(host, name)); };
		b(12, 12, 90, 'primary', { label: 'Primary' });
		b(112, 12, 90, 'secondary', { label: 'Secondary', secondary: true });
		b(212, 12, 60, 'small', { label: 'Small', small: true });
		b(282, 12, 110, 'icon', { label: '$(add) With icon' });
		b(402, 12, 80, 'disabled', { label: 'Disabled', disabled: true });
		b(12, 62, 90, 'hover', { label: 'Hover', hover: true });
		b(112, 62, 90, 'secondary-hover', { label: 'Hover', secondary: true, hover: true });
		b(212, 62, 90, 'disabled-secondary', { label: 'Disabled', secondary: true, disabled: true });
		b(312, 62, 100, 'small-icon', { label: '$(check) Small', small: true });
	});

	addCase('toggle', 420, 92, 'editor-background', root => {
		let x = 12;
		for (const [name, checked, disabled] of [['off', false, false], ['on', true, false], ['disabled-on', true, true], ['disabled-off', false, true]]) {
			const t = Toggle({ icon: 'case-sensitive', isChecked: checked, styles: defaultToggleStyles, disabled });
			at(root, x, 12, 0, mark(t, 'toggle-' + name));
			x += 40;
		}
		x = 12;
		for (const [name, checked, disabled] of [['unchecked', false, false], ['checked', true, false], ['disabled-checked', true, true], ['disabled-unchecked', false, true]]) {
			const c = Checkbox(checked, disabled);
			at(root, x, 52, 0, mark(c, 'checkbox-' + name));
			x += 40;
		}
		// Toggle sob o ponteiro (:hover do CSS real).
		const hov = Toggle({ icon: 'regex', isChecked: false, styles: defaultToggleStyles });
		at(root, 200, 12, 0, mark(hov, 'toggle-hover'));
		const foc = Toggle({ icon: 'whole-word', isChecked: true, styles: defaultToggleStyles });
		at(root, 240, 12, 0, mark(foc, 'toggle-on-b'));
		return { hover: '[data-parity="toggle-hover"]' };
	});

	addCase('inputbox', 280, 232, 'editor-background', root => {
		let y = 8;
		const row = (name, o) => {
			const host = $('div');
			const box = InputBox(host, o);
			mark(box, name + '-box');
			mark(box.querySelector('input'), name + '-input');
			at(root, 12, y, 250, mark(host, name));
			y += 34;
			return box;
		};
		row('placeholder', { placeholder: 'Search' });
		row('text', { value: 'src/vs/base/browser/ui' });
		row('focus', { value: 'typing here', focus: true });
		row('info', { value: 'info', messageType: 'info' });
		row('warning', { value: 'warning', messageType: 'warning' });
		const err = row('error', { value: '**/*.{rs,toml', messageType: 'error' });
		InputMessage(root, err, 'error', 'The value must be a valid pattern.');
	});

	addCase('countbadge', 220, 40, 'editor-background', root => {
		const host = $('div');
		host.style.cssText = 'display:flex;gap:10px;align-items:flex-start;';
		for (const [name, count, long] of [['0', 0, false], ['7', 7, false], ['42', 42, false], ['1234', 1234, false], ['long', 3, true]]) {
			host.append(mark(CountBadge(count, long), 'badge-' + name));
		}
		at(root, 12, 10, 0, host);
	});

	addCase('keybinding', 300, 180, 'editor-background', root => {
		const rows = [
			['linux-one', 'linux', [{ ctrl: true, shift: true, key: 'P' }], undefined],
			['linux-chord', 'linux', [{ ctrl: true, key: 'K' }, { ctrl: true, key: 'S' }], undefined],
			['mac', 'mac', [{ shift: true, meta: true, key: 'P' }], undefined],
			['windows-match', 'windows', [{ ctrl: true, alt: true, key: 'Delete' }], { first: { ctrl: true, key: true } }],
			['unbound', 'linux', [], undefined],
		];
		rows.forEach(([name, os, chords, matches], i) => {
			const host = $('div');
			host.style.cssText = 'display:flex;';
			host.append(mark(KeybindingLabel(os, chords, matches, true), 'kb-' + name));
			at(root, 12, 10 + 32 * i, 0, host);
		});
	});

	addCase('progressbar', 240, 60, 'editor-background', root => {
		const mk = (y, worked, name) => { const host = $('div'); ProgressBar(host, worked); at(root, 12, y, 200, mark(host, name)); };
		mk(10, undefined, 'idle');
		mk(22, 50, 'half');
		mk(34, 100, 'full');
	});

	addCase('labels', 420, 150, 'editor-background', root => {
		// O realce colorido (`.highlight`) e a altura de linha de 22 px dos rótulos vêm de uma linha de lista:
		// `.monaco-workbench .monaco-list .monaco-list-row .monaco-highlighted-label .highlight`. O contêiner
		// de cada caso é uma linha de lista de verdade (`.monaco-list > .monaco-list-row`), sem a posição absoluta.
		// A altura da linha do texto herda do contêiner (no VS Code, da linha da lista); o je-code assume a natural
		// (`normal`) no HighlightedLabel e 22 px no IconLabel, então cada linha fixa esse `line-height`.
		const row = (y, name, build, lineHeight = '22px') => {
			const list = $('.monaco-list');
			list.style.cssText = 'height:auto;width:100%;';
			const li = $('.monaco-list-row');
			li.style.cssText = `position:relative;height:auto;width:100%;display:flex;line-height:${lineHeight};`;
			list.append(li);
			const label = build(li);
			mark(li, name + '-row');
			if (label) { mark(label, name); }
			at(root, 12, y, 396, list);
		};
		row(8, 'highlighted', h => {
			const first = HighlightedLabel(h, 'Hello World', [{ start: 0, end: 2 }, { start: 6, end: 9 }]);
			const s = $('span'); s.style.width = '20px'; h.append(s);
			mark(HighlightedLabel(h, '$(symbol-class) MyClass', [{ start: 16, end: 18 }], true), 'highlighted-icon');
			return first;
		}, 'normal');
		row(34, 'iconlabel', h => IconLabel(h, { label: 'filename', matches: [{ start: 0, end: 4 }], suffix: '.ts', description: 'src/description', descriptionMatches: [{ start: 0, end: 3 }], supportIcons: true }));
		row(60, 'iconlabel-italic', h => IconLabel(h, { label: 'filename', italic: true, description: 'description' }));
		row(86, 'iconlabel-strike', h => IconLabel(h, { label: 'filename', strikethrough: true, description: 'description' }));
		row(112, 'iconlabel-icon', h => IconLabel(h, { label: '$(sync) Sincronizar', description: 'agora', iconPath: 'symbol-method', supportIcons: true }));
	});


	addCase('actionbar', 360, 200, 'sideBar-background', root => {
		const host = (name, x, y, w, h, build) => {
			const box = $('div');
			box.style.cssText = `height:${h}px;`;
			const r = build(box);
			mark(box, name);
			at(root, x, y, w, box);
			return r;
		};
		const icons = [
			{ icon: 'add', label: 'Adicionar' }, { icon: 'refresh', label: 'Atualizar', hover: true }, { separator: true },
			{ icon: 'trash', label: 'Excluir', disabled: true }, { icon: 'edit', label: 'Editar', checked: true }, { icon: 'gear', label: 'Configurar' },
		];
		host('bar-icons', 12, 12, 330, 28, h => ActionBar(h, icons, { name: 'bar-icons-item', icon: true, highlightToggledItems: true }));
		host('bar-text', 12, 56, 330, 28, h => ActionBar(h, [
			{ label: 'Executar', keybinding: 'Ctrl+R' }, { label: 'Depurar', keybinding: 'F5' }, { label: 'Parar', disabled: true }, { label: 'Fixar', checked: true },
		], { name: 'bar-text-item', label: true, highlightToggledItems: true }));
		host('bar-vertical', 12, 100, 140, 90, h => ActionBar(h, [
			{ label: 'Novo arquivo' }, { label: 'Abrir...' }, { separator: true }, { label: 'Fixado', checked: true }, { label: 'Sair', disabled: true },
		], { name: 'bar-vertical-item', label: true, vertical: true, highlightToggledItems: true }));
		for (const el of root.querySelectorAll('.action-label')) {
			const r = el.getBoundingClientRect();
			void r;
		}
		return { hover: '.parity-hover' };
	});

	addCase('findinput', 300, 330, 'sideBar-background', root => {
		let y = 8;
		const row = (name, o) => {
			const host = $('div');
			const f = FindInput(host, o);
			mark(f.domNode, name + '-find');
			mark(f.box, name + '-box');
			at(root, 12, y, 270, mark(host, name));
			y += 34;
			return f;
		};
		row('empty', {});
		row('text', { value: 'src/vs/base/browser' });
		row('case', { value: 'TextModel', case: true });
		row('words-regex', { value: '^import .* from', words: true, regex: true });
		row('three', { value: '(foo|bar)+', case: true, words: true, regex: true });
		row('focus', { value: 'typing here', focus: true, regex: true });
		row('plain', { value: 'plain input', common: false });
		row('extra', { value: 'with an extra toggle', extra: ['regex'] });
		const err = row('error', { value: '(unclosed', regex: true, messageType: 'error' });
		InputMessage(root, err.box, 'error', 'Invalid regular expression: missing )');
	});


	addCase('breadcrumbs', 460, 70, 'editor-background', root => {
		const names = ['home', 'john', 'projects', 'je-code', 'sample.ts', 'render'];
		const host = $('div');
		host.style.cssText = 'height:22px;';
		const b = Breadcrumbs(host, names, { width: 436, height: 22, selected: 1, focused: 2, hover: 4 });
		mark(host, 'bar');
		names.forEach((_, i) => mark(b.items[i], `item-${i}`));
		at(root, 12, 8, 436, host);
		const plain = $('div');
		plain.style.cssText = 'height:22px;';
		const p = Breadcrumbs(plain, names.slice(0, 4), { width: 436, height: 22 });
		mark(plain, 'plain');
		p.items.forEach((it, i) => mark(it, `plain-item-${i}`));
		at(root, 12, 40, 436, plain);
		return { hover: '.parity-hover' };
	});


	addCase('selectbox', 460, 90, 'editor-background', root => {
		const fruits = [{ text: 'Apple' }, { text: 'Banana' }, { text: 'Cherry' }, { text: 'Date' }];
		const place = (name, x, y, w, options, selected, o) => {
			// `selectBox.css` dá `min-width`, `min-height` e `padding` ao select dentro de uma barra de ações
			// (`.monaco-action-bar .action-item .monaco-select-box`), o contexto que o je-code assume.
			const bar = $('div.monaco-action-bar');
			const item = $('div.action-item');
			const host = $('div');
			host.style.display = 'flex';
			const select = SelectBox(host, options, selected, o);
			mark(select, name + '-select');
			item.append(mark(host, name));
			bar.append(item);
			at(root, x, y, w, bar);
			return select;
		};
		place('normal', 12, 12, 200, fruits, 1);
		const focused = place('focus', 232, 12, 200, fruits, 0);
		place('disabled', 12, 46, 200, fruits, 0, { disabled: true });
		place('cut', 232, 46, 150, [{ text: 'Uma opção com um texto bem comprido que não cabe' }, { text: 'Curta' }], 0);
		focused.focus();
	});


	addCase('hover', 560, 170, 'editor-background', root => {
		/** `WorkbenchHoverWidget` (platform/hover/browser/hoverWidget.ts) com conteúdo de texto, sem ponteiro. */
		const hover = (name, x, y, text, compact) => {
			const container = $('div.monaco-hover.workbench-hover');
			if (compact) { container.classList.add('compact'); }
			container.tabIndex = 0;
			container.setAttribute('role', 'tooltip');
			const scroll = $('div.monaco-scrollable-element', { role: 'presentation' });
			scroll.style.cssText = 'position: relative; overflow: hidden;';
			const contents = $('div.monaco-hover-content');
			contents.style.overflow = 'hidden';
			const row = $('div.hover-row.markdown-hover');
			const hc = $('div.hover-contents');
			hc.textContent = text;
			hc.style.whiteSpace = 'pre-wrap';
			row.append(hc); contents.append(row); scroll.append(contents); container.append(scroll);
			at(root, x, y, 0, mark(container, name));
		};
		hover('text', 12, 8, 'Dica de texto.\nSegunda linha.', false);
		hover('compact', 12, 130, 'Dica compacta', true);
		hover('long', 12, 60, 'Um texto de dica comprido o bastante para quebrar em mais de uma linha quando passa da largura máxima do conteúdo, que é de quinhentos pixels, e depois continua por mais uma frase.', false);
	});

	// ---------------------------------------------------------------- montagem
	let overlay;
	function mount(name) {
		const c = cases[name];
		overlay?.remove();
		overlay = $('div');
		overlay.id = 'parity-overlay';
		overlay.style.cssText = `position:fixed;left:0;top:0;width:${c.w}px;height:${c.h}px;z-index:99999;overflow:hidden;background:${v(c.bg)};color:${v('foreground')};`;
		workbench.append(overlay);
		const extra = c.build(overlay) || {};
		return { w: c.w, h: c.h, hover: extra.hover };
	}
	function rects() {
		const out = {};
		for (const el of overlay.querySelectorAll('[data-parity]')) {
			const r = el.getBoundingClientRect();
			out[el.dataset.parity] = { x: r.left, y: r.top, w: r.width, h: r.height };
		}
		return out;
	}
	function center(selector) {
		const r = overlay.querySelector(selector).getBoundingClientRect();
		return { x: r.left + r.width / 2, y: r.top + r.height / 2 };
	}
	function unmount() { overlay?.remove(); overlay = undefined; }
	function themeClass() { return workbench.className.match(/2026-(dark|light)/)?.[1]; }
	window.__parity = { names: () => Object.keys(cases), mount, rects, center, unmount, themeClass };
	return Object.keys(cases);
}

// ------------------------------------------------------------------------------------------------ lado do Node
const THEME_IDS = { dark: 'Dark 2026', light: 'Light 2026' };

async function connect(port) {
	const targets = await (await fetch(`http://127.0.0.1:${port}/json/list`)).json();
	const pages = targets.filter(t => t.type === 'page' && /workbench/.test(t.url));
	if (!pages.length) { throw new Error('nenhuma página do workbench no oráculo'); }
	const page = pages.find(t => /sample\.ts/.test(t.title)) ?? pages[0];
	const ws = new WebSocket(page.webSocketDebuggerUrl);
	await new Promise((resolve, reject) => { ws.onopen = resolve; ws.onerror = reject; });
	let id = 0;
	const call = (method, params = {}) => new Promise((resolve, reject) => {
		const my = ++id;
		const onMessage = ev => {
			const msg = JSON.parse(ev.data);
			if (msg.id === my) { ws.removeEventListener('message', onMessage); msg.error ? reject(new Error(`${method}: ${msg.error.message}`)) : resolve(msg.result); }
		};
		ws.addEventListener('message', onMessage);
		ws.send(JSON.stringify({ id: my, method, params }));
	});
	const evaluate = async expression => {
		const r = await call('Runtime.evaluate', { expression, returnByValue: true, awaitPromise: true });
		if (r.exceptionDetails) { throw new Error(`avaliação falhou: ${r.exceptionDetails.exception?.description ?? r.exceptionDetails.text}`); }
		return r.result.value;
	};
	return { call, evaluate, close: () => ws.close() };
}

const sleep = ms => new Promise(r => setTimeout(r, ms));

function settingsPath() {
	return path.join(__dirname, '..', 'wip', 'scratch', 'oracle-ud', 'User', 'settings.json');
}

async function setTheme(page, theme) {
	if (await page.evaluate('window.__parity.themeClass()') === theme) { return; }
	const file = settingsPath();
	const settings = JSON.parse(fs.readFileSync(file, 'utf8'));
	settings['workbench.colorTheme'] = THEME_IDS[theme];
	fs.writeFileSync(file, JSON.stringify(settings, null, 2) + '\n');
	for (let i = 0; i < 60; i++) {
		await sleep(250);
		if (await page.evaluate('window.__parity.themeClass()') === theme) { await sleep(500); return; }
	}
	throw new Error(`o oráculo não trocou para ${theme}`);
}

/** Liga ou desliga `workbench.experimental.modernUI` e espera a classe `modern-ui` seguir. */
async function setModern(page, on) {
	const has = () => page.evaluate("document.querySelector('.monaco-workbench').classList.contains('modern-ui')");
	if (await has() === on) { return; }
	const file = settingsPath();
	const settings = JSON.parse(fs.readFileSync(file, 'utf8'));
	settings['workbench.experimental.modernUI'] = on;
	fs.writeFileSync(file, JSON.stringify(settings, null, 2) + '\n');
	for (let i = 0; i < 60; i++) {
		await sleep(250);
		if (await has() === on) { await sleep(800); return; }
	}
	throw new Error(`o oráculo não trocou modernUI para ${on}`);
}

async function captureCase(page, name, theme, outDir) {
	const meta = await page.evaluate(`window.__parity.mount(${JSON.stringify(name)})`);
	await page.evaluate('document.fonts.ready.then(() => new Promise(r => requestAnimationFrame(() => requestAnimationFrame(r))))');
	// O ponteiro de verdade não pinta :hover de forma confiável sob xvfb (o estilo computado muda, o quadro não);
	// `CSS.forcePseudoState` força o `:hover` nos alvos, de modo determinístico.
	if (meta.hover) {
		await page.call('DOM.enable');
		await page.call('CSS.enable');
		const doc = await page.call('DOM.getDocument', { depth: 0 });
		const { nodeIds } = await page.call('DOM.querySelectorAll', { nodeId: doc.root.nodeId, selector: `#parity-overlay ${meta.hover}` });
		for (const nodeId of nodeIds) { await page.call('CSS.forcePseudoState', { nodeId, forcedPseudoClasses: ['hover'] }); }
		await sleep(300);
	}
	await sleep(200);
	const rects = await page.evaluate('window.__parity.rects()');
	const shot = await page.call('Page.captureScreenshot', { format: 'png', clip: { x: 0, y: 0, width: meta.w, height: meta.h, scale: 1 }, fromSurface: true, captureBeyondViewport: false });
	fs.writeFileSync(path.join(outDir, `${name}-${theme}.png`), Buffer.from(shot.data, 'base64'));
	// TSV: `#size  W  H` e uma linha `nome  x  y  w  h` por nó marcado (lido por `gallery_parity.rs` e `parity_report.py`).
	const lines = [`#size\t${meta.w}\t${meta.h}`, ...Object.entries(rects).map(([k, r]) => `${k}\t${r.x}\t${r.y}\t${r.w}\t${r.h}`)];
	fs.writeFileSync(path.join(outDir, `${name}-${theme}.tsv`), lines.join('\n') + '\n');
	await page.evaluate('window.__parity.unmount()');
	process.stderr.write(`${name}-${theme}: ${meta.w}x${meta.h}\n`);
}


/** Casos da UI REAL do workbench (sem overlay): a barra de menus do título e o menu Arquivo aberto por clique. */
const LIVE_CASES = {
	async menubar(page, outDir, theme) {
		await page.evaluate("document.activeElement?.blur()");
		const buttons = await page.evaluate(`JSON.stringify([...document.querySelectorAll('.menubar .menubar-menu-button')].filter(b => b.textContent).map(b => { const r = b.getBoundingClientRect(); return [b.textContent, r.x, r.y, r.width, r.height]; }))`);
		const bar = await page.evaluate(`JSON.stringify(document.querySelector('.menubar').getBoundingClientRect())`);
		const b = JSON.parse(bar);
		const rects = { bar: [b.x, b.y, b.width, b.height] };
		JSON.parse(buttons).forEach(([text, x, y, w, h]) => { rects[`button-${text}`] = [x, y, w, h]; });
		const clip = { x: 0, y: 0, width: Math.ceil(b.x + b.width + 8), height: Math.ceil(b.height) };
		return { clip, rects, extra: undefined };
	},
	async menu(page, outDir, theme) {
		const btn = JSON.parse(await page.evaluate(`JSON.stringify(document.querySelector('.menubar .menubar-menu-button').getBoundingClientRect())`));
		for (const type of ['mouseMoved', 'mousePressed', 'mouseReleased']) {
			await page.call('Input.dispatchMouseEvent', { type, x: btn.x + btn.width / 2, y: btn.y + btn.height / 2, button: 'left', clickCount: 1 });
		}
		await sleep(900);
		// Para o ponteiro ficar fora do menu (sem :hover) e o item de foco sem mudar.
		await page.call('Input.dispatchMouseEvent', { type: 'mouseMoved', x: 1190, y: 790 });
		await sleep(300);
		const info = JSON.parse(await page.evaluate(`(() => {
			const holder = document.querySelector('.menubar-menu-items-holder');
			const h = holder.getBoundingClientRect();
			const items = [...holder.querySelectorAll('li.action-item')].map(li => {
				const r = li.getBoundingClientRect();
				const a = li.querySelector('.action-label');
				const sep = a.classList.contains('separator');
				const kb = li.querySelector('.keybinding');
				const check = li.querySelector('.menu-item-check');
				const mi = li.querySelector('.action-menu-item'); const checkable = mi && mi.getAttribute('role') === 'menuitemcheckbox' ? (mi.getAttribute('aria-checked') === 'true' ? '1' : '0') : ''; return { kind: sep ? 'separator' : li.querySelector('.monaco-submenu-item') ? 'submenu' : 'item', label: sep ? '' : li.querySelector('.action-label').textContent, keybinding: kb && !kb.hidden ? kb.textContent : '', disabled: li.classList.contains('disabled') && !sep, checked: checkable, rect: [r.x, r.y, r.width, r.height] };
			});
			return JSON.stringify({ holder: [h.x, h.y, h.width, h.height], items, focused: [...holder.querySelectorAll('li.action-item')].findIndex(li => li.classList.contains('focused')) });
		})()`));
		const rects = { holder: info.holder };
		info.items.forEach((it, i) => { rects[`item-${i}`] = it.rect; });
		const clip = { x: Math.floor(info.holder[0]) - 4, y: 0, width: Math.ceil(info.holder[2]) + 40, height: Math.ceil(info.holder[1] + info.holder[3]) + 12 };
		const extra = info.items.map((it, i) => [i, it.kind, it.label, it.keybinding, it.disabled ? 1 : 0, it.checked].join('\t')).join('\n') + '\n';
		return { clip, rects, extra, after: async () => { await page.call('Input.dispatchKeyEvent', { type: 'keyDown', key: 'Escape', code: 'Escape', windowsVirtualKeyCode: 27 }); await page.call('Input.dispatchKeyEvent', { type: 'keyUp', key: 'Escape', code: 'Escape', windowsVirtualKeyCode: 27 }); await sleep(300); } };
	},
};

async function captureLive(page, name, theme, outDir) {
	const r = await LIVE_CASES[name](page, outDir, theme);
	const shot = await page.call('Page.captureScreenshot', { format: 'png', clip: { ...r.clip, scale: 1 }, fromSurface: true });
	fs.writeFileSync(path.join(outDir, `${name}-${theme}.png`), Buffer.from(shot.data, 'base64'));
	const lines = [`#size\t${r.clip.width}\t${r.clip.height}`, `#origin\t${r.clip.x}\t${r.clip.y}`, ...Object.entries(r.rects).map(([k, [x, y, w, h]]) => `${k}\t${x - r.clip.x}\t${y - r.clip.y}\t${w}\t${h}`)];
	fs.writeFileSync(path.join(outDir, `${name}-${theme}.tsv`), lines.join('\n') + '\n');
	if (r.extra !== undefined) { fs.writeFileSync(path.join(outDir, `${name}-${theme}.items.tsv`), r.extra); }
	if (r.after) { await r.after(); }
	process.stderr.write(`${name}-${theme}: ${r.clip.width}x${r.clip.height} (UI real)\n`);
}

async function main() {
	const args = process.argv.slice(2);
	if (args[0] === '--list') {
		console.log([...pageSide.toString().match(/addCase\('([\w-]+)'/g).map(s => s.slice(9, -1)), ...Object.keys(LIVE_CASES)].join('\n'));
		return;
	}
	const modernArg = args.find(a => a.startsWith('--modern='));
	const [port, outDirArg, themeArg, ...only] = args.filter(a => a !== modernArg);
	const modern = modernArg === '--modern=on';
	if (!port || !outDirArg || !themeArg) { throw new Error('uso: widget_parity.js <porta> <pasta> <dark|light|both> [caso ...]'); }
	const outDir = path.resolve(outDirArg);
	fs.mkdirSync(outDir, { recursive: true });
	const page = await connect(port);
	await page.call('Page.bringToFront');
	await page.call('Emulation.setFocusEmulationEnabled', { enabled: true });
	await page.evaluate(`(${pageSide.toString()})()`);
	const names = (only.length ? only : [...await page.evaluate('window.__parity.names()'), ...Object.keys(LIVE_CASES)]);
	const original = await page.evaluate('window.__parity.themeClass()');
	const originalModern = await page.evaluate("document.querySelector('.monaco-workbench').classList.contains('modern-ui')");
	const themes = themeArg === 'both' ? ['dark', 'light'] : [themeArg];
	try {
		await setModern(page, modern);
		for (const theme of themes) {
			await setTheme(page, theme);
			for (const name of names) {
				if (LIVE_CASES[name]) { await captureLive(page, name, theme, outDir); } else { await captureCase(page, name, theme, outDir); }
			}
		}
	} finally {
		if (original && original !== (await page.evaluate('window.__parity.themeClass()'))) { await setTheme(page, original); }
		await setModern(page, originalModern);
		page.close();
	}
}

main().catch(e => { console.error(e); process.exit(1); });
