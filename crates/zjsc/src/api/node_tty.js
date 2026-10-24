// GERADO por `scripts/gen-node-tty.js` a partir de `src/js/node/tty.ts` e `src/js/internal/tty.ts` do bun 1.4.2. Não editar à mão.
(function (host) {
  "use strict";
  const { validators, toClass } = host.internal;
  const internalTty = (() => {
    let OSRelease;
    const COLORS_2 = 1;
    const COLORS_16 = 4;
    const COLORS_256 = 8;
    const COLORS_16m = 24;
    const TERM_ENVS = {
      eterm: COLORS_16,
      cons25: COLORS_16,
      console: COLORS_16,
      cygwin: COLORS_16,
      dtterm: COLORS_16,
      gnome: COLORS_16,
      hurd: COLORS_16,
      jfbterm: COLORS_16,
      konsole: COLORS_16,
      kterm: COLORS_16,
      mlterm: COLORS_16,
      mosh: COLORS_16m,
      putty: COLORS_16,
      st: COLORS_16,
      "rxvt-unicode-24bit": COLORS_16m,
      terminator: COLORS_16m,
      "xterm-kitty": COLORS_16m
    };
    const CI_ENVS = {
      APPVEYOR: COLORS_256,
      BUILDKITE: COLORS_256,
      CIRCLECI: COLORS_16m,
      DRONE: COLORS_256,
      GITEA_ACTIONS: COLORS_16m,
      GITHUB_ACTIONS: COLORS_16m,
      GITLAB_CI: COLORS_256,
      TRAVIS: COLORS_256
    };
    const TERM_ENVS_REG_EXP = [
      /ansi/,
      /color/,
      /linux/,
      /direct/,
      /^con[0-9]*x[0-9]/,
      /^rxvt/,
      /^screen/,
      /^xterm/,
      /^vt100/,
      /^vt220/
    ];
    let warned = false;
    function warnOnDeactivatedColors(env) {
      if (warned)
        return;
      let name = "";
      if (env.NODE_DISABLE_COLORS !== undefined && env.NODE_DISABLE_COLORS !== "")
        name = "NODE_DISABLE_COLORS";
      if (env.NO_COLOR !== undefined && env.NO_COLOR !== "") {
        if (name !== "") {
          name += "' and '";
        }
        name += "NO_COLOR";
      }
      if (name !== "") {
        process.emitWarning(`The '${name}' env is ignored due to the 'FORCE_COLOR' env being set.`, "Warning");
        warned = true;
      }
    }
    function getColorDepth(env) {
      const FORCE_COLOR = env.FORCE_COLOR;
      if (FORCE_COLOR !== undefined) {
        switch (FORCE_COLOR) {
          case "":
          case "1":
          case "true":
            warnOnDeactivatedColors(env);
            return COLORS_16;
          case "2":
            warnOnDeactivatedColors(env);
            return COLORS_256;
          case "3":
            warnOnDeactivatedColors(env);
            return COLORS_16m;
          default:
            return COLORS_2;
        }
      }
      if (env.NODE_DISABLE_COLORS !== undefined && env.NODE_DISABLE_COLORS !== "" || env.NO_COLOR !== undefined && env.NO_COLOR !== "" || env.TERM === "dumb") {
        return COLORS_2;
      }
      if (process.platform === "win32") {
        if (OSRelease === undefined) {
          const { release } = require("node:os");
          OSRelease = release().split(".");
        }
        if (+OSRelease[0] >= 10) {
          const build = +OSRelease[2];
          if (build >= 14931)
            return COLORS_16m;
          if (build >= 10586)
            return COLORS_256;
        }
        return COLORS_16;
      }
      if (env.TMUX) {
        return COLORS_16m;
      }
      if ("TF_BUILD" in env && "AGENT_NAME" in env) {
        return COLORS_16;
      }
      if ("CI" in env) {
        for (const name in CI_ENVS) {
          if (name in env) {
            return CI_ENVS[name];
          }
        }
        if (env.CI_NAME === "codeship") {
          return COLORS_256;
        }
        return COLORS_2;
      }
      if ("TEAMCITY_VERSION" in env) {
        return /^(9\.(0*[1-9]\d*)\.|\d{2,}\.)/.test(env.TEAMCITY_VERSION) ? COLORS_16 : COLORS_2;
      }
      switch (env.TERM_PROGRAM) {
        case "iTerm.app":
          if (!env.TERM_PROGRAM_VERSION || /^[0-2]\./.test(env.TERM_PROGRAM_VERSION)) {
            return COLORS_256;
          }
          return COLORS_16m;
        case "HyperTerm":
        case "ghostty":
        case "WezTerm":
        case "MacTerm":
          return COLORS_16m;
        case "Apple_Terminal":
          return COLORS_256;
      }
      const COLORTERM = env.COLORTERM;
      if (COLORTERM === "truecolor" || COLORTERM === "24bit") {
        return COLORS_16m;
      }
      const TERM = env.TERM;
      if (TERM) {
        if (/truecolor/.test(TERM)) {
          return COLORS_16m;
        }
        if (TERM.startsWith("xterm-256")) {
          return COLORS_256;
        }
        const termEnv = TERM.toLowerCase();
        if (TERM_ENVS[termEnv]) {
          return TERM_ENVS[termEnv];
        }
        if (TERM_ENVS_REG_EXP.some((term) => term.test(termEnv))) {
          return COLORS_16;
        }
      }
      if (env.COLORTERM) {
        return COLORS_16;
      }
      return COLORS_2;
    }
    return { getColorDepth };

  })();
  const require = name => {
    if (name === "internal/validators") return validators;
    if (name === "internal/tty") return internalTty;
    if (name === "internal/fs/streams") return { get ReadStream() { return host.requireBuiltin("node:fs").ReadStream; }, get WriteStream() { return host.requireBuiltin("node:fs").WriteStream; } };
    return host.requireBuiltin(name);
  };
  return (() => {
    const {
      setRawMode: ttySetMode,
      isatty,
      getWindowSize: _getWindowSize,
      rawModeStateSize
    } = host.tty;
    const { validateInteger } = require("internal/validators");
    const fs = require("internal/fs/streams");
    const kRawModeState = Symbol("rawModeState");
    function ReadStream(fd) {
      if (!(this instanceof ReadStream)) {
        return new ReadStream(fd);
      }
      fs.ReadStream.apply(this, ["", { fd }]);
      this.isRaw = false;
      this.isTTY = isatty(fd);
    }
    toClass(ReadStream, "ReadStream", fs.ReadStream);
    Object.defineProperty(ReadStream, "prototype", {
      get() {
        const Prototype = Object.create(fs.ReadStream.prototype);
        Prototype.ref = function() {
          const source = undefined;
          if (source?.updateRef) {
            source.updateRef(true);
          }
          return this;
        };
        Prototype.unref = function() {
          const source = undefined;
          if (source?.updateRef) {
            source.updateRef(false);
          }
          return this;
        };
        Prototype.setRawMode = function(flag) {
          flag = !!flag;
          if (process.platform === "win32") {
            if (this.fd === 0) {
              const err = ttySetMode(flag);
              if (err) {
                this.emit("error", new Error("setRawMode failed with errno: " + err));
                return this;
              }
            } else {
              const handle = undefined;
              if (!handle) {
                this.emit("error", new Error("setRawMode failed because it was called on something that is not a TTY"));
                return this;
              }
              undefined;
              const err = handle.setRawMode(flag);
              if (err) {
                this.emit("error", err);
                return this;
              }
            }
          } else {
            const state = this[kRawModeState] ??= new Uint8Array(rawModeStateSize);
            const err = ttySetMode(this.fd, flag, state);
            if (err) {
              this.emit("error", new Error("setRawMode failed with errno: " + err));
              return this;
            }
          }
          this.isRaw = flag;
          return this;
        };
        Object.defineProperty(ReadStream, "prototype", { value: Prototype });
        return Prototype;
      },
      enumerable: true,
      configurable: true
    });
    function WriteStream(fd) {
      if (!(this instanceof WriteStream))
        return new WriteStream(fd);
      const stream = fs.WriteStream.call(this, null, { fd, $fastPath: true, autoClose: false });
      stream.columns = undefined;
      stream.rows = undefined;
      stream.isTTY = isatty(stream.fd);
      if (stream.isTTY) {
        const windowSizeArray = [0, 0];
        if (_getWindowSize(fd, windowSizeArray) === true) {
          stream.columns = windowSizeArray[0];
          stream.rows = windowSizeArray[1];
        }
      }
      return stream;
    }
    Object.defineProperty(WriteStream, "prototype", {
      get() {
        const Real = fs.WriteStream.prototype;
        Object.defineProperty(WriteStream, "prototype", { value: Real });
        WriteStream.prototype._refreshSize = function() {
          const oldCols = this.columns;
          const oldRows = this.rows;
          const windowSizeArray = [0, 0];
          if (_getWindowSize(this.fd, windowSizeArray) === true) {
            if (oldCols !== windowSizeArray[0] || oldRows !== windowSizeArray[1]) {
              this.columns = windowSizeArray[0];
              this.rows = windowSizeArray[1];
              this.emit("resize");
            }
          }
        };
        WriteStream.prototype.clearLine = function(dir, cb) {
          return require("node:readline").clearLine(this, dir, cb);
        };
        WriteStream.prototype.clearScreenDown = function(cb) {
          return require("node:readline").clearScreenDown(this, cb);
        };
        WriteStream.prototype.cursorTo = function(x, y, cb) {
          return require("node:readline").cursorTo(this, x, y, cb);
        };
        WriteStream.prototype.getColorDepth = function(env = process.env) {
          return require("internal/tty").getColorDepth(env);
        };
        WriteStream.prototype.getWindowSize = function() {
          return [this.columns, this.rows];
        };
        WriteStream.prototype.hasColors = function(count, env) {
          if (env === undefined && (count === undefined || typeof count === "object" && count !== null)) {
            env = count;
            count = 16;
          } else {
            validateInteger(count, "count", 2);
          }
          return count <= 2 ** this.getColorDepth(env);
        };
        WriteStream.prototype.moveCursor = function(dx, dy, cb) {
          return require("node:readline").moveCursor(this, dx, dy, cb);
        };
        WriteStream.prototype[Symbol.asyncIterator] = function() {
          return async function* () {}();
        };
        return Real;
      },
      enumerable: true,
      configurable: true
    });
    return { ReadStream, WriteStream, isatty };

  })();
})
