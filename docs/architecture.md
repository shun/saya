# System Architecture

## 1. エグゼクティブ・サマリ：最強のハイブリッド・アーキテクチャ
本プロジェクトは、Vimの歴史的かつ完璧な操作パラダイムを維持しつつ、拡張性および設定環境を現代の最高峰技術で再構築する。アーキテクチャの最適解として、**「TypeScript (V8) と WebAssembly のハイブリッド構成」**を絶対原則とする。

1. **コントロールプレーン：`deno_core` (V8) による基本操作**
ユーザーが記述する `init.ts` やキーバインド設定、npmライブラリの読み込み、エディタの各種API操作は、すべてエディタ本体のRustバイナリに組み込まれた `deno_core` で処理する。通信遅延をナノ秒単位に抑え、TypeScriptの強力なエコシステムをそのまま提供する。
2. **データプレーン：Wasmへの高負荷処理オフロード**
ファジー検索アルゴリズム、巨大なファイルの構文解析など、純粋な計算速度が求められる重負荷処理は、TypeScript単体（JSエンジン）で実行させない。Deno（V8）標準のWasm爆速実行能力を活用し、該当処理のみをRust/Zig等でWasm化し、TypeScript内から `WebAssembly.instantiate` で動的ロードして呼び出す。

## 2. システム・レイヤー構造 (System Architecture Layers)
システムは厳格に分離された4つのレイヤーで構成される。上位レイヤーは下位レイヤーにのみ依存でき、逆は許されない。

```text
[Layer 4] User / Plugin Ecosystem (TypeScript & Wasm Hybrid)
    ├── init.ts / User Configurations
    ├── npm packages (e.g., Prettier, ESLint)
    └── High-Perf Wasm Modules (loaded via WebAssembly API)
         │
         ▼ (WebAssembly API / V8 Fast API)
[Layer 3] Scripting Engine (Rust + deno_core)
    ├── V8 Isolate (Worker Thread)
    └── Zero-Copy API Bridge (deno_core::op2)
         │
         ▼ (tokio::sync::mpsc / Async Event Bus)
[Layer 2] Orchestration & Presentation (Rust Main Thread)
    ├── Main Event Loop & Thread Manager
    ├── UI Renderer (TUI: Ratatui / GUI: WGPU)
    └── Buffer State Sync (Rope)
         │
         ▼ (C-FFI / bindgen)
[Layer 1] Original Vim Core (C Language: libvim)
    ├── Vim State Machine (Normal/Insert/Visual etc.)
    └── Text Objects & Ex Command Parser
```

## 3. スレッドモデルとレイテンシ保護 (Thread Isolation)
エディタのフリーズ（マイクロスタッター）を完全に防ぐため、以下のスレッド分離を絶対ルールとする。

1. **Main Thread (UI & Rust Core & C-Core)**:
ユーザー入力の受付、画面描画（Ratatui）、および C言語Vimコア（スレッドセーフではない）の操作を専任する。このスレッドはいかなる理由があってもブロックしてはならない。
2. **V8 Worker Thread (TypeScript Environment)**:
`deno_core` のイベントループとV8のガベージコレクション（GC）は、完全に独立した非同期タスク上で稼働させる。さらに、コア機能とプラグイン機能でWorkerを分離する（PluginRuntimeSupervisorによる隔離）。
3. **同期メカニズム**:
TS側からエディタを操作する場合、Rustの `tokio::sync::mpsc` チャネルを用いた**非同期メッセージパッシング**によりMain Threadへ命令を送信する。これにより、TS内で無限ループ等の暴走が起きても、Vimのカーソル移動やテキスト入力は絶対にフリーズしない。

## 4. プロジェクト・ディレクトリ構成 (Cargo Workspace)

```text
ts-vim-hybrid/
├── Cargo.toml                  # ワークスペース定義
├── vendor/
│   └── vim_src/                # オリジナルVim C言語ソースツリー (Git Submodule)
├── crates/
│   ├── vim_ffi/                # [Layer 1] Cコードのコンパイル(build.rs)とbindgenによるFFI生成
│   ├── editor_core/            # [Layer 2] Rust製テキスト管理(Rope)とメインイベントループ
│   ├── scripting_deno/         # [Layer 3] deno_coreの組み込み, op2定義
│   └── ui_renderer/            # [Layer 2] Ratatui描画フロントエンド
└── runtime_ts/                 # エディタバイナリに内蔵されるTypeScriptコアAPI群
    ├── std/
    └── vim_api.d.ts            # ユーザー(プラグイン開発者)向けの型定義ファイル
```

## 5. Technology Stack

| Layer | Choice / Version | Role in Feature |
|-------|------------------|-----------------|
| Frontend / CLI | `ratatui` + `crossterm` | terminal 描画、入力イベント、画面再描画 |
| Backend / Services | `Rust stable` + `tokio` | Main Thread orchestration、worker 管理、channel 通信 |
| Core Engine | Vim C source + `cc` + `bindgen` | Vim互換編集意味の実行、FFI境界生成 |
| Scripting Runtime | `deno_core` + `rusty_v8` | TypeScript実行と `op2` 契約提供 |
| Compute Offload | WebAssembly API | 高負荷処理の実行領域 (TypeScript空間内) |
| Observability | `tracing` + `opentelemetry` | 診断ログ、SLO計測、監査 |

## 6. UI Rendering & Virtual Projection (CUI WYSIWYG)
本エディタは、CUIでありながらMarkdownのダイアグラム（Mermaid等）をインラインで描画するWYSIWYG体験をサポートする。これを実現するため、Vimコアのバッファ管理とは独立した**「プロジェクション（投影）レイヤー」**をRust側に設ける。

1. **Virtual Lines (仮想行) と装飾**
バッファの生データ（Rope）を汚染することなく、TypeScriptプラグインから「指定行の直後にN行の仮想的な空行（Padding）を挿入する」「特定の文字列を別の文字や画像に置き換える（Virtual Text）」といった装飾情報をRust側に送信できる。
2. **Vim Foldとの連携**
Vimコアの標準機能である「Fold（折りたたみ）」を利用して元のソースコードブロック（例: 20行のMermaidコード）を1行に圧縮し、その直後にRust側のプロジェクション層が「画像表示用の仮想行（例: 15行）」を動的に挿入する。これにより、カーソル移動の整合性を保ちつつ、画像表示用のスペースをシームレスに確保する。
3. **ターミナル画像プロトコルのネイティブサポート**
RustのUIレンダラー（Ratatui）は、SixelやKitty Graphics Protocolなどのモダンな端末画像描画プロトコルをネイティブにサポートし、Wasmで生成されたダイアグラム画像（PNG/SVG等）を確保した仮想行スペースへ正確にレンダリングする。
