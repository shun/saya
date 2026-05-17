// deno-fmt-ignore-file

// LSP セッションの lifecycle 状態機械。
//
// 名前空間規約: top-level に '__lspLifecycle' 1 個のみ。
// 依存: なし。
//
// 公開 API -
// - create(): { state(), transition(target), runWhenReady(work), close(reason) }
//
// 状態遷移グラフ（一方向） -
//   idle → initializing → ready → shuttingDown → exited
//
// 'runWhenReady(work)' -
//   - 'ready' 到達前は queue に積む。'ready' 到達時に挿入順に flush する。
//   - 既に 'ready' の場合は次の microtask で即座に実行。
//   - 'exited' に至った場合、未実行の work の戻り値 promise は reject する。
//   - work は同期関数でも async でも良い。戻り値が promise なら chained。
const __lspLifecycle = {
  create() {
    // 許可される直接遷移を '<from>:<to>' で表現する。
    const allowed = {
      "idle:initializing": true,
      "initializing:ready": true,
      "initializing:exited": true,
      "ready:shuttingDown": true,
      "ready:exited": true,
      "shuttingDown:exited": true,
    };

    const session = {
      current: "idle",
      pendingQueue: [],
    };

    function assertAllowed(from, to) {
      const key = from + ":" + to;
      if (allowed[key] !== true) {
        throw new Error(
          "invalid lifecycle transition: " + from + " -> " + to,
        );
      }
    }

    function flushQueueOnReady() {
      const snapshot = session.pendingQueue.slice();
      session.pendingQueue.length = 0;
      for (let i = 0; i < snapshot.length; i = i + 1) {
        const entry = snapshot[i];
        Promise.resolve()
          .then(function () {
            return entry.work();
          })
          .then(function (value) {
            entry.resolve(value);
          })
          .catch(function (err) {
            entry.reject(err);
          });
      }
    }

    function rejectQueue(reason) {
      const snapshot = session.pendingQueue.slice();
      session.pendingQueue.length = 0;
      const rejectError =
        reason instanceof Error
          ? reason
          : new Error(
              "LSP lifecycle exited before reaching ready: " +
                (reason == null ? "unknown" : String(reason)),
            );
      for (let i = 0; i < snapshot.length; i = i + 1) {
        snapshot[i].reject(rejectError);
      }
    }

    const api = {};
    api.state = function () {
      return session.current;
    };
    api.transition = function (target) {
      assertAllowed(session.current, target);
      const previous = session.current;
      session.current = target;
      if (target === "ready" && previous === "initializing") {
        flushQueueOnReady();
      }
      if (target === "exited") {
        // 未処理 work を reject する（reach 'ready' する前に死んだ場合）
        if (session.pendingQueue.length > 0) {
          rejectQueue("server exited");
        }
      }
    };
    api.runWhenReady = function (work) {
      if (typeof work !== "function") {
        throw new TypeError("runWhenReady expects a function");
      }
      if (session.current === "ready") {
        return Promise.resolve().then(function () {
          return work();
        });
      }
      if (session.current === "exited") {
        return Promise.reject(new Error("LSP lifecycle is exited; cannot run new work"));
      }
      return new Promise(function (resolve, reject) {
        const entry = {
          work,
          resolve,
          reject,
        };
        session.pendingQueue.push(entry);
      });
    };
    api.close = function (reason) {
      if (session.current === "exited") {
        return;
      }
      // exited に直接遷移する経路を許す（緊急停止）
      session.current = "exited";
      if (session.pendingQueue.length > 0) {
        rejectQueue(reason == null ? "lifecycle closed" : reason);
      }
    };
    return api;
  },
};
