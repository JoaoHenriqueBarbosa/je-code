// Avalia uma expressão no workbench do oráculo por CDP e imprime o resultado.
// Uso: node oracle-harness/cdp_eval.js <porta> '<expressão>'
'use strict';
const [port, expression] = process.argv.slice(2);
(async () => {
	const targets = await (await fetch(`http://127.0.0.1:${port}/json/list`)).json();
	const page = targets.find(t => t.type === 'page' && /workbench/.test(t.url)) ?? targets.find(t => t.type === 'page');
	const ws = new WebSocket(page.webSocketDebuggerUrl);
	await new Promise((resolve, reject) => { ws.onopen = resolve; ws.onerror = reject; });
	ws.onmessage = ev => {
		const msg = JSON.parse(ev.data);
		if (msg.id === 1) {
			const r = msg.result?.result;
			console.log(r?.value !== undefined ? (typeof r.value === 'string' ? r.value : JSON.stringify(r.value)) : JSON.stringify(msg));
			ws.close();
		}
	};
	ws.send(JSON.stringify({ id: 1, method: 'Runtime.evaluate', params: { expression, returnByValue: true, awaitPromise: true } }));
})().catch(e => { console.error(e); process.exit(1); });
