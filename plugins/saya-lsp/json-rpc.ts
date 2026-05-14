// LSP の JSON-RPC framing / parser / client。
//
// 名前空間規約: top-level に '__lspJsonRpc' 1 個のみ。
// 依存: '__lspUtf8'（先に './utf8.ts' が inline 展開されている前提）。
//
// 規約: 起動時 transpiler 'strip_type_annotations' は object literal の
// 'key: identifierExpr' を「型注釈」と誤判定し値を削除する。そのため
// object literal で動的値を持たせる箇所はすべて shorthand
// ('{ params }') で書く。shorthand にできない場面は property assignment
// で構築する。
//
// 公開 API -
// - encodeMessageBytes(message): Uint8Array
//     LSP wire format（'Content-Length: N\r\n\r\n<body>'）の UTF-8
//     バイト列に変換する。
// - createBytesParser(onMessage, onError?): { accept(Uint8Array): void }
//     stdout から来るバイトを受け取り、framing を解いた JSON を
//     'onMessage(parsedJson)' で通知する。
// - createCancelToken(): { signal, cancel(reason) }
//     deno_core に AbortController が無いため、自前 signal-like を提供。
// - createClient(transport, options?): { request, notify, dispose }
//     JSON-RPC 2.0 クライアント。pending Map / cancel / timeout を内蔵。
const __lspJsonRpc = {
  encodeMessageBytes(message) {
    const body = JSON.stringify(message);
    const bodyBytes = __lspUtf8.encodeBytes(body);
    const header = "Content-Length: " + bodyBytes.length + "\r\n\r\n";
    const headerBytes = new Uint8Array(header.length);
    for (let i = 0; i < header.length; i = i + 1) {
      headerBytes[i] = header.charCodeAt(i) & 0xff;
    }
    const out = new Uint8Array(headerBytes.length + bodyBytes.length);
    out.set(headerBytes, 0);
    out.set(bodyBytes, headerBytes.length);
    return out;
  },

  buildRequestMessage(id, method, params) {
    // object literal の 'params: ident' 形式を避けるため shorthand と
    // property assignment で構築する。
    const safeParams = params === undefined ? null : params;
    const msg = { jsonrpc: "2.0" };
    msg.id = id;
    msg.method = method;
    msg.params = safeParams;
    return msg;
  },

  buildNotificationMessage(method, params) {
    const safeParams = params === undefined ? null : params;
    const msg = { jsonrpc: "2.0" };
    msg.method = method;
    msg.params = safeParams;
    return msg;
  },

  buildSuccessResponseMessage(id, result) {
    const safeResult = result === undefined ? null : result;
    const msg = { jsonrpc: "2.0" };
    msg.id = id;
    msg.result = safeResult;
    return msg;
  },

  buildErrorResponseMessage(id, error) {
    const msg = { jsonrpc: "2.0" };
    msg.id = id;
    msg.error = error;
    return msg;
  },

  buildCancelNotificationMessage(id) {
    const params = { id };
    const msg = { jsonrpc: "2.0" };
    msg.method = "$/cancelRequest";
    msg.params = params;
    return msg;
  },

  createBytesParser(onMessage, onError) {
    if (typeof onMessage !== "function") {
      throw new TypeError("createBytesParser requires onMessage callback");
    }
    const reportError =
      typeof onError === "function"
        ? onError
        : function defaultThrow(err) {
            throw err;
          };
    let buffer = new Uint8Array(0);
    function appendChunk(chunk) {
      if (!(chunk instanceof Uint8Array)) {
        throw new TypeError("parser accept expects Uint8Array");
      }
      const next = new Uint8Array(buffer.length + chunk.length);
      next.set(buffer, 0);
      next.set(chunk, buffer.length);
      buffer = next;
    }
    function findHeaderEnd() {
      for (let i = 0; i + 3 < buffer.length; i = i + 1) {
        if (
          buffer[i] === 0x0d &&
          buffer[i + 1] === 0x0a &&
          buffer[i + 2] === 0x0d &&
          buffer[i + 3] === 0x0a
        ) {
          return i;
        }
      }
      return -1;
    }
    function parseContentLength(headerText) {
      const lines = headerText.split("\r\n");
      for (let i = 0; i < lines.length; i = i + 1) {
        const line = lines[i];
        const lower = line.toLowerCase();
        if (lower.startsWith("content-length")) {
          const colon = line.indexOf(":");
          if (colon < 0) {
            return null;
          }
          const raw = line.slice(colon + 1).trim();
          const parsed = Number(raw);
          if (Number.isFinite(parsed) && parsed >= 0) {
            return parsed;
          }
          return null;
        }
      }
      return null;
    }
    function headerSlice(end) {
      let text = "";
      for (let i = 0; i < end; i = i + 1) {
        text = text + String.fromCharCode(buffer[i]);
      }
      return text;
    }
    function consume(count) {
      buffer = buffer.slice(count);
    }
    const accepting = {};
    accepting.accept = function (chunk) {
      appendChunk(chunk);
      while (true) {
        const headerEnd = findHeaderEnd();
        if (headerEnd < 0) {
          return;
        }
        const headerText = headerSlice(headerEnd);
        const contentLength = parseContentLength(headerText);
        if (contentLength === null) {
          reportError(new Error("LSP message is missing Content-Length header"));
          consume(headerEnd + 4);
          continue;
        }
        const bodyStart = headerEnd + 4;
        if (buffer.length < bodyStart + contentLength) {
          return;
        }
        const bodyBytes = buffer.slice(bodyStart, bodyStart + contentLength);
        consume(bodyStart + contentLength);
        let parsed = null;
        let parseError = null;
        try {
          const text = __lspUtf8.decodeBytes(bodyBytes);
          parsed = JSON.parse(text);
        } catch (err) {
          parseError = err instanceof Error ? err : new Error(String(err));
        }
        if (parseError !== null) {
          reportError(parseError);
          continue;
        }
        try {
          onMessage(parsed);
        } catch (handlerError) {
          reportError(
            handlerError instanceof Error
              ? handlerError
              : new Error(String(handlerError)),
          );
        }
      }
    };
    return accepting;
  },

  createCancelToken() {
    // deno_core ランタイムには AbortController / AbortSignal が無い。
    // 同等インタフェース（aborted / reason / addEventListener("abort")
    // / removeEventListener）を最小限自前で提供する。
    const listeners = [];
    const signal = {
      aborted: false,
      reason: undefined,
    };
    signal.addEventListener = function (eventName, listener) {
      if (eventName !== "abort") {
        return;
      }
      if (typeof listener !== "function") {
        return;
      }
      listeners.push(listener);
    };
    signal.removeEventListener = function (eventName, listener) {
      if (eventName !== "abort") {
        return;
      }
      for (let i = listeners.length - 1; i >= 0; i = i - 1) {
        if (listeners[i] === listener) {
          listeners.splice(i, 1);
        }
      }
    };
    function cancel(reason) {
      if (signal.aborted) {
        return;
      }
      signal.aborted = true;
      signal.reason = reason;
      const snapshot = listeners.slice();
      listeners.length = 0;
      const event = { type: "abort" };
      event.target = signal;
      for (let i = 0; i < snapshot.length; i = i + 1) {
        try {
          snapshot[i](event);
        } catch (_err) {
          // listener エラーは伝播させない（cancel 経路の堅牢性のため）
        }
      }
    }
    const token = {};
    token.signal = signal;
    token.cancel = cancel;
    return token;
  },

  createClient(transport, options) {
    if (!transport || typeof transport.writeBytes !== "function") {
      throw new TypeError("createClient requires a transport with writeBytes");
    }
    const opts = options || {};
    const requestTimeoutMs =
      typeof opts.requestTimeoutMs === "number" ? opts.requestTimeoutMs : 30000;
    const onNotification =
      typeof opts.onNotification === "function" ? opts.onNotification : null;
    const onServerRequest =
      typeof opts.onServerRequest === "function" ? opts.onServerRequest : null;
    const log = typeof opts.log === "function" ? opts.log : function () {};

    const state = {
      nextId: 1,
      disposed: false,
      closeReason: null,
    };
    // transpiler の 'strip_type_annotations' が 'key: new X()' を
    // 型注釈と誤判定するため、Map 初期化は property assignment で行う。
    state.pending = new Map();

    function cleanupWaiter(waiter) {
      if (waiter.timeoutHandle !== null && typeof clearTimeout === "function") {
        clearTimeout(waiter.timeoutHandle);
        waiter.timeoutHandle = null;
      }
      if (waiter.externalSignal && waiter.signalAbortListener) {
        try {
          waiter.externalSignal.removeEventListener("abort", waiter.signalAbortListener);
        } catch (_err) {
          // 互換 signal が removeEventListener を持たない場合に備える
        }
        waiter.signalAbortListener = null;
      }
    }

    function dispatchIncoming(message) {
      if (!message || typeof message !== "object") {
        return;
      }
      const hasId =
        Object.prototype.hasOwnProperty.call(message, "id") &&
        message.id !== null &&
        message.id !== undefined;
      const isResponse =
        hasId &&
        (Object.prototype.hasOwnProperty.call(message, "result") ||
          Object.prototype.hasOwnProperty.call(message, "error"));
      if (isResponse) {
        const waiter = state.pending.get(message.id);
        if (!waiter) {
          log("dropped response for unknown id: " + String(message.id));
          return;
        }
        state.pending.delete(message.id);
        cleanupWaiter(waiter);
        if (message.error) {
          waiter.reject(message.error);
        } else {
          const result = Object.prototype.hasOwnProperty.call(message, "result")
            ? message.result
            : null;
          waiter.resolve(result);
        }
        return;
      }
      if (typeof message.method === "string" && hasId) {
        if (onServerRequest !== null) {
          Promise.resolve()
            .then(function () {
              return onServerRequest(message);
            })
            .then(function (result) {
              return transport.writeBytes(
                __lspJsonRpc.encodeMessageBytes(
                  __lspJsonRpc.buildSuccessResponseMessage(message.id, result),
                ),
              );
            })
            .catch(function (err) {
              const errMessage = err && err.message ? String(err.message) : String(err);
              const error = { code: -32603 };
              error.message = errMessage;
              return transport.writeBytes(
                __lspJsonRpc.encodeMessageBytes(
                  __lspJsonRpc.buildErrorResponseMessage(message.id, error),
                ),
              );
            });
        } else {
          const error = {
            code: -32601,
            message: "Method not found: " + String(message.method),
          };
          transport.writeBytes(
            __lspJsonRpc.encodeMessageBytes(
              __lspJsonRpc.buildErrorResponseMessage(message.id, error),
            ),
          );
        }
        return;
      }
      if (typeof message.method === "string") {
        if (onNotification !== null) {
          try {
            onNotification(message);
          } catch (err) {
            log(
              "notification handler error: " +
                (err && err.message ? err.message : String(err)),
            );
          }
        }
        return;
      }
    }

    const parser = __lspJsonRpc.createBytesParser(dispatchIncoming, function (err) {
      log("parser error: " + (err && err.message ? err.message : String(err)));
    });
    if (typeof transport.onBytes === "function") {
      transport.onBytes(function (chunk) {
        parser.accept(chunk);
      });
    }
    if (typeof transport.onClose === "function") {
      transport.onClose(function (reason) {
        if (state.disposed) {
          return;
        }
        state.disposed = true;
        state.closeReason = reason == null ? "transport closed" : String(reason);
        const rejectReason = new Error("LSP transport closed: " + state.closeReason);
        for (const waiter of state.pending.values()) {
          cleanupWaiter(waiter);
          waiter.reject(rejectReason);
        }
        state.pending.clear();
      });
    }

    const client = {};
    client.request = function (method, params, requestOptions) {
      if (state.disposed) {
        return Promise.reject(
          new Error("LSP client disposed: " + (state.closeReason || "unknown")),
        );
      }
      const id = state.nextId;
      state.nextId = state.nextId + 1;
      const externalSignal =
        requestOptions && requestOptions.signal ? requestOptions.signal : null;
      const timeoutMs =
        requestOptions && typeof requestOptions.timeoutMs === "number"
          ? requestOptions.timeoutMs
          : requestTimeoutMs;

      return new Promise(function (resolve, reject) {
        const waiter = {
          resolve,
          reject,
          externalSignal,
          signalAbortListener: null,
          timeoutHandle: null,
        };

        function cancel(reason) {
          if (!state.pending.has(id)) {
            return;
          }
          state.pending.delete(id);
          cleanupWaiter(waiter);
          transport
            .writeBytes(
              __lspJsonRpc.encodeMessageBytes(
                __lspJsonRpc.buildCancelNotificationMessage(id),
              ),
            )
            .catch(function (_err) {
              // cancel 通知の write 失敗は無視（transport が既に死んでいる場合）
            });
          const rejectError =
            reason instanceof Error
              ? reason
              : new Error(
                  typeof reason === "string" ? reason : "LSP request cancelled",
                );
          reject(rejectError);
        }

        if (externalSignal) {
          if (externalSignal.aborted) {
            cancel(externalSignal.reason);
            return;
          }
          const abortListener = function () {
            cancel(externalSignal.reason);
          };
          waiter.signalAbortListener = abortListener;
          externalSignal.addEventListener("abort", abortListener);
        }

        if (timeoutMs > 0 && typeof setTimeout === "function") {
          waiter.timeoutHandle = setTimeout(function () {
            cancel(new Error("LSP request timed out after " + timeoutMs + "ms"));
          }, timeoutMs);
        }

        state.pending.set(id, waiter);
        transport
          .writeBytes(
            __lspJsonRpc.encodeMessageBytes(
              __lspJsonRpc.buildRequestMessage(id, method, params),
            ),
          )
          .catch(function (err) {
            if (state.pending.has(id)) {
              state.pending.delete(id);
              cleanupWaiter(waiter);
              reject(err);
            }
          });
      });
    };

    client.notify = function (method, params) {
      if (state.disposed) {
        return Promise.reject(new Error("LSP client disposed"));
      }
      return transport.writeBytes(
        __lspJsonRpc.encodeMessageBytes(
          __lspJsonRpc.buildNotificationMessage(method, params),
        ),
      );
    };

    client.dispose = function () {
      if (state.disposed) {
        return;
      }
      state.disposed = true;
      state.closeReason = "explicit dispose";
      const rejectError = new Error("LSP client disposed");
      for (const waiter of state.pending.values()) {
        cleanupWaiter(waiter);
        waiter.reject(rejectError);
      }
      state.pending.clear();
    };

    return client;
  },
};
