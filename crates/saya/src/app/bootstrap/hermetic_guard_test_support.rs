use std::cell::Cell;

thread_local! {
    /// このスレッドで「ホーム解決経路を踏んだら隔離漏れとして失敗させる」ガードが
    /// 有効かどうか。bootstrap の密閉テストだけがこれを有効化し、`ConfigSource::Default`
    /// が実ホーム解決へ落ちた瞬間にパニックさせる。main.rs などスコープ外のテストには影響しない。
    static ACTIVE: Cell<bool> = const { Cell::new(false) };
}

/// ガードを有効化し、Drop で元の状態へ戻す RAII ハンドル。
pub(in crate::app::bootstrap) struct GuardScope {
    previous: bool,
}

impl GuardScope {
    pub(in crate::app::bootstrap) fn activate() -> Self {
        let previous = ACTIVE.with(|active| active.replace(true));
        Self { previous }
    }
}

impl Drop for GuardScope {
    fn drop(&mut self) {
        let previous = self.previous;
        ACTIVE.with(|active| active.set(previous));
    }
}

pub(super) fn is_active() -> bool {
    ACTIVE.with(|active| active.get())
}
