// GERADO por `scripts/gen-node-modules.js` a partir de `src/js/node/child_process.ts` do bun 1.4.2. Não editar à mão.
(function (host) {
  "use strict";
  const { validators, toClass, codes, makeAbortError } = host.internal;
  const debug = () => {};
  const assert = () => {};
  const isObject = value => value !== null && (typeof value === "object" || typeof value === "function");
  // `$newPromise()` e `$resolvePromise()` do JSC: uma promessa sem executor, resolvida de fora.
  const promiseResolvers = new WeakMap();
  const newPromise = () => {
    let resolve;
    const promise = new Promise(settle => {
      resolve = settle;
    });
    promiseResolvers.set(promise, resolve);
    return promise;
  };
  const resolvePromise = (promise, value) => promiseResolvers.get(promise)(value);
  const isBlob = value => typeof Blob === "function" && value instanceof Blob;
  // O que ainda não existe (TLS, `BlockList`...): lança no ponto de uso, nunca finge.
  const notImplemented = what => () => host.internal.throwNotImplemented(what);
  const require = name => (name.startsWith("internal/") || name.startsWith("native/") ? host.internalModule(name) : host.requireBuiltin(name));
  return (() => {
    const EventEmitter = require("node:events");
    const { kHandle } = require("internal/shared");
    const {
      validateBoolean,
      validateFunction,
      validateString,
      validateAbortSignal,
      validateArray,
      validateObject,
      validateOneOf
    } = require("internal/validators");
    var NetModule;
    var ObjectCreate = Object.create;
    var ObjectAssign = Object.assign;
    var BufferConcat = Buffer.concat;
    var BufferIsEncoding = Buffer.isEncoding;
    var kEmptyObject = ObjectCreate(null);
    let signals;
    function getSignals() {
      return signals ??= require("node:os").constants.signals;
    }
    var ArrayPrototypeJoin = Array.prototype.join;
    var ArrayPrototypeIncludes = Array.prototype.includes;
    var ArrayPrototypeSlice = Array.prototype.slice;
    var ArrayPrototypeUnshift = Array.prototype.unshift;
    const ArrayPrototypeFilter = Array.prototype.filter;
    const ArrayPrototypeSort = Array.prototype.sort;
    const StringPrototypeToUpperCase = String.prototype.toUpperCase;
    const ArrayPrototypePush = Array.prototype.push;
    const ArrayPrototypeLastIndexOf = Array.prototype.lastIndexOf;
    const ArrayPrototypeSplice = Array.prototype.splice;
    var ArrayBufferIsView = ArrayBuffer.isView;
    var NumberIsInteger = Number.isInteger;
    var ObjectHasOwn = Object.hasOwn;
    var StringPrototypeIncludes = String.prototype.includes;
    var StringPrototypeStartsWith = String.prototype.startsWith;
    var Uint8ArrayPrototypeIncludes = Uint8Array.prototype.includes;
    const MAX_BUFFER = 1024 * 1024;
    const kFromNode = Symbol("kFromNode");
    if (false) {
      debug("child_process: debug mode on");
      globalThis.__lastId = null;
      globalThis.__getId = () => {
        return globalThis.__lastId !== null ? globalThis.__lastId++ : 0;
      };
    }
    function spawn(file, args, options) {
      options = normalizeSpawnArguments(file, args, options);
      const windowsBatchFileError = options.windowsBatchFileError;
      if (windowsBatchFileError)
        throw windowsBatchFileError;
      validateTimeout(options.timeout);
      validateAbortSignal(options.signal, "options.signal");
      const killSignal = sanitizeKillSignal(options.killSignal);
      const child = new ChildProcess;
      debug("spawn", options);
      options[kFromNode] = true;
      child.spawn(options);
      const timeout = options.timeout;
      if (timeout && timeout > 0) {
        let timeoutId = setTimeout(() => {
          if (timeoutId) {
            timeoutId = null;
            try {
              child.kill(killSignal);
            } catch (err) {
              child.emit("error", err);
            }
          }
        }, timeout).unref();
        const clear = () => {
          if (timeoutId) {
            clearTimeout(timeoutId);
            timeoutId = null;
          }
        };
        child.once("exit", clear);
        child.once("close", clear);
      }
      const signal = options.signal;
      if (signal) {
        let onAbortListener = function() {
          abortChildProcess(child, killSignal, signal.reason);
        };
        if (signal.aborted) {
          process.nextTick(onAbortListener);
        } else {
          signal.addEventListener("abort", onAbortListener, { once: true });
          const remove = () => signal.removeEventListener("abort", onAbortListener);
          child.once("exit", remove);
          child.once("close", remove);
        }
      }
      return child;
    }
    function execFile(file, args, options, callback) {
      ({ file, args, options, callback } = normalizeExecFileArgs(file, args, options, callback));
      options = {
        __proto__: null,
        encoding: "utf8",
        timeout: 0,
        maxBuffer: MAX_BUFFER,
        killSignal: "SIGTERM",
        cwd: null,
        env: null,
        shell: false,
        ...options
      };
      const maxBuffer = options.maxBuffer;
      validateTimeout(options.timeout);
      validateMaxBuffer(maxBuffer);
      options.killSignal = sanitizeKillSignal(options.killSignal);
      const child = spawn(file, args, {
        cwd: options.cwd,
        env: options.env,
        timeout: options.timeout,
        killSignal: options.killSignal,
        uid: options.uid,
        gid: options.gid,
        cgroup: options.cgroup,
        windowsHide: options.windowsHide,
        windowsVerbatimArguments: options.windowsVerbatimArguments,
        shell: options.shell,
        signal: options.signal
      });
      let encoding;
      const _stdout = [];
      const _stderr = [];
      const optionsEncoding = options.encoding;
      if (optionsEncoding !== "buffer" && BufferIsEncoding(optionsEncoding)) {
        encoding = optionsEncoding;
      } else {
        encoding = null;
      }
      let killed = false;
      let exited = false;
      let timeoutId;
      let ex = null;
      let cmd = file;
      function exitHandler(code = 0, signal) {
        if (exited)
          return;
        exited = true;
        if (timeoutId) {
          clearTimeout(timeoutId);
          timeoutId = null;
        }
        if (!callback)
          return;
        let stdout;
        let stderr;
        if (encoding || child.stdout?.readableEncoding) {
          stdout = ArrayPrototypeJoin.call(_stdout, "");
        } else {
          stdout = BufferConcat(_stdout);
        }
        if (encoding || child.stderr?.readableEncoding) {
          stderr = ArrayPrototypeJoin.call(_stderr, "");
        } else {
          stderr = BufferConcat(_stderr);
        }
        if (!ex && code === 0 && signal === null) {
          callback(null, stdout, stderr);
          return;
        }
        if (args?.length)
          cmd += ` ${ArrayPrototypeJoin.call(args, " ")}`;
        if (!ex) {
          const { getSystemErrorName } = require("node:util");
          let message = `Command failed: ${cmd}`;
          if (stderr)
            message += `
    ${stderr}`;
          ex = genericNodeError(message, {
            code: code < 0 ? getSystemErrorName(code) : code,
            killed: child.killed || killed,
            signal
          });
        }
        ex.cmd = cmd;
        callback(ex, stdout, stderr);
      }
      function errorHandler(e) {
        ex = e;
        const { stdout, stderr } = child;
        if (stdout)
          stdout.destroy();
        if (stderr)
          stderr.destroy();
        exitHandler();
      }
      function kill() {
        const { stdout, stderr } = child;
        if (stdout)
          stdout.destroy();
        if (stderr)
          stderr.destroy();
        killed = true;
        try {
          child.kill(options.killSignal);
        } catch (e) {
          ex = e;
          exitHandler();
        }
      }
      const optionsTimeout = options.timeout;
      if (optionsTimeout > 0) {
        timeoutId = setTimeout(function delayedKill() {
          timeoutId = null;
          kill();
        }, optionsTimeout).unref();
      }
      function addOnDataListener(child_buffer, _buffer, kind) {
        if (encoding)
          child_buffer.setEncoding(encoding);
        let totalLen = 0;
        if (maxBuffer === 1 / 0) {
          child_buffer.on("data", function onDataNoMaxBuf(chunk) {
            Array.prototype.push.call(_buffer, chunk);
          });
          return;
        }
        child_buffer.on("data", function onData(chunk) {
          const encoding = child_buffer.readableEncoding;
          if (encoding) {
            const length = Buffer.byteLength(chunk, encoding);
            totalLen += length;
            if (totalLen > maxBuffer) {
              const truncatedLen = maxBuffer - (totalLen - length);
              Array.prototype.push.call(_buffer, String.prototype.slice.call(chunk, 0, truncatedLen));
              ex = codes.CHILD_PROCESS_STDIO_MAXBUFFER(kind);
              kill();
            } else {
              Array.prototype.push.call(_buffer, chunk);
            }
          } else {
            const length = chunk.length;
            totalLen += length;
            if (totalLen > maxBuffer) {
              const truncatedLen = maxBuffer - (totalLen - length);
              Array.prototype.push.call(_buffer, chunk.slice(0, truncatedLen));
              ex = codes.CHILD_PROCESS_STDIO_MAXBUFFER(kind);
              kill();
            } else {
              Array.prototype.push.call(_buffer, chunk);
            }
          }
        });
      }
      const childStdout = child.stdout;
      if (childStdout)
        addOnDataListener(childStdout, _stdout, "stdout");
      const childStderr = child.stderr;
      if (childStderr)
        addOnDataListener(childStderr, _stderr, "stderr");
      child.addListener("close", exitHandler);
      child.addListener("error", errorHandler);
      return child;
    }
    function exec(command, options, callback) {
      const opts = normalizeExecArgs(command, options, callback);
      return execFile(opts.file, opts.options, opts.callback);
    }
    const kCustomPromisifySymbol = Symbol.for("nodejs.util.promisify.custom");
    const customPromiseExecFunction = (orig) => {
      return (...args) => {
        const { resolve, reject, promise } = Promise.withResolvers();
        promise.child = orig(...args, (err, stdout, stderr) => {
          if (err !== null) {
            err.stdout = stdout;
            err.stderr = stderr;
            reject(err);
          } else {
            resolve({ stdout, stderr });
          }
        });
        return promise;
      };
    };
    Object.defineProperty(exec, kCustomPromisifySymbol, {
      __proto__: null,
      configurable: true,
      value: customPromiseExecFunction(exec)
    });
    exec[kCustomPromisifySymbol][kCustomPromisifySymbol] = exec[kCustomPromisifySymbol];
    Object.defineProperty(execFile, kCustomPromisifySymbol, {
      __proto__: null,
      configurable: true,
      value: customPromiseExecFunction(execFile)
    });
    execFile[kCustomPromisifySymbol][kCustomPromisifySymbol] = execFile[kCustomPromisifySymbol];
    function spawnSync(file, args, options) {
      options = {
        __proto__: null,
        maxBuffer: MAX_BUFFER,
        ...normalizeSpawnArguments(file, args, options)
      };
      if (options.windowsBatchFileError) {
        const error = new SystemError(`spawnSync ${options.file} EINVAL`, options.file, "spawnSync " + options.file, -4071, "EINVAL");
        error.spawnargs = ArrayPrototypeSlice.call(options.args, 1);
        return {
          signal: null,
          status: null,
          output: [null, null, null],
          pid: 0,
          stdout: null,
          stderr: null,
          error
        };
      }
      const maxBuffer = options.maxBuffer;
      const encoding = options.encoding;
      debug("spawnSync", options);
      validateTimeout(options.timeout);
      validateMaxBuffer(maxBuffer);
      options.killSignal = sanitizeKillSignal(options.killSignal);
      const stdio = options.stdio || "pipe";
      const bunStdio = getBunStdioFromOptions(stdio);
      var { input } = options;
      if (input) {
        if (ArrayBufferIsView(input)) {
          bunStdio[0] = input;
        } else if (typeof input === "string") {
          bunStdio[0] = Buffer.from(input, encoding || "utf8");
        } else {
          throw codes.INVALID_ARG_TYPE(`options.stdio[0]`, ["string", "Buffer", "TypedArray", "DataView"], input);
        }
      }
      var error;
      try {
        var {
          stdout = null,
          stderr = null,
          exitCode,
          signalCode,
          exitedDueToTimeout,
          exitedDueToMaxBuffer,
          pid
        } = Bun.spawnSync({
          cmd: [options.file, ...Array.prototype.slice.call(options.args, 1)],
          env: options[kBunEnv] || options.env || undefined,
          cwd: options.cwd || undefined,
          stdio: bunStdio,
          detached: options.detached,
          uid: options.uid,
          gid: options.gid,
          cgroup: options.cgroup,
          windowsVerbatimArguments: options.windowsVerbatimArguments,
          windowsHide: options.windowsHide,
          argv0: options.args[0],
          timeout: options.timeout,
          killSignal: options.killSignal,
          maxBuffer: options.maxBuffer
        });
      } catch (err) {
        error = err;
        stdout = null;
        stderr = null;
      }
      const outputStdout = typeof stdout === "number" ? null : stdout;
      const outputStderr = typeof stderr === "number" ? null : stderr;
      const result = {
        signal: signalCode ?? null,
        status: exitCode,
        output: [null, outputStdout, outputStderr],
        pid
      };
      if (error) {
        result.error = error;
      }
      if (outputStdout && encoding && encoding !== "buffer") {
        result.output[1] = result.output[1]?.toString(encoding);
      }
      if (outputStderr && encoding && encoding !== "buffer") {
        result.output[2] = result.output[2]?.toString(encoding);
      }
      result.stdout = result.output[1];
      result.stderr = result.output[2];
      if (exitedDueToTimeout && error == null) {
        result.error = new SystemError("spawnSync " + options.file + " ETIMEDOUT", options.file, "spawnSync " + options.file, etimedoutErrorCode(), "ETIMEDOUT");
      }
      if (exitedDueToMaxBuffer && error == null) {
        result.error = new SystemError("spawnSync " + options.file + " ENOBUFS (stdout or stderr buffer reached maxBuffer size limit)", options.file, "spawnSync " + options.file, enobufsErrorCode(), "ENOBUFS");
      }
      const resultError = result.error;
      if (resultError) {
        resultError.syscall = "spawnSync " + options.file;
        resultError.spawnargs = ArrayPrototypeSlice.call(options.args, 1);
      }
      return result;
    }
    const etimedoutErrorCode = (() => -110);
    const enobufsErrorCode = (() => -105);
    function execFileSync(file, args, options) {
      ({ file, args, options } = normalizeExecFileArgs(file, args, options));
      const inheritStderr = !options.stdio;
      const ret = spawnSync(file, args, options);
      let retStderr;
      if (inheritStderr && (retStderr = ret.stderr))
        process.stderr.write(retStderr);
      const errArgs = [options.argv0 || file];
      ArrayPrototypePush.apply(errArgs, args);
      const err = checkExecSyncError(ret, errArgs);
      if (err)
        throw err;
      return ret.stdout;
    }
    function execSync(command, options) {
      const opts = normalizeExecArgs(command, options, null);
      const inheritStderr = !opts.options.stdio;
      const ret = spawnSync(opts.file, opts.options);
      let retStderr;
      if (inheritStderr && (retStderr = ret.stderr))
        process.stderr.write(retStderr);
      const err = checkExecSyncError(ret, undefined, command);
      if (err)
        throw err;
      return ret.stdout;
    }
    function stdioStringToArray(stdio, channel) {
      let options;
      switch (stdio) {
        case "ignore":
        case "overlapped":
        case "pipe":
          options = [stdio, stdio, stdio];
          break;
        case "inherit":
          options = [0, 1, 2];
          break;
        default:
          throw codes.INVALID_ARG_VALUE("stdio", stdio);
      }
      if (channel)
        Array.prototype.push.call(options, channel);
      return options;
    }
    function fork(modulePath, args = [], options) {
      modulePath = getValidatedPath(modulePath, "modulePath");
      if (args == null) {
        args = [];
      } else if (typeof args === "object" && !Array.isArray(args)) {
        options = args;
        args = [];
      } else {
        validateArray(args, "args");
      }
      if (options != null) {
        validateObject(options, "options");
      }
      options = { __proto__: null, ...options, shell: false };
      options.execPath = options.execPath || process.execPath;
      validateArgumentNullCheck(options.execPath, "options.execPath");
      let execArgv = options.execArgv || process.execArgv;
      validateArgumentsNullCheck(execArgv, "options.execArgv");
      let processEval;
      if (execArgv === process.execArgv && (processEval = process._eval) != null) {
        const index = ArrayPrototypeLastIndexOf.call(execArgv, processEval);
        if (index > 0) {
          execArgv = ArrayPrototypeSlice.call(execArgv);
          ArrayPrototypeSplice.call(execArgv, index - 1, 2);
        }
      }
      args = [...execArgv, modulePath, ...args];
      if (typeof options.stdio === "string") {
        options.stdio = stdioStringToArray(options.stdio, "ipc");
      } else if (!Array.isArray(options.stdio)) {
        options.stdio = stdioStringToArray(options.silent ? "pipe" : "inherit", "ipc");
      } else if (!ArrayPrototypeIncludes.call(options.stdio, "ipc")) {
        throw codes.CHILD_PROCESS_IPC_REQUIRED("options.stdio");
      }
      return spawn(options.execPath, args, options);
    }
    function convertToValidSignal(signal) {
      if (typeof signal === "number" && getSignalsToNamesMapping()[signal])
        return signal;
      if (typeof signal === "string") {
        const signalName = getSignals()[StringPrototypeToUpperCase.call(signal)];
        if (signalName)
          return signalName;
      }
      throw ERR_UNKNOWN_SIGNAL(signal);
    }
    function sanitizeKillSignal(killSignal) {
      if (typeof killSignal === "string" || typeof killSignal === "number") {
        return convertToValidSignal(killSignal);
      } else if (killSignal != null) {
        throw codes.INVALID_ARG_TYPE("options.killSignal", ["string", "number"], killSignal);
      }
    }
    let signalsToNamesMapping;
    function getSignalsToNamesMapping() {
      if (signalsToNamesMapping !== undefined)
        return signalsToNamesMapping;
      signalsToNamesMapping = ObjectCreate(null);
      const names = getSignals();
      for (const key in names) {
        signalsToNamesMapping[names[key]] = key;
      }
      return signalsToNamesMapping;
    }
    function normalizeExecFileArgs(file, args, options, callback) {
      if (Array.isArray(args)) {
        args = ArrayPrototypeSlice.call(args);
      } else if (args != null && typeof args === "object") {
        callback = options;
        options = args;
        args = null;
      } else if (typeof args === "function") {
        callback = args;
        options = null;
        args = null;
      }
      if (args == null) {
        args = [];
      }
      if (typeof options === "function") {
        callback = options;
      } else if (options != null) {
        validateObject(options, "options");
      }
      if (options == null) {
        options = kEmptyObject;
      }
      if (callback != null) {
        validateFunction(callback, "callback");
      }
      const argv0 = options.argv0;
      if (argv0 != null) {
        validateString(argv0, "options.argv0");
        validateArgumentNullCheck(argv0, "options.argv0");
      }
      return { file, args, options, callback };
    }
    function normalizeExecArgs(command, options, callback) {
      validateString(command, "command");
      validateArgumentNullCheck(command, "command");
      if (typeof options === "function") {
        callback = options;
        options = undefined;
      }
      options = { __proto__: null, ...options };
      options.shell = typeof options.shell === "string" ? options.shell : true;
      return {
        file: command,
        options,
        callback
      };
    }
    const kBunEnv = Symbol("bunEnv");
    function normalizeSpawnArguments(file, args, options) {
      validateString(file, "file");
      validateArgumentNullCheck(file, "file");
      if (file.length === 0)
        throw codes.INVALID_ARG_VALUE("file", file, "cannot be empty");
      if (Array.isArray(args)) {
        args = ArrayPrototypeSlice.call(args);
      } else if (args == null) {
        args = [];
      } else if (typeof args !== "object") {
        throw codes.INVALID_ARG_TYPE("args", "object", args);
      } else {
        options = args;
        args = [];
      }
      validateArgumentsNullCheck(args, "args");
      if (options === undefined)
        options = {};
      else
        validateObject(options, "options");
      options = { __proto__: null, ...options };
      let cwd = options.cwd;
      if (cwd != null) {
        cwd = getValidatedPath(cwd, "options.cwd");
      }
      const detached = options.detached;
      if (detached != null) {
        validateBoolean(detached, "options.detached");
      }
      const uid = options.uid;
      if (uid != null && !isInt32(uid)) {
        throw codes.INVALID_ARG_TYPE("options.uid", "int32", uid);
      }
      const gid = options.gid;
      if (gid != null && !isInt32(gid)) {
        throw codes.INVALID_ARG_TYPE("options.gid", "int32", gid);
      }
      const shell = options.shell;
      if (shell != null && typeof shell !== "boolean" && typeof shell !== "string") {
        throw codes.INVALID_ARG_TYPE("options.shell", ["boolean", "string"], shell);
      }
      const argv0 = options.argv0;
      if (argv0 != null) {
        validateString(argv0, "options.argv0");
        validateArgumentNullCheck(argv0, "options.argv0");
      }
      const windowsHide = options.windowsHide;
      if (windowsHide != null) {
        validateBoolean(windowsHide, "options.windowsHide");
      }
      let { windowsVerbatimArguments } = options;
      if (windowsVerbatimArguments != null) {
        validateBoolean(windowsVerbatimArguments, "options.windowsVerbatimArguments");
      }
      let windowsBatchFileError;
      if (shell) {
        validateArgumentNullCheck(shell, "options.shell");
        const command = ArrayPrototypeJoin.call([file, ...args], " ");
        if (process.platform === "win32") {
          if (typeof shell === "string")
            file = shell;
          else
            file = process.env.comspec || "cmd.exe";
          if (/^(?:.*\\)?cmd(?:\.exe)?$/i.exec(file) !== null) {
            args = ["/d", "/s", "/c", `"${command}"`];
            windowsVerbatimArguments = true;
          } else {
            args = ["-c", command];
          }
        } else {
          if (typeof shell === "string")
            file = shell;
          else if (process.platform === "android")
            file = "sh";
          else
            file = "/bin/sh";
          args = ["-c", command];
        }
      } else if (process.platform === "win32" && /\.(?:cmd|bat)[\s.]*$/i.exec(file) !== null) {
        windowsBatchFileError = new SystemError(`spawn ${file} EINVAL`, file, "spawn", -4071, "EINVAL");
      }
      if (typeof argv0 === "string") {
        ArrayPrototypeUnshift.call(args, argv0);
      } else {
        ArrayPrototypeUnshift.call(args, file);
      }
      const env = options.env || process.env;
      const bunEnv = {};
      let envKeys = [];
      for (const key in env) {
        ArrayPrototypePush.call(envKeys, key);
      }
      if (process.platform === "win32") {
        const sawKey = new Set;
        envKeys = ArrayPrototypeFilter.call(ArrayPrototypeSort.call(envKeys), (key) => {
          const uppercaseKey = StringPrototypeToUpperCase.call(key);
          if (sawKey.has(uppercaseKey)) {
            return false;
          }
          sawKey.add(uppercaseKey);
          return true;
        });
      }
      for (const key of envKeys) {
        const value = env[key];
        if (value !== undefined) {
          validateArgumentNullCheck(key, `options.env['${key}']`);
          validateArgumentNullCheck(value, `options.env['${key}']`);
          bunEnv[key] = value;
        }
      }
      return {
        __proto__: null,
        ...options,
        args,
        cwd,
        detached: !!options.detached,
        [kBunEnv]: bunEnv,
        file,
        windowsHide: !!options.windowsHide,
        windowsVerbatimArguments: !!windowsVerbatimArguments,
        argv0: options.argv0,
        windowsBatchFileError
      };
    }
    function checkExecSyncError(ret, args, cmd) {
      let err;
      const retError = ret.error;
      if (retError) {
        err = retError;
        ObjectAssign(err, ret);
        delete err.error;
      } else if (ret.status !== 0) {
        let msg = "Command failed: ";
        msg += cmd || ArrayPrototypeJoin.call(args, " ");
        const stderr = ret.stderr;
        if (stderr && stderr.length > 0)
          msg += `
    ${stderr.toString()}`;
        err = genericNodeError(msg, ret);
      }
      return err;
    }
    function parseEnvPairs(envPairs) {
      if (!envPairs)
        return;
      const resEnv = {};
      for (const line of envPairs) {
        const [key, ...value] = line.split("=", 2);
        resEnv[key] = value.join("=");
      }
      return resEnv;
    }

    class ChildProcess extends EventEmitter {
      #handle;
      #closesNeeded = 1;
      #closesGot = 0;
      signalCode = null;
      exitCode = null;
      spawnfile;
      spawnargs;
      pid;
      channel;
      killed = false;
      [Symbol.dispose]() {
        if (!this.killed) {
          this.kill();
        }
      }
      #handleOnExit(exitCode, signalCode, err) {
        if (signalCode) {
          this.signalCode = signalCode;
        } else {
          this.exitCode = exitCode;
        }
        {
          if (this.#stdin) {
            this.#stdin.destroy();
          } else if (this.#stdioOptions[0] === "pipe") {
            this.#stdioOptions[0] = "destroyed";
          }
          if (err) {
            if (this.#stdioOptions[1] === "pipe")
              this.#stdioOptions[1] = "destroyed";
            if (this.#stdioOptions[2] === "pipe")
              this.#stdioOptions[2] = "destroyed";
          }
          const stdout = this.#stdout, stderr = this.#stderr;
          if (stdout === undefined) {
            this.#stdout = this.#getBunSpawnIo(1, true);
          } else if (stdout && this.#stdioOptions[1] === "pipe" && !stdout.destroyed && stdout.readable) {
            stdout.resume?.();
          }
          if (stderr === undefined) {
            this.#stderr = this.#getBunSpawnIo(2, true);
          } else if (stderr && this.#stdioOptions[2] === "pipe" && !stderr.destroyed && stderr.readable) {
            stderr.resume?.();
          }
        }
        const spawnfile = this.spawnfile;
        if (err) {
          if (spawnfile)
            err.path = spawnfile;
          err.spawnargs = ArrayPrototypeSlice.call(this.spawnargs, 1);
          err.pid = this.pid;
          this.emit("error", err);
        } else if (exitCode < 0) {
          const err = new SystemError(`Spawned process exited with error code: ${exitCode}`, undefined, "spawn", "EUNKNOWN", "ERR_CHILD_PROCESS_UNKNOWN_ERROR");
          err.pid = this.pid;
          if (spawnfile)
            err.path = spawnfile;
          err.spawnargs = ArrayPrototypeSlice.call(this.spawnargs, 1);
          this.emit("error", err);
        }
        this.emit("exit", this.exitCode, this.signalCode);
        this.#maybeClose();
      }
      #getBunSpawnIo(i, autoResume = false) {
        if (false && !this.#handle) {
          if (this.#handle === null) {
            debug("ChildProcess: getBunSpawnIo: this.#handle is null. This means the subprocess already exited");
          } else {
            debug("ChildProcess: getBunSpawnIo: this.#handle is undefined");
          }
        }
        const handle = this.#handle;
        const io = this.#stdioOptions[i];
        switch (i) {
          case 0: {
            switch (io) {
              case "pipe": {
                const stdin = handle?.stdin;
                if (!stdin) {
                  const Writable = require("internal/streams/writable");
                  const stream = new Writable({
                    write(chunk, encoding, callback) {
                      if (callback)
                        callback();
                      return false;
                    }
                  });
                  stream.destroy();
                  return stream;
                }
                const result = host.fsStreams().writableFromFileSink(stdin);
                result.readable = false;
                return result;
              }
              case "inherit":
                return null;
              case "destroyed": {
                const Writable = require("internal/streams/writable");
                const stream = new Writable({
                  write(chunk, encoding, callback) {
                    if (callback)
                      callback();
                    return false;
                  }
                });
                stream.destroy();
                return stream;
              }
              case "undefined":
                return;
              default:
                return null;
            }
          }
          case 2:
          case 1: {
            switch (io) {
              case "pipe": {
                const value = handle?.[fdToStdioName(i)];
                if (!value) {
                  const Readable = require("internal/streams/readable");
                  const stream = new Readable({ read() {} });
                  stream.destroy();
                  return stream;
                }
                const pipe = require("native/native-readable").constructNativeReadable(value, {});
                this.#closesNeeded++;
                pipe.once("close", () => this.#maybeClose());
                if (autoResume)
                  pipe.resume();
                return pipe;
              }
              case "destroyed": {
                const Readable = require("internal/streams/readable");
                const stream = new Readable({ read() {} });
                stream.destroy();
                return stream;
              }
              case "undefined":
                return;
              default:
                return null;
            }
          }
          default:
            switch (io) {
              case "pipe":
              case "socket-fd":
                if (!NetModule)
                  NetModule = require("node:net");
                const fd = handle && handle.stdio[i];
                if (fd == null)
                  return null;
                return NetModule.connect({ fd });
            }
            return null;
        }
      }
      #stdin;
      #stdout;
      #stderr;
      #stdioObject;
      #stdioOptions;
      #createStdioObject() {
        const opts = this.#stdioOptions;
        const length = opts.length;
        let result = new Array(length);
        for (let i = 0;i < length; i++) {
          const element = opts[i];
          if (element === "undefined") {
            return;
          }
          if (element !== "pipe" && element !== "socket-fd") {
            result[i] = null;
            continue;
          }
          switch (i) {
            case 0:
              result[i] = this.stdin;
              continue;
            case 1:
              result[i] = this.stdout;
              continue;
            case 2:
              result[i] = this.stderr;
              continue;
            default:
              result[i] = this.#getBunSpawnIo(i, false);
              continue;
          }
        }
        return result;
      }
      get stdin() {
        return this.#stdin ??= this.#getBunSpawnIo(0, false);
      }
      get stdout() {
        return this.#stdout ??= this.#getBunSpawnIo(1, false);
      }
      get stderr() {
        return this.#stderr ??= this.#getBunSpawnIo(2, false);
      }
      get stdio() {
        return this.#stdioObject ??= this.#createStdioObject();
      }
      get connected() {
        const handle = this.#handle;
        if (handle === null)
          return false;
        return handle.connected ?? false;
      }
      get [kHandle]() {
        return this.#handle;
      }
      spawn(options) {
        validateObject(options, "options");
        validateOneOf(options.serialization, "options.serialization", [undefined, "json", "advanced"]);
        const serialization = options.serialization || "json";
        const stdio = options.stdio || ["pipe", "pipe", "pipe"];
        const bunStdio = getBunStdioFromOptions(stdio);
        if (process.platform !== "win32") {
          for (let i = 3;i < bunStdio.length; i++) {
            if (bunStdio[i] === "pipe")
              bunStdio[i] = "socket-fd";
          }
        }
        const has_ipc = Array.isArray(stdio) && stdio.includes("ipc");
        const envPairs = options.envPairs;
        if (has_ipc) {
          if (envPairs !== undefined) {
            validateArray(envPairs, "options.envPairs");
          }
        }
        var env = options[kBunEnv] || parseEnvPairs(envPairs) || process.env;
        const detachedOption = options.detached;
        this.#stdioOptions = bunStdio;
        const stdioCount = stdio.length;
        const hasSocketsToEagerlyLoad = stdioCount >= 3;
        validateString(options.file, "options.file");
        var file;
        file = this.spawnfile = options.file;
        var spawnargs;
        if (options.args === undefined) {
          spawnargs = this.spawnargs = [];
        } else {
          validateArray(options.args, "options.args");
          spawnargs = this.spawnargs = options.args;
        }
        try {
          this.#handle = Bun.spawn({
            cmd: [file, ...Array.prototype.slice.call(spawnargs, 1)],
            stdio: bunStdio,
            cwd: options.cwd || undefined,
            env,
            detached: typeof detachedOption !== "undefined" ? !!detachedOption : false,
            uid: options.uid,
            gid: options.gid,
            cgroup: options.cgroup,
            onExit: (handle, exitCode, signalCode, err) => {
              this.#handle = handle;
              this.pid = this.#handle.pid;
              debug("ChildProcess: onExit", exitCode, signalCode, err, this.pid);
              if (hasSocketsToEagerlyLoad) {
                process.nextTick(() => {
                  this.stdio;
                  debug("ChildProcess: onExit", exitCode, signalCode, err, this.pid);
                });
              }
              process.nextTick((exitCode, signalCode, err) => this.#handleOnExit(exitCode, signalCode, err), exitCode, signalCode, err);
            },
            lazy: true,
            ipc: has_ipc ? this.#emitIpcMessage.bind(this) : undefined,
            onDisconnect: has_ipc ? (ok) => this.#onDisconnect(ok) : undefined,
            serialization,
            argv0: spawnargs[0],
            windowsHide: !!options.windowsHide,
            windowsVerbatimArguments: !!options.windowsVerbatimArguments
          });
          this.pid = this.#handle.pid;
          debug("ChildProcess: spawn", this.pid, spawnargs);
          process.nextTick(() => {
            this.emit("spawn");
          });
          if (has_ipc) {
            this.send = this.#send;
            this.disconnect = this.#disconnect;
            this.channel = new Control;
            Object.defineProperty(this, "_channel", {
              get() {
                return this.channel;
              },
              set(value) {
                this.channel = value;
              }
            });
            if (options[kFromNode])
              this.#closesNeeded += 1;
          }
          if (hasSocketsToEagerlyLoad) {
            for (let item of this.stdio) {
              item?.ref?.();
            }
          }
        } catch (ex) {
          const exCode = ex != null && typeof ex === "object" && Object.hasOwn(ex, "code") ? ex.code : undefined;
          if (exCode === "EACCES" || exCode === "EAGAIN" || exCode === "EMFILE" || exCode === "ENFILE" || exCode === "ENOENT") {
            this.#handle = null;
            ex.syscall = "spawn " + this.spawnfile;
            ex.spawnargs = Array.prototype.slice.call(this.spawnargs, 1);
            process.nextTick(() => {
              this.emit("error", ex);
              this.emit("close", ex.errno ?? -1);
            });
            if (exCode === "EMFILE" || exCode === "ENFILE") {
              this.#stdioOptions[0] = "undefined";
              this.#stdioOptions[1] = "undefined";
              this.#stdioOptions[2] = "undefined";
            }
          } else {
            if (exCode !== undefined) {
              ex.syscall = "spawn";
            }
            throw ex;
          }
        }
      }
      #emitIpcMessage(message, _, handle) {
        this.emit(isInternalIpcMessage(message) ? "internalMessage" : "message", message, handle);
      }
      #send(message, handle, options, callback) {
        if (typeof handle === "function") {
          callback = handle;
          handle = undefined;
          options = undefined;
        } else if (typeof options === "function") {
          callback = options;
          options = undefined;
        } else if (options !== undefined) {
          if (typeof options !== "object" || options === null) {
            throw codes.INVALID_ARG_TYPE("options", "object", options);
          }
        }
        if (!this.#handle) {
          if (callback) {
            process.nextTick(callback, new TypeError("Process was closed while trying to send message"));
          } else {
            this.emit("error", new TypeError("Process was closed while trying to send message"));
          }
          return false;
        }
        return this.#handle.send(message, handle, options, (err) => {
          if (callback) {
            callback(err);
          } else if (err) {
            this.emit("error", err);
          }
        });
      }
      #onDisconnect(firstTime) {
        if (!firstTime) {
          return;
        }
        assert(!this.connected);
        process.nextTick(() => this.emit("disconnect"));
        process.nextTick(() => this.#maybeClose());
      }
      #disconnect() {
        if (!this.connected) {
          this.emit("error", codes.IPC_DISCONNECTED());
          return;
        }
        this.#handle.disconnect();
        this.channel = null;
      }
      kill(sig) {
        const signal = sig === 0 ? sig : convertToValidSignal(sig === undefined ? "SIGTERM" : sig);
        const handle = this.#handle;
        if (handle) {
          if (handle.killed) {
            return false;
          }
          try {
            handle.kill(signal);
            this.killed = true;
            return true;
          } catch (e) {
            this.emit("error", e);
          }
        }
        return false;
      }
      #maybeClose() {
        debug("Attempting to maybe close...");
        this.#closesGot++;
        if (this.#closesGot === this.#closesNeeded) {
          this.emit("close", this.exitCode, this.signalCode);
        }
      }
      ref() {
        if (this.#handle)
          this.#handle.ref();
      }
      unref() {
        if (this.#handle)
          this.#handle.unref();
      }
      static {
        Object.defineProperties(this.prototype, {
          stdin: {
            get: function() {
              const value = this.#stdin ??= this.#getBunSpawnIo(0, false);
              Object.defineProperty(this, "stdin", {
                value,
                enumerable: true,
                configurable: true,
                writable: true
              });
              return value;
            },
            enumerable: true,
            configurable: true
          },
          stdout: {
            get: function() {
              const value = this.#stdout ??= this.#getBunSpawnIo(1, false);
              Object.defineProperty(this, "stdout", {
                value,
                enumerable: true,
                configurable: true,
                writable: true
              });
              return value;
            },
            enumerable: true,
            configurable: true
          },
          stderr: {
            get: function() {
              const value = this.#stderr ??= this.#getBunSpawnIo(2, false);
              Object.defineProperty(this, "stderr", {
                value,
                enumerable: true,
                configurable: true,
                writable: true
              });
              return value;
            },
            enumerable: true,
            configurable: true
          },
          stdio: {
            get: function() {
              const value = this.#stdioObject ??= this.#createStdioObject();
              Object.defineProperty(this, "stdio", {
                value,
                enumerable: true,
                configurable: true,
                writable: true
              });
              return value;
            },
            enumerable: true,
            configurable: true
          }
        });
      }
    }
    const nodeToBunLookup = {
      ignore: null,
      pipe: "pipe",
      overlapped: "pipe",
      inherit: "inherit",
      ipc: "ipc"
    };
    const INTERNAL_IPC_PREFIX = "NODE_";
    function isInternalIpcMessage(message) {
      if (message === null || typeof message !== "object")
        return false;
      if (!ObjectHasOwn(message, "cmd"))
        return false;
      const cmd = message.cmd;
      if (typeof cmd !== "string" || cmd.length <= INTERNAL_IPC_PREFIX.length)
        return false;
      return StringPrototypeStartsWith.call(cmd, INTERNAL_IPC_PREFIX);
    }
    function streamFdOf(item) {
      const itemFd = ObjectHasOwn(item, "fd") ? item.fd : undefined;
      if (typeof itemFd === "number")
        return itemFd;
      const handle = item._handle;
      const handleFd = handle ? handle.fd : undefined;
      if (typeof handleFd === "number")
        return handleFd;
      if (item.destroyed)
        return;
      const sink = item[host.fsStreams().kWriteStreamFastPath];
      if (sink && sink !== true) {
        const fd = sink._getFd();
        if (typeof fd === "number" && fd >= 0)
          return fd;
      }
      return;
    }
    function nodeToBun(item, index) {
      if (item == null) {
        return index > 2 ? "ignore" : "pipe";
      }
      if (typeof item === "number") {
        return item;
      }
      if (isNodeStreamReadable(item) || isNodeStreamWritable(item)) {
        const fd = streamFdOf(item);
        if (fd !== undefined)
          return fd;
        const kind = isNodeStreamReadable(item) ? "Readable" : "Writable";
        throw new Error(`Passing a stream.${kind} without an underlying file descriptor as stdio[${index}] is not yet implemented in Bun`);
      }
      const result = nodeToBunLookup[item];
      if (result === undefined) {
        throw new Error(`Invalid stdio option[${index}] "${item}"`);
      }
      return result;
    }
    function isNodeStreamReadable(item) {
      if (typeof item !== "object")
        return false;
      if (!item)
        return false;
      if (typeof item.on !== "function")
        return false;
      if (typeof item.pipe !== "function")
        return false;
      return true;
    }
    function isNodeStreamWritable(item) {
      if (typeof item !== "object")
        return false;
      if (!item)
        return false;
      if (typeof item.on !== "function")
        return false;
      if (typeof item.write !== "function")
        return false;
      return true;
    }
    function fdToStdioName(fd) {
      switch (fd) {
        case 0:
          return "stdin";
        case 1:
          return "stdout";
        case 2:
          return "stderr";
        default:
          return null;
      }
    }
    function getBunStdioFromOptions(stdio) {
      const normalizedStdio = normalizeStdio(stdio);
      if (normalizedStdio.filter((v) => v === "ipc").length > 1)
        throw codes.IPC_ONE_PIPE();
      const bunStdio = normalizedStdio.map(nodeToBun);
      return bunStdio;
    }
    function normalizeStdio(stdio) {
      if (typeof stdio === "string") {
        switch (stdio) {
          case "ignore":
            return ["ignore", "ignore", "ignore"];
          case "pipe":
            return ["pipe", "pipe", "pipe"];
          case "overlapped":
            return ["overlapped", "overlapped", "overlapped"];
          case "inherit":
            return ["inherit", "inherit", "inherit"];
          default:
            throw ERR_INVALID_OPT_VALUE("stdio", stdio);
        }
      } else if (Array.isArray(stdio)) {
        let processedStdio;
        if (stdio.length === 0)
          processedStdio = ["pipe", "pipe", "pipe"];
        else if (stdio.length === 1)
          processedStdio = [stdio[0], "pipe", "pipe"];
        else if (stdio.length === 2)
          processedStdio = [stdio[0], stdio[1], "pipe"];
        else if (stdio.length >= 3)
          processedStdio = stdio;
        return processedStdio;
      } else {
        throw ERR_INVALID_OPT_VALUE("stdio", stdio);
      }
    }
    function abortChildProcess(child, killSignal, reason) {
      if (!child)
        return;
      try {
        if (child.kill(killSignal)) {
          child.emit("error", makeAbortError(undefined, { cause: reason }));
        }
      } catch (err) {
        child.emit("error", err);
      }
    }

    class Control extends EventEmitter {
      constructor() {
        super();
      }
    }
    function validateMaxBuffer(maxBuffer) {
      if (maxBuffer != null && !(typeof maxBuffer === "number" && maxBuffer >= 0)) {
        throw codes.OUT_OF_RANGE("options.maxBuffer", "a positive number", maxBuffer);
      }
    }
    function validateArgumentNullCheck(arg, propName) {
      if (typeof arg === "string" && StringPrototypeIncludes.call(arg, "\x00")) {
        throw codes.INVALID_ARG_VALUE(propName, arg, "must be a string without null bytes");
      }
    }
    function validateArgumentsNullCheck(args, propName) {
      for (let i = 0;i < args.length; ++i) {
        validateArgumentNullCheck(args[i], `${propName}[${i}]`);
      }
    }
    function validateTimeout(timeout) {
      if (timeout != null && !(NumberIsInteger(timeout) && timeout >= 0)) {
        throw codes.OUT_OF_RANGE("timeout", "an unsigned integer", timeout);
      }
    }
    function isInt32(value) {
      return value === (value | 0);
    }
    function nullCheck(path, propName, throwError = true) {
      const pathIsString = typeof path === "string";
      const pathIsUint8Array = isUint8Array(path);
      if (!pathIsString && !pathIsUint8Array || pathIsString && !StringPrototypeIncludes.call(path, "\x00") || pathIsUint8Array && !Uint8ArrayPrototypeIncludes.call(path, 0)) {
        return;
      }
      const err = codes.INVALID_ARG_VALUE(propName, path, "must be a string or Uint8Array without null bytes");
      if (throwError) {
        throw err;
      }
      return err;
    }
    function validatePath(path, propName = "path") {
      if (typeof path !== "string" && !isUint8Array(path)) {
        throw codes.INVALID_ARG_TYPE(propName, ["string", "Buffer", "URL"], path);
      }
      const err = nullCheck(path, propName, false);
      if (err !== undefined) {
        throw err;
      }
    }
    function getValidatedPath(fileURLOrPath, propName = "path") {
      const path = toPathIfFileURL(fileURLOrPath);
      validatePath(path, propName);
      return path;
    }
    function isUint8Array(value) {
      return typeof value === "object" && value !== null && value instanceof Uint8Array;
    }
    function isURLInstance(fileURLOrPath) {
      return fileURLOrPath != null && fileURLOrPath.href && fileURLOrPath.origin;
    }
    function toPathIfFileURL(fileURLOrPath) {
      if (!isURLInstance(fileURLOrPath))
        return fileURLOrPath;
      return Bun.fileURLToPath(fileURLOrPath);
    }
    var Error = globalThis.Error;
    var TypeError = globalThis.TypeError;
    function genericNodeError(message, errorProperties) {
      const err = new Error(message);
      ObjectAssign(err, errorProperties);
      return err;
    }
    function ERR_UNKNOWN_SIGNAL(name) {
      const err = new TypeError(`Unknown signal: ${name}`);
      err.code = "ERR_UNKNOWN_SIGNAL";
      return err;
    }
    function ERR_INVALID_OPT_VALUE(name, value) {
      const err = new TypeError(`The value "${value}" is invalid for option "${name}"`);
      err.code = "ERR_INVALID_OPT_VALUE";
      return err;
    }

    class SystemError extends Error {
      path;
      syscall;
      errno;
      code;
      constructor(message, path, syscall, errno, code) {
        super(message);
        this.path = path;
        this.syscall = syscall;
        this.errno = errno;
        this.code = code;
      }
      get name() {
        return "SystemError";
      }
    }
    return {
      ChildProcess,
      spawn,
      execFile,
      exec,
      fork,
      spawnSync,
      execFileSync,
      execSync
    };
  })();
})
