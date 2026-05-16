# saya

`saya` は `vim-core-rs` を土台にした Markdown-first な CLI
テキストエディタです。アプリケーション層で編集挙動を近似するのではなく、
Vim 由来の編集セマンティクスを組み込み、Rust でオーケストレーションと
TUI を構築し、設定と拡張の公開面を Vim script ではなく TypeScript で
再設計しています。

この README は、プロジェクトの入口として、概要、ビルド方法、起動方法、
恒久ドキュメントへの導線だけをまとめています。長期的に参照する情報は
すべて `docs/` 以下に整理しています。

> **Note:** This is a preview feature currently under active development.

## このリポジトリの位置づけ

現在の `saya` には、動作する CLI エディタ MVP、startup 用の TypeScript
設定 runtime、そして別レイヤーとしての runtime callback 基盤があります。
一方で、TUI と常駐 runtime の完全統合はまだ途中なので、現状は
「動く CLI エディタ」と「育成中の TypeScript-first 拡張モデル」を
合わせ持つリポジトリです。

## ドキュメント

恒久的に残すドキュメントは `docs/` にあります。最初は次のページから読むと
全体像を掴みやすいです。

- [Project overview](docs/overview.md)
- [Requirements](docs/requirements.md)
- [Architecture](docs/architecture.md)
- [Boot flow design](docs/design/boot-flow.md)
- [Editing flow design](docs/design/editing-flow.md)
- [Floating windows design](docs/design/floating-windows.md)
- [TypeScript runtime design](docs/design/typescript-runtime.md)
- [Startup API](docs/api/startup-api.md)
- [Runtime API](docs/api/runtime-api.md)
- [LSP preview](docs/api/lsp-preview.md)
- [Testing](docs/testing.md)
- [Status](docs/status.md)

英語版の入口ページは
[README.md](README.md)
です。

## ビルド

このリポジトリは crates.io で公開された `vim-core-rs` crate を参照します。
そのため、Cargo がビルド時に自動で取得します。

ビルド前に、次の前提を満たしてください。

- Rust stable
- `cargo`
- `rusty_v8` をビルドできる C または C++ toolchain

リポジトリルートで次を実行します。

```bash
cargo build
```

バイナリ名は `sy` です。

## 起動

今ある CLI エディタは、既存ファイルでも新規バッファでも起動できます。

既存ファイルを開くには、次を実行します。

```bash
cargo run --bin sy -- path/to/file.txt
```

新規バッファで起動するには、次を実行します。

```bash
cargo run --bin sy --
```

`-u` を省略した場合、`sy` はデフォルト設定として
`$XDG_CONFIG_HOME/saya/init.ts` を探し、`XDG_CONFIG_HOME` が未設定なら
`$HOME/.config/saya/init.ts` にフォールバックします。

TypeScript 設定ファイルを渡すには、次を実行します。

```bash
cargo run --bin sy -- path/to/file.txt -u ./init.ts
```

標準入力から読み込むには、次を実行します。

```bash
printf 'hello\nworld\n' | cargo run --bin sy -- -
```

指定行から開くには、次を実行します。

```bash
cargo run --bin sy -- +42 path/to/file.txt
```

read-only で開くには、次を実行します。

```bash
cargo run --bin sy -- -R path/to/file.txt
```

ヘルプを表示するには、次を実行します。

```bash
cargo run --bin sy -- --help
```

## 最小の `init.ts`

現在の startup API では、起動前に option、command、event を宣言できます。

```ts
saya.options.tabstop = 4;
saya.options.number = true;

saya.keymap.set("normal", "<leader>w", saya.commands.execute("writeCurrent"));

saya.commands.register("writeCurrent", () => {
  return saya.commands.execute("write");
});

saya.events.on("bufferOpen", (payload) => {
  console.log(payload.buffer.id);
});
```

現時点で TUI 上で最も確認しやすい設定は `tabstop` と `number` です。
command と event の登録は実装済みで headless テストもありますが、常駐 runtime
との本番統合はまだ進行中です。

## 現在のスコープ

現在の実装では、次の機能を利用できます。

- 既存ファイル起動と新規バッファ起動
- Normal mode と Insert mode
- `hjkl` 移動、文字入力、削除
- host action ベースの保存と終了
- UI 上の dirty state 表示
- tab 展開と行番号表示
- `deno_core` を使った TypeScript startup 評価
- typed snapshot を使う runtime callback 基盤

## ライセンス

`saya` のソースコード自体は Apache License 2.0 です。詳細は
[LICENSE](LICENSE)
を参照してください。

一方で、このリポジトリは `vim-core-rs` に依存していて、その先で modified Vim
sources を Vim License のまま再配布します。`saya` のバイナリを再配布する
場合は、関連する第三者ライセンス通知も一緒に扱ってください。詳細は
[THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md)
を参照してください。

## 次に読むとよいもの

より詳しく理解したい場合は、次の順で読むのが近道です。

1. [Project overview](docs/overview.md)
   を読む
2. [Requirements](docs/requirements.md)
   を読む
3. [Architecture](docs/architecture.md)
   を読む
4. [docs/design](docs/design)
   以下の設計書を読む
5. [docs/api](docs/api)
   以下の API ドキュメントを読む
