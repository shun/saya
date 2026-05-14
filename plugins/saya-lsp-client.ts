// このファイルは互換維持のためのシムです。
// 実体は plugins/saya-lsp/ ディレクトリ配下のモジュール群に分割されています。
// 既存の import { ... } from "plugins/saya-lsp-client.ts" を壊さないため、
// すべての公開エントリポイントを ./saya-lsp/index.ts から取り込みます。
import { setupSayaLspClient, createLspJsonRpcClient, createLspMessageParser, encodeLspMessage, lspPositionFromSayaCursor, lspRangeFromSayaRange, parseLsifLine } from "./saya-lsp/index.ts";
