// H15: carrega e ativa UMA extensão embutida do VS Code sobre o motor que roda este arquivo.
// Uso: <motor> activate.js <dir da extensão>
'use strict';
const fs = require('fs');
const path = require('path');
const { vscode, touched, registeredCommands, state } = require('./fake-vscode.js');

const extDir = process.argv[2];
const t0 = process.hrtime.bigint();
const ms = (a, b) => Number(b - a) / 1e6;

const pkg = JSON.parse(fs.readFileSync(path.join(extDir, 'package.json'), 'utf8'));
state.packageJSON = pkg;
let main = path.join(extDir, pkg.main);
if (!fs.existsSync(main)) main += '.js';
const source = fs.readFileSync(main, 'utf8');
const t1 = process.hrtime.bigint();

const realRequire = require;
const missing = [];
function extRequire(id) {
  if (id === 'vscode') return vscode;
  try {
    return realRequire(id);
  } catch (error) {
    missing.push(id + ': ' + String(error && error.message).split('\n')[0]);
    throw error;
  }
}
extRequire.resolve = (id) => id;

const module_ = { exports: {} };
const wrapper = new Function('require', 'module', 'exports', '__dirname', '__filename', source);
const t2 = process.hrtime.bigint();
wrapper(extRequire, module_, module_.exports, path.dirname(main), main);
const t3 = process.hrtime.bigint();

const context = {
  subscriptions: [],
  extension: { id: pkg.publisher + '.' + pkg.name, packageJSON: pkg, extensionPath: extDir },
  extensionPath: extDir,
  extensionUri: vscode.Uri.file(extDir),
  globalState: { get: (k, d) => d, update: () => Promise.resolve(), keys: () => [], setKeysForSync() {} },
  workspaceState: { get: (k, d) => d, update: () => Promise.resolve(), keys: () => [] },
  asAbsolutePath: (p) => path.join(extDir, p),
  extensionMode: 1,
  environmentVariableCollection: {},
  secrets: {},
};

(async () => {
  let activateError = null;
  const t4 = process.hrtime.bigint();
  try {
    await module_.exports.activate(context);
  } catch (error) {
    activateError = error;
  }
  const t5 = process.hrtime.bigint();
  const report = {
    extension: pkg.name,
    bytes: source.length,
    readMs: ms(t0, t1),
    compileMs: ms(t1, t2),
    evalMs: ms(t2, t3),
    activateMs: ms(t4, t5),
    activated: !activateError,
    activateError: activateError ? String(activateError && activateError.stack || activateError).split('\n').slice(0, 4).join(' | ') : null,
    commands: [...registeredCommands.keys()].length,
    subscriptions: context.subscriptions.length,
    missingModules: missing,
    stubbedApis: [...touched.keys()].filter((k) => k.includes('stub')),
    realApisTouched: [...touched.keys()].filter((k) => !k.includes('stub')).length,
  };
  console.log(JSON.stringify(report, null, 1));
})();
