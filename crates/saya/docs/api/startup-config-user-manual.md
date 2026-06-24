# init.ts User Manual

この文書は、Saya の TypeScript startup config である `init.ts` を、
目的別に設定するための単一リファレンスです。Vim user manual のように、
順に読める説明と、必要な項目だけを引ける参照性を両立します。

`init.ts` は Vim script ではありません。`set tabstop=4`、
`nnoremap`、`autocmd` をそのまま書く形式ではなく、TypeScript の
`saya.*` API を使って起動時の設定と runtime callback を宣言します。

## この文書の読み方

最初に最小設定と Startup API / Runtime API の境界を読んでください。
その後は、やりたいことに近い「よく使う設定レシピ」から設定例を写し、
詳細が必要な場合に各 Reference 節を参照します。

この文書の根拠は、主に次の一次情報です。

- `crates/saya/docs/api/startup-api.md`
- `crates/saya/docs/api/runtime-api.md`
- `crates/saya/README_ja.md`
- `plugins/ts/saya-startup.d.ts`
- `plugins/ts/types/startup.d.ts`
- `crates/saya/src/runtime/options.rs`
- `crates/saya/src/runtime/startup/saya_payload.rs`
- `crates/saya/src/runtime/startup/ops.rs`
- `crates/saya/src/runtime/config/mod.rs`
- `plugins/ts/bundled/*/index.ts`
- `plugins/ts/bundled/*/manifest.json`

`.d.ts` と Rust 実装に差分がある場合、この文書は実行時挙動に近い
Rust registry と startup payload を主根拠にし、差分を該当箇所で明記
します。

## init.ts とは

`init.ts` は、Saya が editor session を開始する前に評価する
TypeScript 設定ファイルです。起動時に option、keymap、command、
event、filetype 設定、status line、theme、log、plugin 宣言を収集し、
その結果を session 初期化へ渡します。

`init.ts` では、静的な local TypeScript import を使えます。local import
は startup 評価前に解決されます。bare package import と network import
は startup surface の対象外です。

```ts
/// <reference path="./plugins/saya-startup.d.ts" />
import { setupSayaDired } from "./plugins/bundled/dired/index.ts";

saya.options.number = true;
setupSayaDired({ keymap: { enter: "<Enter>" } });
```

`/// <reference ... />` は外部 TypeScript language server 向けの型補助で、
runtime の挙動は変えません。

## 最小設定

最小構成では、global option、status line、filetype override、keymap、
command、event をまとめて宣言します。

```ts
saya.options.tabstop = 4;
saya.options.shiftwidth = 4;
saya.options.expandtab = true;
saya.options.smartindent = true;
saya.options.number = true;

saya.statusline.set({
  left: ["fileName", "mode"],
  right: ["filetype", "modified"],
});

saya.ftplugin.set("go", {
  extensions: ["go"],
  options: {
    expandtab: false,
    softtabstop: 0,
    shiftwidth: 0,
  },
});

saya.commands.register("writeCurrent", async () => {
  await saya.commands.execute("write");
});

saya.keymap.set("normal", "<leader>w", saya.commands.execute("writeCurrent"));

saya.events.on("bufferOpen", async (payload) => {
  console.log(payload.buffer.id);
});
```

この例では、`init.ts` 評価時に登録だけを行います。`writeCurrent` と
`bufferOpen` の callback が実行される時点では Runtime API が有効です。

## Startup API と Runtime API の違い

Startup API は、起動前に「何を登録するか」を宣言する API です。
Runtime API は、登録済み command や event callback の中で「現在の
buffer や editor 状態を読む」「host command を実行する」ための API です。

| 区分 | 使える場所 | 主な namespace | 役割 |
| --- | --- | --- | --- |
| Startup API | `init.ts` 評価中 | `saya.options`, `saya.keymap`, `saya.commands.register`, `saya.events.on`, `saya.ftplugin`, `saya.statusline`, `saya.theme`, `saya.log`, `saya.plugins` | 起動時設定を収集する |
| Runtime API | `saya.commands.register()` と `saya.events.on()` の callback 実行中 | `saya.commands.execute`, `saya.buffer`, `saya.window`, `saya.editor`, `saya.workspace`, `saya.fs`, `saya.filer`, `saya.lsp`, `saya.lsif`, `saya.input`, `saya.selector`, `saya.completion`, `saya.process`, `saya.panel`, `saya.plugins.loadLazy` | 現在状態を読み、host capability を呼ぶ |

重要な境界は、`saya.commands.register()` や `saya.events.on()` 自体は
Startup API であり、その callback の中では Runtime API を使うことです。
callback 内で新しい command や event を登録する用途は公開されていません。

```ts
saya.commands.register("showPath", async () => {
  const path = await saya.buffer.currentPath();
  await saya.commands.execute(`echo ${path ?? "[No Name]"}`);
});
```

`saya.commands.execute()` は、startup 直下で実行するものではありません。
keymap action を作るために `saya.commands.execute("name")` を渡すか、
runtime callback 内で `await saya.commands.execute("name")` として使います。

## よく使う設定レシピ

この節は、LLM や人間が `init.ts` を生成するときの入口です。各例は
TypeScript startup config として書きます。

### 行番号を表示する

行番号だけなら `number`、相対行番号も使うなら `relativenumber` を設定します。

```ts
saya.options.number = true;
saya.options.relativenumber = true;
saya.options.numberwidth = 5;
```

効果: 起動時の projected TUI で行番号欄を表示します。

関連項目: `Options Reference`, `Status Line`

### タブ幅とインデントを設定する

space indent を基本にする場合は、`tabstop`、`shiftwidth`、`softtabstop`、
`expandtab` をまとめて設定します。

```ts
saya.options.tabstop = 2;
saya.options.shiftwidth = 2;
saya.options.softtabstop = 2;
saya.options.expandtab = true;
saya.options.smartindent = true;
```

効果: 起動時の core option と editor state に indentation policy を渡します。

関連項目: `Filetype Settings`

### Go ファイルだけタブインデントにする

global は space indent にし、Go だけ Vim の Go 既定値に近づけます。

```ts
saya.options.expandtab = true;
saya.options.shiftwidth = 2;
saya.options.softtabstop = 2;

saya.ftplugin.set("go", {
  extensions: ["go"],
  options: {
    expandtab: false,
    shiftwidth: 0,
    softtabstop: 0,
  },
});
```

効果: `.go` buffer では filetype layer が global option を上書きします。

注意: `ftplugin` の option 型は `.d.ts` 上 `Partial<SayaStartupOptionsSurface>`
ですが、現在の適用経路は core option 寄りです。表示専用 option を
filetype ごとに確実に変えられるかは実装上確認できないため、indent 系を
中心に使ってください。

### statusline を調整する

status line は `left` と `right` に segment 名を並べます。

```ts
saya.statusline.set({
  left: ["mode", "fileName"],
  right: ["filetype", "modified"],
});
```

効果: status line の表示項目を置き換えます。複数回呼んだ場合は、最後に
収集された設定が使われます。

関連項目: `Status Line`

### Markdown と theme 表示を調整する

palette を定義し、Markdown semantic style から参照します。

```ts
saya.theme.palette = {
  accent: "#7aa2f7",
  muted: "#6b7280",
};

saya.theme.markdown = {
  heading1: { fg: "accent", bold: true },
  inlineCode: { fg: "#f7768e" },
  link: { fg: "accent", underline: true },
};

saya.theme.filer = {
  directory: { fg: "accent", bold: true },
  marked: { bg: "#334155" },
};
```

効果: Markdown、filer、UI、syntax の semantic style を startup で収集します。

注意: style の色は直接の `#rrggbb` か、`#rrggbb` を指す palette token を
使うのが安全です。ops は任意 string を受け取りますが、解決できない色は
theme resolution で落ちる可能性があります。

### bufferOpen で処理する

event callback では Runtime API を使えます。

```ts
saya.events.on("bufferOpen", async (payload) => {
  const path = payload.buffer.path;
  const root = path
    ? await saya.workspace.findRoot(path, ["Cargo.toml", "go.mod", ".git"])
    : null;

  console.log({ bufferId: payload.buffer.id, root });
});
```

効果: buffer が開かれたときに read-only snapshot を受け取り、workspace root
など runtime state に基づく処理を行います。

### keymap でコマンドを呼ぶ

registered command を keymap から呼ぶ場合は、`saya.commands.execute()` が
返す startup command reference を action に渡します。

```ts
saya.commands.register("openParent", async () => {
  const path = await saya.buffer.currentPath();
  if (!path) return;
  await saya.commands.execute("edit ..");
});

saya.keymap.set("normal", "-", saya.commands.execute("openParent"));
```

効果: normal mode の `-` が `openParent` command を起動します。

### LSP を有効化する

LSP は bundled `lsp-client` plugin を import して設定します。

```ts
import { setupSayaLspClient } from "./plugins/bundled/lsp-client/index.ts";

setupSayaLspClient({
  enableBufferEvents: true,
  keymap: {
    hover: "K",
    definition: "gd",
    references: "gR",
    documentSymbol: "gO",
  },
  languageIdByExtension: {
    go: "go",
    rs: "rust",
  },
  servers: [
    {
      name: "gopls",
      command: "gopls",
      args: ["serve"],
      languages: ["go"],
      filePatterns: ["**/*.go"],
      rootMarkers: ["go.mod", ".git"],
    },
  ],
});
```

効果: LSP command、必要に応じた keymap、buffer lifecycle event handler を
登録します。

注意: LSP は preview 機能です。未対応機能は `Bundled Plugin Recipes` を
参照してください。

### dired を有効化する

dired は keymap を明示したときだけ mapping を登録します。

```ts
import { setupSayaDired } from "./plugins/bundled/dired/index.ts";

setupSayaDired({
  root: ".",
  hiddenFilePolicy: "hide",
  sortPolicy: "kind",
  keymap: {
    up: "-",
    enter: "<Enter>",
    refresh: "gr",
  },
});
```

効果: dired command を登録し、指定した normal mode keymap から呼べます。

### diagnostic log を出す

startup log は `saya.log.file` と `saya.log.level` で設定します。

```ts
saya.log.file = "/tmp/saya.log";
saya.log.level = "debug";
```

効果: startup config 由来の log 設定を収集します。

注意: 環境変数 `SAYA_LOG_FILE` は startup の `saya.log.file` より優先されます。

## Options Reference

`saya.options.*` は Vim / Neovim 風の lowercase option 名を使います。
JavaScript 風の camelCase 名は startup public API ではありません。

この表は Rust の `SayaOptionRegistry`、startup payload の getter default、
および config apply の範囲検証を根拠にしています。`.d.ts` は canonical 名
のみを公開しており、alias property は型定義にありません。ただし startup
payload と Rust registry は alias を受け付けます。LLM が設定を生成する
場合は、互換性のため canonical 名を優先してください。

| 名前 | 型 | デフォルト値 | alias | 設定例 | 効果 | startup public | 備考または制限 |
| --- | --- | --- | --- | --- | --- | --- | --- |
| `autoindent` | `boolean` | `false` | `ai` | `saya.options.autoindent = true;` | core の自動インデント初期値を設定する | true | boolean のみ |
| `cursorline` | `boolean` | `false` | `cul` | `saya.options.cursorline = true;` | cursor line 表示を有効化する | true | presentation-owned |
| `expandtab` | `boolean` | `false` | `et` | `saya.options.expandtab = true;` | tab 入力を space 展開する方針にする | true | core-owned |
| `foldlevel` | `number` | `0` | `fdl` | `saya.options.foldlevel = 1;` | fold 表示 level を設定する | true | 適用時の範囲は `0..99` |
| `foldmethod` | `string` | `"manual"` | `fdm` | `saya.options.foldmethod = "manual";` | fold method 名を保持する | true | 現在の apply 層では値の列挙検証なし |
| `hlsearch` | `boolean` | `false` | `hls` | `saya.options.hlsearch = true;` | 検索 highlight の初期 flag を設定する | true | search pattern がある時に効く |
| `ignorecase` | `boolean` | `false` | `ic` | `saya.options.ignorecase = true;` | 検索などの ignorecase 方針を設定する | true | core-owned |
| `laststatus` | `number` | `2` | `ls` | `saya.options.laststatus = 2;` | status line 表示方針を設定する | true | 適用時の範囲は `0..3` |
| `number` | `boolean` | `false` | `nu` | `saya.options.number = true;` | 行番号を表示する | true | presentation-owned |
| `list` | `boolean` | `false` | なし | `saya.options.list = true;` | 不可視文字表示を有効化する | true | `listchars` と併用 |
| `listchars` | `string` | `"tab:>-,trail:-"` | `lcs` | `saya.options.listchars = "tab:>-,trail:-";` | list 表示の文字を設定する | true | 現在の apply 層では詳細構文検証なし |
| `mermaidpreview` | `boolean` | `true` | `mmdpreview` | `saya.options.mermaidpreview = false;` | Mermaid preview の自動表示方針を設定する | true | presentation-owned |
| `mermaidpreviewbackground` | `string` | `"transparent"` | `mmdpreviewbackground` | `saya.options.mermaidpreviewbackground = "#ffffff";` | Mermaid preview 背景を設定する | true | 空文字は `transparent` に正規化 |
| `mermaidpreviewwidth` | `number` | `55` | `mmdpreviewwidth` | `saya.options.mermaidpreviewwidth = 70;` | Mermaid preview 幅の割合を設定する | true | 適用時の範囲は `1..100` |
| `mermaidpreviewheight` | `number` | `55` | `mmdpreviewheight` | `saya.options.mermaidpreviewheight = 60;` | Mermaid preview 高さの割合を設定する | true | 適用時の範囲は `1..100` |
| `cmdheight` | `number` | `5` | `ch` | `saya.options.cmdheight = 3;` | message area の高さを設定する | true | 適用時の範囲は `1..999` |
| `numberwidth` | `number` | `4` | `nuw` | `saya.options.numberwidth = 5;` | 行番号欄の幅を設定する | true | 適用時の範囲は `1..32` |
| `relativenumber` | `boolean` | `false` | `rnu` | `saya.options.relativenumber = true;` | 相対行番号を表示する | true | registry には canonical と同名 alias もある |
| `scrolloff` | `number` | `0` | `so` | `saya.options.scrolloff = 3;` | cursor 周辺に保つ縦余白を設定する | true | 適用時の範囲は `0..999` |
| `shiftwidth` | `number` | `8` | `sw` | `saya.options.shiftwidth = 4;` | indent shift 幅を設定する | true | 適用時の範囲は `0..32` |
| `sidescrolloff` | `number` | `0` | `siso` | `saya.options.sidescrolloff = 2;` | 横 scroll 周辺の余白を設定する | true | 適用時の範囲は `0..999` |
| `smartcase` | `boolean` | `false` | `scs` | `saya.options.smartcase = true;` | ignorecase と組み合わせる smartcase 方針を設定する | true | core-owned |
| `smartindent` | `boolean` | `false` | `si` | `saya.options.smartindent = true;` | smart indent 方針を設定する | true | core-owned |
| `softtabstop` | `number` | `0` | `sts` | `saya.options.softtabstop = 4;` | editing 上の soft tab stop を設定する | true | 適用時の範囲は `-1..32` |
| `syntax` | `boolean` | `false` | なし | `saya.options.syntax = true;` | syntax 表示の初期 flag を設定する | true | core command flag |
| `tabstop` | `number` | `8` | `ts` | `saya.options.tabstop = 4;` | tab 幅を設定する | true | 適用時の範囲は `1..32` |
| `wrap` | `boolean` | `true` | なし | `saya.options.wrap = false;` | 折り返し表示を設定する | true | presentation-owned |

### 現在 init.ts では設定できない候補

次の option は registry にありますが、`startup_public: false` です。
`init.ts` で設定しようとすると unsupported startup option になります。

| 名前 | 型 | alias | owner | startup public | 備考 |
| --- | --- | --- | --- | --- | --- |
| `backup` | `boolean` | `bk` | unsupported planned | false | 現在 startup では未公開 |
| `clipboard` | `string` | `cb` | host-owned | false | host clipboard 方針は未公開 |
| `fileencoding` | `string` | `fenc` | unsupported planned | false | file encoding 設定は未公開 |
| `fileformat` | `string` | `ff` | unsupported planned | false | file format 設定は未公開 |
| `markdownrender` | `boolean` | `mdrender` | presentation-owned | false | registry にはあるが startup では未公開 |
| `undofile` | `boolean` | `udf` | unsupported planned | false | persistent undo は未公開 |
| `writebackup` | `boolean` | `wb` | unsupported planned | false | write backup は未公開 |

## Keymaps

`saya.keymap.set(mode, lhs, action)` は startup 時に keymap を登録します。
`mode` は `"normal"`、`"insert"`、`"visual"` を受け付けます。`lhs` は
左辺 key sequence です。`action` は literal string か
`saya.commands.execute(name)` が返す startup command reference です。

```ts
saya.keymap.set("normal", "<leader>w", saya.commands.execute("writeCurrent"));
saya.keymap.set("insert", "jk", "\x1b");
```

効果: startup registry に keymap entry を追加します。`visual` mode は
既存 boot 経路向け command では normal mode として正規化される箇所がある
ため、visual 専用動作は実際の runtime 側の対応状況を確認してください。

## Commands

`saya.commands.register(name, callback)` は startup で command callback の
source を登録します。callback は runtime で実行されるため、callback 内では
Runtime API を使います。

```ts
saya.commands.register("showCurrentFile", async () => {
  const buffer = await saya.buffer.current();
  await saya.window.openFloat({
    content: { kind: "lines", lines: [buffer.path ?? "[No Name]"] },
    relativeTo: { kind: "cursor" },
    width: 50,
    height: 3,
    row: 1,
    col: 0,
    border: "single",
  });
});
```

`saya.commands.execute(name)` は、callback 内では command を実行します。
startup 直下では command reference を作るための marker を返します。

## Events

`saya.events.on(name, callback)` は runtime event handler を startup で登録
します。型定義と既存 docs が公開している event は次の 4 つです。

| event | payload | 主な用途 |
| --- | --- | --- |
| `bufferOpen` | `{ buffer }` | buffer を開いたときの初期化 |
| `bufferChanged` | `{ buffer }` | 入力や変更に応じた処理 |
| `bufferWritePost` | `{ buffer }` | 保存後の処理 |
| `bufferClosed` | `{ buffer }` | close 後の cleanup |

`buffer` snapshot は `id`、`path`、`lineCount`、`cursorRow`、`cursorCol`、
`currentLine`、`text` を持ちます。

```ts
saya.events.on("bufferWritePost", async (payload) => {
  if (payload.buffer.path?.endsWith(".md")) {
    console.log(`saved markdown: ${payload.buffer.path}`);
  }
});
```

注意: startup collection の実装は event 名として string を受け取りますが、
runtime seed は上記 4 event 以外をサポートしない可能性があります。LLM は
この 4 つだけを生成してください。

## Filetype Settings

`saya.ftplugin` は filetype ごとの option override を宣言します。
`saya.ftplugin.enabled` は全体の有効フラグ、`set()` は filetype 定義、
`disable()` は指定 filetype の無効化です。

```ts
saya.ftplugin.enabled = true;

saya.ftplugin.set("markdown", {
  extensions: ["md", "markdown"],
  options: {
    wrap: true,
    tabstop: 2,
    shiftwidth: 2,
  },
});

saya.ftplugin.disable("go");
```

filetype 名は trim され、小文字化されます。extension は trim され、先頭の
`.` が取り除かれ、小文字化されます。`extensions` を省略すると filetype 名
自体が extension として使われます。

現在の実装では、built-in Go definition として `.go` に
`expandtab=false`、`softtabstop=0`、`shiftwidth=0` が用意されています。
同じ extension に複数の定義が合う場合、後から収集された定義が勝つ設計です。

## Status Line

`saya.statusline.set(config)` は status line の segment を宣言します。
指定できる segment は `fileName`、`mode`、`filetype`、`modified` です。

```ts
saya.statusline.set({
  left: ["fileName", "mode"],
  right: ["filetype", "modified"],
});
```

デフォルトは left に `fileName`、`mode`、`filetype`、`modified`、right は
空です。未知の segment は startup evaluation error になります。

## Theme

`saya.theme` は semantic style を startup で収集します。代入できる領域は
`palette`、`ui`、`syntax`、`languages`、`filer`、`markdown` です。

```ts
saya.theme.palette = {
  fg: "#c0caf5",
  bg: "#1a1b26",
  accent: "#7aa2f7",
};

saya.theme.ui = {
  text: { fg: "fg", bg: "bg" },
  statusActive: { fg: "bg", bg: "accent", bold: true },
  warningMsg: { fg: "#e0af68", bold: true },
};

saya.theme.syntax = {
  comment: { fg: "#565f89", italic: true },
  string: { fg: "#9ece6a" },
  function: { fg: "accent" },
};

saya.theme.languages = {
  go: {
    syntax: {
      string: { fg: "#9ece6a" },
    },
  },
};
```

style property は `fg`、`bg`、`bold`、`italic`、`underline`、
`strikethrough` です。`ui` の key は `text`、`gutter`、`statusActive`、
`statusInactive`、`message`、`warningMsg`、`prompt` です。`syntax` の key は
`comment`、`string`、`constant`、`statement`、`identifier`、`type`、
`function`、`punctuation`、`markup`、`default` です。`filer` の key は
`directory`、`file`、`symlink`、`other`、`marked` です。`markdown` の key は
`heading`、`heading1` から `heading6`、`inlineCode`、`link`、`listMarker`、
`checkboxChecked`、`checkboxUnchecked`、`table`、`fencedCodeBlock` です。

## Logging

`saya.log.file` と `saya.log.level` は startup log 設定を収集します。

```ts
saya.log.file = "/tmp/saya.log";
saya.log.level = "trace";
```

`level` は `error`、`warn`、`info`、`debug`、`trace` を受け付けます。
複数回設定した場合は最後の値が使われます。環境変数 `SAYA_LOG_FILE` は
startup config の `saya.log.file` より優先されます。

## Plugins

`saya.plugins.use()` と `saya.plugins.lazy()` は plugin declaration を
startup で収集します。editor startup 中に repository を clone したり、
network へアクセスしたりする API ではありません。

```ts
saya.plugins.use([
  { local: "~/.config/saya/plugins/workspace-tools" },
  { github: "shun/saya-theme-tokyo-night", rev: "main" },
]);

saya.plugins.lazy([
  {
    github: "shun/saya-git-tools",
    commands: ["GitStatus", "GitBlame"],
  },
  {
    local: "~/.config/saya/plugins/workspace-tools",
    events: ["bufferOpen"],
  },
]);
```

plugin spec は `local` か `github` のどちらか一方を必ず指定します。
`github` は `owner/repository` 形式です。`module` の default は `mod.ts`、
`setup` の default は `setup` です。

注意: bundled plugin を直接 import して `setup...()` を呼ぶ方法と、
plugin manager declaration は別の入口です。bundled plugin の manifest は
lazy placeholder の metadata になりますが、manifest だけで keymap や source
が自動で有効になるわけではありません。

## Bundled Plugin Recipes

bundled plugin は preview を含みます。設定例は `plugins/ts/bundled/*/index.ts`
と各 manifest を根拠にしています。

### dired

dired は directory editor command を登録します。`keymap` を省略すると、
command は登録されますが normal mode keymap は入りません。

```ts
import { setupSayaDired } from "./plugins/bundled/dired/index.ts";

setupSayaDired({
  root: ".",
  hiddenFilePolicy: "hide",
  sortPolicy: "kind",
  filter: "rs",
  confirmStrategy: "preview",
  keymap: {
    up: "-",
    enter: "<Enter>",
    refresh: "gr",
    mark: "m",
    unmark: "M",
    clearMarks: "gM",
    bulkDeletePreview: "D",
  },
});
```

default command は `dired.open`、`dired.enter`、`dired.up`、`dired.refresh`、
`dired.mark`、`dired.unmark`、`dired.clearMarks`、
`dired.bulkDeletePreview` です。dired v1 は preview であり、recursive delete、
trash、remote、archive、広い filesystem access は現在の contract 外です。

### completion

completion は `completion.trigger` を登録します。source、manual keymap、
auto trigger は明示しない限り有効になりません。

```ts
import {
  createBufferWordSource,
  createLspCompletionSource,
  createPathCompletionSource,
  setupSayaCompletion,
} from "./plugins/bundled/completion/index.ts";

setupSayaCompletion({
  key: "<C-Space>",
  autoTrigger: true,
  autoTriggerDelayMs: 80,
  sources: [
    createLspCompletionSource({ minPrefixLength: 1 }),
    createPathCompletionSource({
      minPrefixLength: 1,
      triggerCharacters: ["/", "."],
    }),
    createBufferWordSource({ minPrefixLength: 2 }),
  ],
});
```

default menu key は confirm が `<Enter>`、`<Tab>`、`<C-y>`、close が
`<C-e>`、next が `<Down>`、`<C-n>`、previous が `<Up>`、`<C-p>` です。
LSP source は `lsp.completion` command を呼ぶため、LSP と併用する場合は
両方の plugin setup が必要です。

### lsp-client

LSP client は preview です。command は登録されますが、keymap と buffer
lifecycle event handler は明示 opt-in です。

```ts
import { setupSayaLspClient } from "./plugins/bundled/lsp-client/index.ts";

setupSayaLspClient({
  enableBufferEvents: true,
  keymap: {
    hover: "K",
    definition: "gd",
    references: "gR",
    documentSymbol: "gO",
    nextDiagnostic: "]d",
    previousDiagnostic: "[d",
  },
  trace: "messages",
  positionEncoding: "utf-16",
  languageIdByExtension: {
    go: "go",
  },
  servers: {
    go: {
      name: "gopls",
      command: "gopls",
      args: ["serve"],
      languages: ["go"],
      filePatterns: ["**/*.go"],
      rootMarkers: ["go.mod", ".git"],
      initializationOptions: {
        semanticTokens: true,
      },
    },
  },
});
```

未対応または preview の範囲には、semantic tokens、inlay hints、
hierarchy API、workspace symbols、workspace edits、incremental sync、
dynamic registration、remote / TCP server が含まれます。LSP integration は
runtime の `saya.lsp.connect()` を使う設計で、一般的な process 管理を
直接組む用途ではありません。

### agent

agent plugin は preview の panel surface を使い、AI CLI を terminal panel
として開く command を登録します。

```ts
import { setupSayaAgent } from "./plugins/bundled/agent/index.ts";

setupSayaAgent({
  id: "ai-agent",
  defaultTool: "codex",
  layout: { position: "right", size: "35%" },
  tools: {
    codex: { command: ["codex"] },
    gemini: { command: ["gemini"] },
    claude: { command: ["claude"] },
  },
  promptLibrary: [
    { name: "review", prompt: "Review the current file.\n" },
  ],
});
```

default command は `panel.toggle`、`panel.focus`、`panel.unfocus`、
`panel.close`、`panel.detach`、`agent.sendCurrentFile`、
`agent.sendCurrentLine`、`agent.sendSelectedRange`、`agent.sendPrompt` です。
`agent.sendPrompt` は現在、`promptLibrary` の最初の entry を送ります。

## Runtime Callback Recipes

runtime callback では Runtime API の snapshot と host capability を使います。

### 現在の buffer path を読む

```ts
saya.commands.register("echoPath", async () => {
  const path = await saya.buffer.currentPath();
  await saya.commands.execute(`echo ${path ?? "[No Name]"}`);
});
```

### floating window を開く

```ts
saya.commands.register("showLine", async () => {
  const buffer = await saya.buffer.current();
  await saya.window.openFloat({
    content: { kind: "lines", lines: [buffer.currentLine] },
    relativeTo: { kind: "cursor" },
    width: 72,
    height: 3,
    row: 1,
    col: 0,
    border: "single",
  });
});
```

### directory entry を操作する

```ts
saya.commands.register("markCurrentEntry", async () => {
  const entry = await saya.filer.currentEntry();
  if (!entry) return;
  await saya.filer.mark(entry.path);
});
```

### workspace root を探す

```ts
saya.commands.register("showRoot", async () => {
  const path = await saya.buffer.currentPath();
  const root = path
    ? await saya.workspace.findRoot(path, ["Cargo.toml", "go.mod", ".git"])
    : null;
  console.log(root);
});
```

## 未対応・注意事項

`init.ts` は startup declaration のための TypeScript API です。Vim script や
Vim / Neovim 互換 API ではありません。

- startup phase では、広い filesystem、network、live runtime state access は
  公開されていません。
- bare package import と network import は対象外です。local file import を
  使ってください。
- `set`、`nnoremap`、`autocmd` をそのまま書く形式ではありません。
- `saya.options.*` は canonical name を優先してください。alias は runtime
  payload で受け付けますが、`.d.ts` には載っていません。
- event 名は `bufferOpen`、`bufferChanged`、`bufferWritePost`、
  `bufferClosed` に限定してください。
- plugin manager declaration は startup 中に network 操作をしません。
- preview plugin は API が安定前に変わる可能性があります。
- `saya.plugins.loadLazy()` は Runtime API にありますが、現在の実装では
  module evaluation まで行う挙動は確認できません。

## 既存 docs の改善候補

調査中に見つかった、既存 docs の改善候補です。

- `startup-api.md` の Namespace 一覧に `saya.ftplugin` と
  `saya.statusline` が抜けています。
- `startup-api.md` の Theme 節に、実装済みの `saya.theme.languages` と
  `saya.theme.filer` の説明がありません。
- `startup-api.md` の Options 節は `tabstop`、`number`、`hlsearch` 中心で、
  startup public option 全体を説明していません。
- `README_ja.md` の「現在の startup API では、起動前に option、command、
  event を宣言できます」という文は、直後の例に `statusline` と
  `ftplugin` があるため更新余地があります。
- `startup-api.md` の LSP サンプルは `initializationOptions` 周辺の閉じ括弧が
  不足しているように見えます。
- import path の例が `./plugins/bundled/...` と
  `/path/to/plugins/bundled/...` で揺れています。配布時と開発 checkout 時の
  前提を分けると分かりやすくなります。
- `runtime_public_surface_names()` と runtime docs / `.d.ts` の間で、
  `workspace`、`fs` の public surface 表現に差分があります。
- `runtime_public_surface_paths()` には `saya.fs.readDir` が見当たりませんが、
  runtime docs と実装には存在します。

## 索引

この索引は、設定生成時に探しやすい名前から該当節へ戻るためのものです。

| 探したいこと | 参照先 |
| --- | --- |
| 行番号 | `よく使う設定レシピ`, `Options Reference` の `number` |
| インデント | `よく使う設定レシピ`, `Options Reference` の `tabstop`、`shiftwidth`、`expandtab` |
| filetype ごとの設定 | `Filetype Settings` |
| status line | `Status Line` |
| theme / Markdown | `Theme` |
| log | `Logging` |
| keymap | `Keymaps` |
| command callback | `Commands`, `Runtime Callback Recipes` |
| event callback | `Events`, `Runtime Callback Recipes` |
| dired | `Bundled Plugin Recipes` の `dired` |
| completion | `Bundled Plugin Recipes` の `completion` |
| LSP | `Bundled Plugin Recipes` の `lsp-client` |
| agent panel | `Bundled Plugin Recipes` の `agent` |
| 未対応事項 | `未対応・注意事項` |
