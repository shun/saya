// LSP セッションオーケストレータ。
//
// 名前空間規約: top-level に '__lspSession' 1 個のみ。
// 依存 -
//   - '__lspJsonRpc'（client + cancel token）
//   - '__lspLifecycle'（状態機械 + queue）
//
// 公開 API -
// - create(transport, options): {
//     state(),
//     start(initializeParams): Promise<initializeResult>,
//     request(method, params, requestOptions?): Promise<unknown>,
//     notify(method, params): Promise<void>,
//     shutdown(): Promise<void>,
//     close(reason?),
//     onNotification(handler),
//   }
//
// 振る舞い -
// - 'start(initializeParams)' で 'initialize' 要求を送信し、capability
//   応答を受けたら 'initialized' 通知を送って 'ready' に遷移する。
//   戻り値は サーバの initialize result。
// - 'start' 完了前の 'request' は queue に積まれ、'ready' 到達時に flush。
// - 'shutdown' は 'shutdown' request → 'exit' 通知 → lifecycle 遷移
//   ('ready → shuttingDown → exited') を行う。
// - transport の 'onClose' が発火したら lifecycle を 'exited' に遷移して
//   queue を reject する。
const __lspSession = {
  create(transport, options) {
    if (!transport) {
      throw new TypeError("session.create requires a transport");
    }
    const opts = options || {};
    const log = typeof opts.log === "function" ? opts.log : function () {};

    const lifecycle = __lspLifecycle.create();

    const state = {
      notificationHandler: null,
      serverRequestHandler: null,
    };

    function onNotification(message) {
      if (state.notificationHandler !== null) {
        try {
          state.notificationHandler(message);
        } catch (err) {
          log(
            "session notification handler threw: " +
              (err && err.message ? err.message : String(err)),
          );
        }
      }
    }

    const clientOptions = {};
    clientOptions.onNotification = onNotification;
    clientOptions.log = log;
    if (typeof opts.requestTimeoutMs === "number") {
      clientOptions.requestTimeoutMs = opts.requestTimeoutMs;
    }
    const client = __lspJsonRpc.createClient(transport, clientOptions);

    // transport close で lifecycle も exited にする
    if (typeof transport.onClose === "function") {
      transport.onClose(function (reason) {
        if (lifecycle.state() !== "exited") {
          try {
            lifecycle.close(reason);
          } catch (err) {
            log(
              "session close error: " +
                (err && err.message ? err.message : String(err)),
            );
          }
        }
      });
    }

    function sendInitialize(initializeParams) {
      // initialize は他の request と違って lifecycle queue を通さず直接送る
      return client.request("initialize", initializeParams);
    }

    function sendInitialized() {
      // notification なので応答を待たない
      return client.notify("initialized", {});
    }

    function sendShutdown() {
      return client.request("shutdown", null);
    }

    function sendExit() {
      return client.notify("exit", null);
    }

    const api = {};
    api.state = function () {
      return lifecycle.state();
    };
    api.onNotification = function (handler) {
      if (typeof handler !== "function") {
        throw new TypeError("onNotification expects a function");
      }
      state.notificationHandler = handler;
    };
    api.start = async function (initializeParams) {
      const currentState = lifecycle.state();
      if (currentState !== "idle") {
        throw new Error("session.start requires 'idle' state, got " + currentState);
      }
      lifecycle.transition("initializing");
      let initializeResult = null;
      try {
        initializeResult = await sendInitialize(initializeParams);
      } catch (err) {
        // initialize 失敗 → exited に遷移して queue を reject
        lifecycle.transition("exited");
        throw err;
      }
      // initialize 成功 → initialized 通知 → ready
      try {
        await sendInitialized();
      } catch (err) {
        log(
          "session initialized notification failed: " +
            (err && err.message ? err.message : String(err)),
        );
      }
      lifecycle.transition("ready");
      return initializeResult;
    };
    api.request = function (method, params, requestOptions) {
      const currentState = lifecycle.state();
      if (currentState === "exited") {
        return Promise.reject(new Error("session is exited; cannot send " + method));
      }
      if (currentState === "ready") {
        return client.request(method, params, requestOptions);
      }
      // initialize 中など → queue
      return lifecycle.runWhenReady(function () {
        return client.request(method, params, requestOptions);
      });
    };
    api.notify = function (method, params) {
      const currentState = lifecycle.state();
      if (currentState === "exited") {
        return Promise.reject(new Error("session is exited; cannot notify " + method));
      }
      if (currentState === "ready") {
        return client.notify(method, params);
      }
      return lifecycle.runWhenReady(function () {
        return client.notify(method, params);
      });
    };
    api.shutdown = async function () {
      const currentState = lifecycle.state();
      if (currentState === "exited") {
        return;
      }
      if (currentState !== "ready") {
        // ready でない場合は強制 exit のみ
        lifecycle.close("shutdown before ready");
        client.dispose();
        if (typeof transport.close === "function") {
          try {
            transport.close("session shutdown");
          } catch (_err) {
            // 無視
          }
        }
        return;
      }
      lifecycle.transition("shuttingDown");
      try {
        await sendShutdown();
      } catch (err) {
        log(
          "session shutdown request failed: " +
            (err && err.message ? err.message : String(err)),
        );
      }
      try {
        await sendExit();
      } catch (err) {
        log(
          "session exit notification failed: " +
            (err && err.message ? err.message : String(err)),
        );
      }
      lifecycle.transition("exited");
      client.dispose();
      if (typeof transport.close === "function") {
        try {
          transport.close("session shutdown");
        } catch (_err) {
          // 無視
        }
      }
    };
    api.close = function (reason) {
      const currentState = lifecycle.state();
      if (currentState === "exited") {
        return;
      }
      lifecycle.close(reason == null ? "session close" : reason);
      client.dispose();
      if (typeof transport.close === "function") {
        try {
          transport.close(reason);
        } catch (_err) {
          // 無視
        }
      }
    };
    return api;
  },
};
