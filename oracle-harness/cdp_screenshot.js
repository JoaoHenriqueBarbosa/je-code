// Lado do oráculo da H26: captura a janela do VS Code real por CDP (Page.captureScreenshot).
// Uso: node oracle-harness/cdp_screenshot.js <porta> <saida.png> [espera_ms]
// A janela do workbench é o alvo `page` cuja URL termina em workbench.html.
'use strict';
const [port, out, waitMs = '4000'] = process.argv.slice(2);
const fs = require('fs');
(async () => {
	const targets = await (await fetch(`http://127.0.0.1:${port}/json/list`)).json();
	const page = targets.find(t => t.type === 'page' && /workbench/.test(t.url)) ?? targets.find(t => t.type === 'page');
	if (!page) { throw new Error('nenhuma página do workbench'); }
	await new Promise(r => setTimeout(r, Number(waitMs)));
	const ws = new WebSocket(page.webSocketDebuggerUrl);
	await new Promise((resolve, reject) => { ws.onopen = resolve; ws.onerror = reject; });
	let id = 0;
	const call = (method, params = {}) => new Promise((resolve, reject) => {
		const my = ++id;
		const onMessage = ev => {
			const msg = JSON.parse(ev.data);
			if (msg.id === my) { ws.removeEventListener('message', onMessage); msg.error ? reject(new Error(msg.error.message)) : resolve(msg.result); }
		};
		ws.addEventListener('message', onMessage);
		ws.send(JSON.stringify({ id: my, method, params }));
	});
	const metrics = await call('Runtime.evaluate', { expression: 'JSON.stringify({w: innerWidth, h: innerHeight, dpr: devicePixelRatio, font: getComputedStyle(document.body).fontFamily})', returnByValue: true });
	process.stderr.write(`janela: ${metrics.result.value}\n`);
	const shot = await call('Page.captureScreenshot', { format: 'png' });
	fs.writeFileSync(out, Buffer.from(shot.data, 'base64'));
	process.stderr.write(`gravado ${out}\n`);
	ws.close();
})().catch(e => { console.error(e); process.exit(1); });
