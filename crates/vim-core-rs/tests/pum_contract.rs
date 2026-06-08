use std::fs;
use std::sync::{Mutex, MutexGuard, OnceLock};
use vim_core_rs::VimCoreSession;

fn session_test_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

fn acquire_session_test_lock() -> MutexGuard<'static, ()> {
    session_test_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn eval_string(session: &mut VimCoreSession, expr: &str) -> String {
    session
        .eval_string(expr)
        .unwrap_or_else(|| panic!("eval_string failed for expression: {expr}"))
}

// Upstream: test_ins_complete.vim / test_popup.vim
#[test]
fn buffer_keyword_completion_payload_parity_from_upstream_cases() {
    let _guard = acquire_session_test_lock();
    let mut session =
        VimCoreSession::new("alpha\nalphabet\nalpine\nbeta\n").expect("session should initialize");

    session.execute_ex_command("set complete=.").expect(
        "buffer-local keyword completion source should be restricted to the current buffer",
    );
    session
        .execute_ex_command("set completeopt=menuone,noinsert,noselect")
        .expect("completion options should be set");

    let completion_tx = session
        .execute_normal_command("Goal\x18\x0e")
        .expect("buffer keyword completion should start");
    let pum = completion_tx
        .snapshot
        .pum
        .as_ref()
        .expect("pum should exist for keyword completion");

    assert_eq!(pum.selected_index, None);
    assert_eq!(
        eval_string(&mut session, "string(complete_info(['mode']).mode)"),
        "'keyword'"
    );
    assert_eq!(
        eval_string(&mut session, "string(complete_info(['selected']).selected)"),
        "-1"
    );
    assert_eq!(
        eval_string(&mut session, "string(len(complete_info(['items']).items))"),
        pum.items.len().to_string()
    );
    assert!(pum.row > 0);
    assert!(pum.col > 0);
    assert!(pum.width > 0);
    assert!(pum.height > 0);

    let pum_words = pum
        .items
        .iter()
        .map(|item| item.word.as_str())
        .collect::<Vec<_>>();
    assert_eq!(pum_words, vec!["alpha", "alphabet", "alpine"]);
    assert_eq!(
        eval_string(&mut session, "complete_info(['items']).items[0].word"),
        "alpha"
    );
    assert_eq!(
        eval_string(&mut session, "complete_info(['items']).items[1].word"),
        "alphabet"
    );
    assert_eq!(
        eval_string(&mut session, "complete_info(['items']).items[2].word"),
        "alpine"
    );
}

#[test]
fn pum_contract_stays_separate_from_popupwin_presentation_ownership() {
    let public_api_reference = std::fs::read_to_string("docs/public-api-reference.md")
        .expect("public API reference should be readable");
    let api_index =
        std::fs::read_to_string("docs/api-index.md").expect("API index should be readable");

    assert!(
        public_api_reference.contains("popupwin is host-owned presentation")
            && public_api_reference.contains("does not expose a public popupwin extractor"),
        "public API reference should keep popupwin outside the public PUM extraction contract"
    );
    assert!(
        api_index.contains("does not expose a public popupwin extractor"),
        "API index should distinguish PUM extraction from popupwin ownership"
    );
}

#[test]
fn pum_contract_docs_keep_pum_separate_from_popupwin_exclusion() {
    let public_api_reference = fs::read_to_string("docs/public-api-reference.md")
        .expect("public API reference should be readable");
    let api_index = fs::read_to_string("docs/api-index.md").expect("API index should be readable");

    assert!(
        public_api_reference.contains("pum stays separate from popupwin exclusion"),
        "public API reference should keep PUM separate from popupwin exclusion"
    );
    assert!(
        api_index.contains("pum stays separate from popupwin exclusion"),
        "API index should keep PUM separate from popupwin exclusion"
    );
}
