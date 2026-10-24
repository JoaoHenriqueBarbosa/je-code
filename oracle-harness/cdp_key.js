// Envia uma tecla ao workbench do oráculo por CDP (Input.dispatchKeyEvent) e espera o resultado.
// Uso: node oracle-harness/cdp_key.js <porta> <tecla> [modificadores: ctrl,shift,alt] [texto]
// Exemplo: node oracle-harness/cdp_key.js 9333 P ctrl,shift
'use strict';
const [port, key, mods = '', text = ''] = process.argv.slice(2);
const bits = { alt: 1, ctrl: 2, meta: 4, shift: 8 };
const modifiers = mods.split(',').filter(Boolean).reduce((m, k) => m | bits[k], 0);
(async () => {
	const targets = await (await fetch(`http://127.0.0.1:${port}/json/list`)).json();
	const page = targets.find(t => t.type === 'page' && /workbench/.test(t.url)) ?? targets.find(t => t.type === 'page');
	const ws = new WebSocket(page.webSocketDebuggerUrl);
	await new Promise((resolve, reject) => { ws.onopen = resolve; ws.onerror = reject; });
	let id = 0;
	const send = (method, params) => new Promise(resolve => {
		const mine = ++id;
		const handler = ev => { const msg = JSON.parse(ev.data); if (msg.id === mine) { ws.removeEventListener('message', handler); resolve(msg); } };
		ws.addEventListener('message', handler);
		ws.send(JSON.stringify({ id: mine, method, params }));
	});
	const upper = key.length === 1 ? key.toUpperCase() : key;
	const code = key.length === 1 ? 'Key' + upper : key;
	const vk = key.length === 1 ? upper.charCodeAt(0) : ({ Enter: 13, Escape: 27, ArrowDown: 40, ArrowUp: 38 })[key];
	const base = { modifiers, key, code, windowsVirtualKeyCode: vk, nativeVirtualKeyCode: vk };
	if (text) {
		await send('Input.insertText', { text });
	} else {
		await send('Input.dispatchKeyEvent', { type: 'rawKeyDown', ...base });
		await send('Input.dispatchKeyEvent', { type: 'keyUp', ...base });
	}
	ws.close();
})().catch(e => { console.error(e); process.exit(1); });
