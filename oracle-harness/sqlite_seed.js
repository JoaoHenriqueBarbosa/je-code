// Lado do oráculo do teste de ida e volta do je-storage: cria um banco exatamente como o
// `SQLiteStorageDatabase` do VS Code (src/vs/base/parts/storage/node/storage.ts) cria e grava os itens
// de um arquivo JSON (objeto chave -> valor) com o mesmo upsert, pelo @vscode/sqlite3 do próprio VS Code:
// ELECTRON_RUN_AS_NODE=1 ref/oracle/VSCode-linux-x64/code sqlite_seed.js <modulo> <db> <itens.json>
'use strict';
const [modulePath, dbPath, itemsPath] = process.argv.slice(2);
const sqlite3 = require(modulePath);
const items = JSON.parse(require('fs').readFileSync(itemsPath, 'utf8'));
const db = new sqlite3.Database(dbPath, error => {
	if (error) { throw error; }
	db.exec('PRAGMA user_version = 1;CREATE TABLE IF NOT EXISTS ItemTable (key TEXT UNIQUE ON CONFLICT REPLACE, value BLOB);', error => {
		if (error) { throw error; }
		db.serialize(() => {
			db.run('BEGIN TRANSACTION');
			const stmt = db.prepare('INSERT INTO ItemTable VALUES (?,?) ON CONFLICT (key) DO UPDATE SET value = excluded.value WHERE value != excluded.value');
			for (const [key, value] of Object.entries(items)) { stmt.run([key, value]); }
			stmt.finalize();
			db.run('END TRANSACTION', error => {
				if (error) { throw error; }
				db.close(error => { if (error) { throw error; } process.stderr.write(`${Object.keys(items).length} itens gravados\n`); });
			});
		});
	});
});
