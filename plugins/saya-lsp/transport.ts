// LSP の transport 層。
//
// 名前空間規約: top-level に '__lspTransport' 1 個のみ。
// 依存: なし（依存があるのは利用側）。
//
// 公開 API -
// - createInMemoryPair(): { client, server }
//     テスト用に双方向接続された 2 つの transport を返す。client 側で
//     'writeBytes(uint8)' を呼ぶと server 側の 'onBytes' ハンドラに
//     即座に配送される（同期 microtask）。pending pool / framing /
//     状態機械は呼出側で扱う。
// - createFromProcess(processHandle, options?): transport
//     'saya.process.spawn(...)' で得たハンドルを transport に変換する。
//     stdout を非同期 read loop で吸い出し 'onBytes' ハンドラへ配送し、
//     stderr は診断ログとして 'log' callback に流す。EOF / 異常終了で
//     'onClose' ハンドラが発火する。stdin 書き込みは順序保証された
//     promise チェーンで直列化する。
//
// 共通 transport インタフェース -
// - writeBytes(uint8: Uint8Array): Promise<number | void>
// - onBytes(handler: (chunk: Uint8Array) => void): void
// - onClose(handler: (reason: unknown) => void): void
// - close(reason?: unknown): void
const __lspTransport = {
  createInMemoryPair() {
    const peers = [{}, {}];
    for (let i = 0; i < 2; i = i + 1) {
      const peer = peers[i];
      peer.bytesHandler = null;
      peer.closeHandler = null;
      peer.closed = false;
      peer.closeReason = null;
    }

    function makeEndpoint(self, other) {
      const endpoint = {};
      endpoint.writeBytes = function (uint8) {
        if (self.closed) {
          return Promise.reject(
            new Error(
              "in-memory transport closed: " +
                (self.closeReason == null ? "unknown" : String(self.closeReason)),
            ),
          );
        }
        if (!(uint8 instanceof Uint8Array)) {
          return Promise.reject(new TypeError("writeBytes expects Uint8Array"));
        }
        // peer の onBytes を同期 microtask で呼ぶ（順序保証）。
        return Promise.resolve().then(function () {
          if (other.bytesHandler !== null) {
            other.bytesHandler(uint8);
          }
          return uint8.length;
        });
      };
      endpoint.onBytes = function (handler) {
        if (typeof handler !== "function") {
          throw new TypeError("onBytes expects a function");
        }
        self.bytesHandler = handler;
      };
      endpoint.onClose = function (handler) {
        if (typeof handler !== "function") {
          throw new TypeError("onClose expects a function");
        }
        self.closeHandler = handler;
        // close が既に発火していたら即座に通知する
        if (self.closed) {
          try {
            handler(self.closeReason);
          } catch (_err) {
            // ハンドラ例外は伝播させない
          }
        }
      };
      endpoint.close = function (reason) {
        if (self.closed) {
          return;
        }
        self.closed = true;
        self.closeReason = reason == null ? "closed" : reason;
        // 双方の close ハンドラを発火する
        if (self.closeHandler !== null) {
          try {
            self.closeHandler(self.closeReason);
          } catch (_err) {
            // 無視
          }
        }
        if (!other.closed) {
          other.closed = true;
          other.closeReason = self.closeReason;
          if (other.closeHandler !== null) {
            try {
              other.closeHandler(other.closeReason);
            } catch (_err) {
              // 無視
            }
          }
        }
      };
      return endpoint;
    }

    const result = {};
    result.client = makeEndpoint(peers[0], peers[1]);
    result.server = makeEndpoint(peers[1], peers[0]);
    return result;
  },

  createFromProcess(processHandle, options) {
    if (!processHandle || typeof processHandle.stdin !== "object") {
      throw new TypeError("createFromProcess requires a saya.process handle");
    }
    const opts = options || {};
    const readBufferSize =
      typeof opts.readBufferSize === "number" ? opts.readBufferSize : 65536;
    const log = typeof opts.log === "function" ? opts.log : function () {};

    const state = {
      closed: false,
      closeReason: null,
      bytesHandler: null,
      closeHandler: null,
    };
    // writeBytes を直列化する promise tail。'writeTail: Promise.resolve()'
    // を object literal で書くと strip_type_annotations に消されるため
    // property assignment で初期化する。
    state.writeTail = Promise.resolve();

    function fireClose(reason) {
      if (state.closed) {
        return;
      }
      state.closed = true;
      state.closeReason = reason;
      if (state.closeHandler !== null) {
        try {
          state.closeHandler(reason);
        } catch (_err) {
          // 無視
        }
      }
    }

    // stdout reader loop: 64KB buffer を使い回して read → onBytes へ配送。
    async function readStdoutLoop() {
      const buf = new Uint8Array(readBufferSize);
      try {
        while (!state.closed) {
          const n = await processHandle.stdout.read(buf);
          if (n === null) {
            log("[lsp-transport] stdout EOF");
            fireClose("stdout EOF");
            return;
          }
          if (state.bytesHandler !== null && n > 0) {
            // バッファのコピーを作って配送する（読み手は次の read で
            // バッファを上書きされるため）。
            const copy = new Uint8Array(n);
            copy.set(buf.subarray(0, n));
            try {
              state.bytesHandler(copy);
            } catch (handlerError) {
              log(
                "[lsp-transport] onBytes handler threw: " +
                  (handlerError && handlerError.message
                    ? handlerError.message
                    : String(handlerError)),
              );
            }
          }
        }
      } catch (err) {
        const message = err && err.message ? err.message : String(err);
        log("[lsp-transport] stdout reader error: " + message);
        fireClose("stdout reader error: " + message);
      }
    }

    // stderr reader loop: 内容を log callback に渡すだけ。
    async function readStderrLoop() {
      const buf = new Uint8Array(readBufferSize);
      try {
        while (!state.closed) {
          const n = await processHandle.stderr.read(buf);
          if (n === null) {
            return;
          }
          if (n > 0) {
            // stderr は通常 ASCII なので charCode で十分。
            let text = "";
            for (let i = 0; i < n; i = i + 1) {
              text = text + String.fromCharCode(buf[i]);
            }
            log("[lsp-transport][stderr] " + text);
          }
        }
      } catch (err) {
        log(
          "[lsp-transport] stderr reader error: " +
            (err && err.message ? err.message : String(err)),
        );
      }
    }

    // reader loops を起動する（fire-and-forget）。
    readStdoutLoop();
    readStderrLoop();

    const endpoint = {};
    endpoint.writeBytes = function (uint8) {
      if (state.closed) {
        return Promise.reject(
          new Error(
            "lsp-transport closed: " +
              (state.closeReason == null ? "unknown" : String(state.closeReason)),
          ),
        );
      }
      if (!(uint8 instanceof Uint8Array)) {
        return Promise.reject(new TypeError("writeBytes expects Uint8Array"));
      }
      const tail = state.writeTail;
      const next = tail.then(function () {
        return processHandle.stdin.write(uint8);
      });
      state.writeTail = next.catch(function () {
        // tail のエラーは握り潰す（次の write 自体は新たに走らせる）
      });
      return next;
    };
    endpoint.onBytes = function (handler) {
      if (typeof handler !== "function") {
        throw new TypeError("onBytes expects a function");
      }
      state.bytesHandler = handler;
    };
    endpoint.onClose = function (handler) {
      if (typeof handler !== "function") {
        throw new TypeError("onClose expects a function");
      }
      state.closeHandler = handler;
      if (state.closed) {
        try {
          handler(state.closeReason);
        } catch (_err) {
          // 無視
        }
      }
    };
    endpoint.close = function (reason) {
      if (state.closed) {
        return;
      }
      const cleanReason = reason == null ? "closed" : reason;
      // 順序: state を closed にして reader loop を抜けさせ、kill を呼ぶ
      fireClose(cleanReason);
      // 子プロセス kill は fire-and-forget（既に死んでいる可能性あり）
      try {
        processHandle.kill().catch(function () {
          // 無視
        });
      } catch (_err) {
        // kill が同期的に投げた場合も無視
      }
    };
    return endpoint;
  },
};
