// Lado do oráculo da sonda H12: roda no Node do próprio Electron do VS Code
// (ELECTRON_RUN_AS_NODE=1 ref/oracle/VSCode-linux-x64/code sqlite_dump.js <modulo> <db> [write chave valor])
// e usa o @vscode/sqlite3 nativo que o VS Code usa, para despejar ItemTable no formato
// chave<TAB>tipo<TAB>valor (blob em hexadecimal), igual ao `sqlite-probe dump`.
'use strict';
const [modulePath, dbPath, mode, key, value] = process.argv.slice(2);
const sqlite3 = require(modulePath);
const db = new sqlite3.Database(dbPath);
const typeOf = v => v === null ? 'null' : Buffer.isBuffer(v) ? 'blob' : typeof v === 'number' ? (Number.isInteger(v) ? 'integer' : 'real') : 'text';
const render = v => Buffer.isBuffer(v) ? v.toString('hex') : v === null ? '' : String(v);
const dump = () => db.all('SELECT key, value FROM ItemTable ORDER BY key', (err, rows) => {
	if (err) { throw err; }
	for (const r of rows) { process.stdout.write(`${r.key}\t${typeOf(r.value)}\t${render(r.value)}\n`); }
	process.stderr.write(`${rows.length} linhas\n`);
	db.get('PRAGMA integrity_check', (e, row) => { if (e) { throw e; } process.stderr.write(`integrity_check: ${row.integrity_check}\n`); db.close(); });
});
if (mode === 'write') {
	db.run('INSERT INTO ItemTable VALUES (?,?) ON CONFLICT (key) DO UPDATE SET value = excluded.value WHERE value != excluded.value', [key, value], function (err) {
		if (err) { throw err; }
		process.stderr.write(`${this.changes} linha(s) alterada(s)\n`);
		db.close();
	});
} else {
	dump();
}
