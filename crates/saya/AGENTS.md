# AGENTS.md

このファイルは、このリポジトリで作業するエージェント向けの共通ガイド
です。最初に全体像を掴み、設計意図を崩さない変更を行うための基準を
まとめています。

## プロジェクト概要

このリポジトリは、`vim-core-rs` を利用した CLI テキストエディタです。
Vim 由来の編集モデルを活かしつつ、Neovim の互換性は目標に含めません。
Neovim に由来する歴史的負債や互換レイヤーを持ち込まないことを前提に
設計します。

設定系の拡張は Vim script を前提にせず、`deno_core` を使って
TypeScript で記述できることを重要な方向性とします。高負荷処理は必要に
応じて Wasm へオフロードする、`docs/architecture.md` の
ハイブリッド構成を前提にしてください。

関連リポジトリ:

- `vim-core-rs`: https://github.com/shun/vim-core-rs

## 設計上の前提

このリポジトリでの実装や提案は、次の前提に沿って判断してください。

- コアの編集体験は `vim-core-rs` の能力を土台に構成する
- 設定や拡張の公開面は Vim script ではなく TypeScript を優先する
- `deno_core` は TypeScript 実行基盤として扱う
- 高負荷処理は TypeScript に閉じ込めず Wasm オフロードを検討する
- Neovim 互換性は不要であり、そのための複雑性を持ち込まない
- CLI エディタとしての起動速度、単純さ、可搬性を重視する
- レイヤー分離は `docs/architecture.md` を基準に守る

## エージェントへの期待

変更を行うときは、単に動くコードを足すのではなく、このエディタが
目指す責務の境界を保ってください。

- Neovim 由来の概念、互換レイヤー、移植前提 API を追加しない
- Vim script 前提の設定導線を増やさない
- TypeScript 設定体験を悪化させる API 追加を避ける
- コア編集機能と設定実行基盤の責務を分離する
- Main Thread をブロックする設計を避ける
- 設計判断では短期互換性より長期的な単純さを優先する

## 実装時の指針

作業前に、変更がどの層に属するかを明確にしてください。特に
`vim-core-rs` に委譲すべき責務と、このリポジトリで持つべき責務を
混同しないことが重要です。

- 編集動作そのものは、可能な限り `vim-core-rs` の責務として扱う
- TypeScript 設定に関する処理は、`deno_core` を通した実行モデルとして
  一貫性を保つ
- UI スレッドとスクリプト実行系は非同期メッセージで疎結合に保つ
- ユーザー向けの設定 API は、明示的で最小限に保つ
- 新しい依存追加は、CLI エディタとして妥当かを確認してから行う
- 振る舞いを変える変更では、設計意図が伝わるテストかドキュメントを
  併せて更新する
- 実装後の検証は、ログや UI イベントが出たことだけで完了にしない。
  変更がバッファ編集、ファイル保存、runtime state、UI の開閉や選択状態、
  設定反映などの最終状態を変えるなら、テストまたは smoke でその状態を
  読み戻して期待値と一致することを assert する。ログは「どの経路を通った
  か」を説明する補助であり、状態変化の assert の代替にしない。
- TypeScript startup で `saya.commands.register` または `saya.events.on`
  を使う変更では、startup runtime での登録成功だけを検証完了にしない。
  callback は `Function.prototype.toString()` で live runtime に渡される
  ため、closure 変数や startup-only API に依存していると、起動後に
  ユーザーが実行した瞬間に壊れる。登録された callback source を
  closure なし環境で実行するテスト、または
  `CallbackRegistrySeed` から live runtime を起動して実際に command/event
  を実行する Layer 2 テストを必ず追加する。
- 補完 UX (`saya.completion.show`、bundled completion source、PUM、
  input routing、floating window key handling) を変更した場合は、
  「候補が出た」だけで検証完了にしない。少なくとも複数候補が表示される
  こと、選択移動で selected row が変わること、Enter で選択候補が確定
  されること、確定後のメモリ上バッファまたは保存後ファイル内容が期待値
  と一致することをテストで確認する。ログは診断補助であり、buffer/file
  contents assertion の代替にしない。可能なら
  `tests/integration_binary_smoke.rs` の binary smoke で実行ファイル境界も
  通す。
- 補完 source や query helper が filesystem、workspace、editor state を
  読むだけなら、必ず readonly な host API を使う。dired/filer のように
  active buffer を投影・更新する API を補完 source から呼ばない。fake host
  では副作用 API を無害な mock にせず、呼ばれたら失敗するようにして責務境界
  をテストする。path completion では `saya.fs.readDir()` を使い、
  `saya.filer.list()` を使わない。

## 不具合修正の設計方針

不具合修正では、最小パッチを第一候補にしないでください。まず原因を
観測し、責務境界、データフロー、テスト容易性を踏まえて、あるべき設計で
直す方針を検討してください。

修正案は、少なくとも次の観点で比較してください。

- 局所パッチ: 変更量は少ないが、責務の歪み、重複、再発リスクを残さないか
- 設計修正: 変更量は増えても、同種の不具合を構造的に防げるか
- 境界修正: 本来の責務を正しい層へ戻せるか

原則として、同種の不具合が再発し得る場合、局所パッチではなく設計修正を
選んでください。ただし、修正範囲を無制限に広げず、現在の不具合を
構造的に解決する範囲に閉じてください。

実装前に、採用案と不採用案の pros/cons を短く示し、Kent Beck Style の
TDD に従って RED を確認してから実装に入ってください。

## ドキュメントとレビュー

設計意図が伝わりにくい変更では、コードだけで完結させず、
`README.md` や関連ドキュメントも更新候補として確認してください。

レビュー時は、次の観点を優先します。

- その変更は Neovim 依存の再導入になっていないか
- その変更は Neovim 互換性を暗黙に期待する設計になっていないか
- TypeScript 設定の一貫性を崩していないか
- `vim-core-rs` とアプリケーション層の責務分離が守られているか
- CLI ツールとして不要な複雑性を持ち込んでいないか
- スレッド分離と非同期メッセージングの原則を壊していないか

## 不明点の扱い

リポジトリ内の事実だけで判断できない仕様は、推測で固定しないで
ください。特に次の内容が必要になった場合は、作業前に確認を取って
ください。

- TypeScript 設定 API の公開方針
- `vim-core-rs` 側へ寄せる責務の境界
- Neovim 非互換を前提として許容しない機能や API
- CLI UX に関する優先順位

# AI-DLC and Spec-Driven Development

Kiro-style Spec Driven Development implementation on AI-DLC (AI Development Life Cycle)

## Project Memory

Project memory keeps persistent guidance (steering, specs notes, component docs) so Codex honors your standards each run. Treat it as the long-lived source of truth for patterns, conventions, and decisions.

- Use `.kiro/steering/` for project-wide policies: architecture principles, naming schemes, security constraints, tech stack decisions, api standards, etc.
- Use local `AGENTS.md` files for feature or library context (e.g. `src/lib/payments/AGENTS.md`): describe domain assumptions, API contracts, or testing conventions specific to that folder. Codex auto-loads these when working in the matching path.
- Specs notes stay with each spec (under `.kiro/specs/`) to guide specification-level workflows.

## Project Context

### Paths

- Steering: `.kiro/steering/`
- Specs: `.kiro/specs/`

### Steering vs Specification

**Steering** (`.kiro/steering/`) - Guide AI with project-wide rules and context
**Specs** (`.kiro/specs/`) - Formalize development process for individual features

### Active Specifications

- Check `.kiro/specs/` for active specifications
- Use `/prompts:kiro-spec-status [feature-name]` to check progress

## Development Guidelines

- Think in English, generate responses in Japanese. All Markdown content written to project files (e.g., requirements.md, design.md, tasks.md, research.md, validation reports) MUST be written in the target language configured for this specification (see spec.json.language).

## Minimal Workflow

- Phase 0 (optional): `/prompts:kiro-steering`, `/prompts:kiro-steering-custom`
- Phase 1 (Specification):
  - `/prompts:kiro-spec-init "description"`
  - `/prompts:kiro-spec-requirements {feature}`
  - `/prompts:kiro-validate-gap {feature}` (optional: for existing codebase)
  - `/prompts:kiro-spec-design {feature} [-y]`
  - `/prompts:kiro-validate-design {feature}` (optional: design review)
  - `/prompts:kiro-spec-tasks {feature} [-y]`
- Phase 2 (Implementation): `/prompts:kiro-spec-impl {feature} [tasks]`
  - `/prompts:kiro-validate-impl {feature}` (optional: after implementation)
- Progress check: `/prompts:kiro-spec-status {feature}` (use anytime)

## Development Rules

- 3-phase approval workflow: Requirements → Design → Tasks → Implementation
- Human review required each phase; use `-y` only for intentional fast-track
- Keep steering current and verify alignment with `/prompts:kiro-spec-status`
- Follow the user's instructions precisely, and within that scope act autonomously: gather the necessary context and complete the requested work end-to-end in this run, asking questions only when essential information is missing or the instructions are critically ambiguous.
- 不具合や挙動がおかしい調べごとは、まず該当する箇所にログを入れテストを実行して確認するようにしてください。
- コマンドを実行する前に、必ずコマンドで何を実行しようとしているか出力してからコマンドは実行すること
- いきなり実装に入らず、複数の案からpros/cons を検討し、あるべき設計を考慮して方針を立ててからテストコードを作成し、実装にはいること。
- Kent Beck Style のTDDで、確実にREDにしてから実装を行い、GREEN、REFACTORで実装すること。
- 実装時は必ず詳細なログを含め、常にログから原因を追えるようにすること。デバッグログは明示的な指示があるまでクリーンアップしない。
- サステナブルにメンテできるように仕組み化すること
- 実装する際には、必ず先にログを実装してテスト時にうまく実装されているのか、されていないかをログから判断できるように実装してください。
- テストなどがハングしないように `gtimeout` コマンドを使用してコマンドを実行してください。
- オリジナルのvimのコードを編集しているかもしれないので、比較したい場合は vendor/upstream/vim（submodule）を参照してください。
- headless で必ず動作確認できるような実装にすること

## Steering Configuration

- Load entire `.kiro/steering/` as project memory
- Default files: `product.md`, `tech.md`, `structure.md`
- Custom files are supported (managed via `/prompts:kiro-steering-custom`)
