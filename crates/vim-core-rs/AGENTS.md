# AGENTS.md

このファイルは、このリポジトリで作業するエージェント向けの共通ガイド
です。`vim-core-rs` の責務境界を崩さず、既存の契約とテストを基準に変更
を行うための前提と、日常開発で守る運用ルールをまとめています。

## Repository identity

このリポジトリは `vim-core-rs` 専用のチェックアウトです。canonical path は
`/Users/skudo/ghq/github.com/shun/saya_ws/vim-core-rs` です。

`saya` の bare repository 向け AGENTS にある worktree 運用ルールは、この
リポジトリには直接適用しません。issue 本文、対象ファイル、責務境界の
いずれかが `vim-core-rs` 側を指す場合は、このリポジトリへ移動してから
調査、spec 作成、実装、検証を進めてください。逆に、作業対象が `saya`
本体や別リポジトリに属すると判明した場合は、ここで実装を続けず、対象
リポジトリへ切り替えてその AGENTS に従ってください。

## Project overview

`vim-core-rs` は、1 つの埋め込み Vim ランタイムを Rust から扱うための
ホスト統合レイヤーです。これは CLI エディタ本体ではなく、別のホスト
アプリケーションが Vim のモーダル編集エンジンを利用するためのクレート
です。

このクレートの主要な責務は次のとおりです。

- 埋め込み Vim ランタイムを 1 プロセスにつき 1 つ管理する
- Normal / Ex コマンドを実行する
- バッファ、カーソル、ウィンドウ、検索、Undo、補完候補などの状態を
  スナップショットとして取り出す
- ファイルライクな Ex コマンドをホスト管理の VFS 要求へ変換する
- ジョブやチャネル相当の動作をホスト管理の VFD 経由で橋渡しする

詳細な前提を確認したい場合は、まず [README.md](README.md) を読み、
次に [docs/SCOPE.md](docs/SCOPE.md) と
[docs/api-contracts.md](docs/api-contracts.md) を参照してください。

## Design assumptions

このリポジトリでの実装や提案は、次の前提に沿って判断してください。

- コアの編集意味論は Vim 本体の C 実装に委譲する
- Rust 側は安全な公開 API、状態変換、ホスト境界の調停を担う
- `vim_bridge.h` を唯一の bindgen 入力として維持する
- Vim 内部ヘッダーや内部型を Rust 公開面へ漏らさない
- ホストが UI、永続化、実プロセス管理、非同期 orchestration を所有する
- 契約テストを振る舞いの最終的な正とみなす
- Neovim 互換や一般的なプラグインホスト化を目標にしない

## Out of scope

次の方向へ拡張しないでください。曖昧な変更案では
[docs/SCOPE.md](docs/SCOPE.md) の in scope / out of scope を優先します。

- エディタ全体の event loop や rendering pipeline の所有
- ファイル保存や OS プロセス起動の自己完結実装
- Vim script / Lua を中心にした汎用スクリプトプラットフォーム化
- Tree-sitter などを汎用の意味解析エンジンとして内包すること
- `:terminal` 相当のターミナルサブシステムの内包
- Neovim 互換レイヤーや msgpack RPC 前提の設計

Tree-sitter を扱う場合は、汎用 semantic platform ではなく、公開
syntax extraction contract の一部として扱ってください。具体的には
[docs/adr/0003-versioned-tree-sitter-extraction.md](docs/adr/0003-versioned-tree-sitter-extraction.md)
に従い、Vim-derived syntax extraction と別 surface にし、versioned
language package、source revision、parse status、normalized chunks、
request / response 型 preparation model を前提にしてください。
`saya` などのホストへ grammar、query、capture overlap 解決、cache
invalidation の所有責務を移してはいけません。

## Important invariants

README と契約テストから外してはいけない前提です。実装や設計変更では、
まずこの不変条件を壊していないかを確認してください。

- 同一プロセス内で生存できる `VimCoreSession` は 1 つだけ
- `VimCoreSession` は stateful であり、`Send` / `Sync` ではない
- `take_pending_host_action()` は通常制御フローの一部であり補助 API ではない
- VFS 要求はホスト所有であり、`submit_vfs_response()` で応答する
- ジョブ実行はホスト所有であり、`JobStart` に応答して実プロセスを起動する
- stdout / stderr は `inject_vfd_data()` で戻し、終了状態は
  `notify_job_status()` で通知する
- 文書と直感が衝突したら、まず `tests/` の契約テストを確認する

## Repository reading order

全体像を掴むときは、責務境界と契約を先に読み、実装詳細は後から追って
ください。順番を守ると、Rust 側で再実装してはいけない責務を見失いにくく
なります。

1. [README.md](README.md)
2. [docs/SCOPE.md](docs/SCOPE.md)
3. [docs/known-limitations.md](docs/known-limitations.md)
4. [docs/api-index.md](docs/api-index.md)
5. [docs/api-contracts.md](docs/api-contracts.md)
6. [src/lib.rs](src/lib.rs)
7. `tests/*.rs`

主要パスの役割は次のとおりです。

- `src/lib.rs`: 公開 API、コマンド実行、スナップショット変換、イベント /
  host action キュー
- `src/vfs.rs`: VFS 要求 ledger、buffer binding、deferred close
- `src/vfd.rs`: 仮想 file descriptor と job bridge
- `native/`: C bridge と埋め込み Vim ランタイムの薄い接着層
- `tests/`: 契約テスト群
- `build.rs` と `build_*.rs`: allowlist 検証、bindgen、native build、
  link audit、artifact 解決

## Ownership and investigation

変更前に、その責務がどの層に属するかを明確にしてください。特に
`vim-core-rs` とホストアプリケーションの境界、Rust 側と Vim 本体の境界を
曖昧にしたまま実装を始めてはいけません。

- いきなり実装に入らず、複数案の pros / cons と責務境界を確認してから
  方針を決める
- 不具合や挙動調査では、まずログを入れ、再現テストを実行して現象を再現
  する
- 実装時は、先にログを入れてテスト時に挙動を追える状態を作る
- 明示的な指示があるまで、デバッグログを勝手にクリーンアップしない
- サステナブルに保守できるように、場当たり対応より仕組み化を優先する
- headless で必ず動作確認できる実装を優先する
- オリジナル Vim との差分確認が必要なら `vendor/upstream/vim` を参照する

## Development workflow

このリポジトリの標準ワークフローは `cc-sdd` ベースの spec-driven
development です。Kiro 系の `.kiro/` 資産と整合する形で、discovery から
spec の必要性を判断して進めてください。

- 既定のワークフローとして `cc-sdd` を使う
- upstream の最新安定 `cc-sdd@latest` を前提にし、legacy installer flow は
  使わない
- Codex Skills を標準 install target とし、初期化や更新には
  `npx cc-sdd@latest --codex-skills` を使う
- `cc-sdd` の資産が欠けている、または stale の場合は、作業前に対象
  checkout で再実行する
- まず `kiro-discovery` から始め、既存 spec の拡張、新規 spec、複数 spec へ
  の分割、または直接実装のいずれに進むかを判断する
- spec が必要な場合は `kiro-spec-init`、`kiro-spec-requirements`、
  `kiro-spec-design`、`kiro-spec-tasks`、`kiro-impl` の順に進める
- active spec がある場合は `.kiro/specs/` と整合していることを確認し、phase
  を飛ばす場合は意図を明確にする
- Think in English, generate responses in Japanese
- Markdown で残す成果物は、対象 spec の `spec.json.language` に従う

## Implementation rules

実装は Kent Beck スタイルの TDD を前提に進めてください。変更の正しさは
契約テストと再現テストで担保し、ログで原因追跡できる状態を保って進めます。

- Kent Beck スタイルの TDD を採用し、RED → GREEN → REFACTOR の順で進める
- 編集ロジックを Rust で再実装せず、可能な限り Vim 本体へ委譲する
- Rust 側では unsafe 境界を局所化し、安全な型で再ラップする
- FFI 境界の変更時は `vim_bridge.h` / `native/` / bindgen 生成物の流れを
  一貫して保つ
- VFS / VFD / host action のようなホスト境界は、責務を混ぜず明示的な状態
  遷移として扱う
- 公開 API を増やすときは host-owned / crate-owned の境界を文書化する
- Tree-sitter syntax extraction を追加または変更するときは
  [ADR 0003](docs/adr/0003-versioned-tree-sitter-extraction.md) を優先し、
  `get_line_syntax()` / `CoreSyntaxChunk` の Vim-derived contract と混ぜない
- 振る舞い変更では、対応する契約テストかドキュメントを必ず更新する
- コードやドキュメントを修正したら、`skill-creator` を使って関連 skill の
  `SKILL.md` や `agents/openai.yaml` が最新か確認し、必要なら更新する

## Command and build rules

コマンド実行では、何を確認するのかを先に共有し、ハングしにくい形で
再現性のある検証を行ってください。通常開発のビルド基準は source build
です。

- コマンドを実行する前に、必ずそのコマンドで何を確認するのかを出力する
- ハングの可能性があるテストや長時間コマンドでは `gtimeout` を優先する
- リポジトリ開発時の基本は `VIM_CORE_FROM_SOURCE=1` を付けたビルドと
  テストにする
- デフォルトの `cargo test` は prebuilt artifact を探しに行くため、
  リリース前の開発基準として扱わない
- `vendor/vim_src/` の変更では allowlist / build manifest / audit への
  影響を必ず確認する

## Subagent policy

このリポジトリでは、調査、設計検討、spec 作成、実装、テスト実行、
原因分析、検証など、作業の本文は原則すべて subagent に委譲します。
メインエージェントは、ユーザーとの通信、委譲計画、結果回収、統合、
整合性確認、最終判断、最終報告を担当します。メインエージェントが
直接進めるのは、subagent 化が不自然な最小限のオーケストレーション作業と、
subagent の成果を本体ワークスペースへ統合するための必要最小限の反映に
限ります。

- 使用するモデルは `gpt-5.4-mini` を subagent 委譲時の第一選択とする
- 調査、設計検討、spec 作成、実装、テスト実行、原因分析、検証などの
  本文作業は、原則としてまず subagent に委譲する
- subagent への依頼は 1 目的、または 1 不具合仮説ごとに分割する
- 同時に稼働させる subagent は必要最小限とし、原則 2 体までとする
- 同一スコープの重複実行は禁止する
- メインエージェントは、subagent に委譲した同一スコープの調査や実装を
  並行して実行してはならない
- discovery、spec 作成、実装、検証の本文作業は、`kiro-discovery` から
  方針を固めた上で subagent に分担させる
- subagent 利用がホストポリシー、利用可能ツール、権限、または実行環境の
  制約で禁止または失敗する場合は、その競合を即時にユーザーへ報告して
  停止する
- subagent が失敗、タイムアウト、キャンセルした場合は、失敗理由を共有し、
  再委譲または代替方針を明示して進める
- 「修正完了」や「原因特定完了」などの完了表現は、結果回収と必要な検証が
  完了した後のみ使う
- 最終報告には、委譲したタスク一覧、実行したテストコマンド、
  アーキテクチャ上の判断を含める

## Subagent management rules

subagent のライフサイクル管理は、委譲そのものと同じくらい重要です。不要な
agent を残さず、状態を追跡できるようにしてください。

- subagent を起動したら、agent id、目的、担当ファイル、起動時刻を記録し、
  管理対象一覧を維持する
- subagent が完了、失敗、不要になった時点で、必ず `close_agent` を呼んで
  クローズ結果を確認する
- `close_agent` の成功確認前に、停止済みと断言してはならない
- ユーザーが停止を求めた場合は、把握しているすべての agent id に対して
  `close_agent` を実行し、`not found` を含む結果を共有する
- UI 上に subagent が残って見える場合は、内部状態だけを根拠に停止済みと
  判断してはならない
- 新しい subagent を起動する前に、既存の稼働中 subagent が本当に必要かを
  点検し、不要なら先に閉じる
- rate limit やコストへの影響を避けるため、軽微な調査や短いコード読解では
  subagent を濫用しない
- 完了通知だけでは終了扱いにせず、`close_agent` 後、または `not found`
  確認後にのみ完全終了として扱う
- 最終報告には、起動した subagent 一覧と、全員を停止済みであることを
  含める

## Documentation and review

設計意図が伝わりにくい変更では、コードだけで完結させず、README や
`docs/` の更新も確認してください。レビューでは責務境界と契約維持を最優先
で見ます。

- Vim 本体で持つべき責務を Rust / C bridge 側で再実装していないか
- FFI 境界が広がりすぎていないか
- ホスト所有の責務を crate 側へ取り込んでいないか
- 単一セッション制約や VFS / VFD の状態遷移を壊していないか
- 公開 API が契約テストと文書の境界に沿っているか
- 不要な複雑性や互換レイヤーを持ち込んでいないか

## Handling unknowns

リポジトリ内の事実だけで判断できない仕様は、推測で固定しないでください。
特に責務境界や期待シーケンスに関わる不明点は、作業前に確認を取ります。

- `vim-core-rs` とホストアプリケーションの責務境界
- 公開 API と internal API のどちらで扱うべきか
- VFS / VFD / host action の期待シーケンス
- upstream Vim 由来の挙動をそのまま保持すべきか、Rust 側で吸収すべきか
