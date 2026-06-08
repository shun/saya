// deno-fmt-ignore-file

// UTF-8 codec ヘルパ。
//
// seed runtime / production runtime のいずれにも TextEncoder / TextDecoder
// が公開されていないため、UTF-8 への encode / decode と UTF-8 byte 単位の
// 境界計算をすべて自前で実装する。
//
// 名前空間規約: ファイル top-level に 1 個の const オブジェクトのみを
// 置く。本ファイルは '__lspUtf8' 名前空間を公開する。
//
// 公開 API -
// - encodeBytes(text: string): Uint8Array
//     文字列を UTF-8 バイト列としてエンコードする。
// - decodeBytes(bytes: Uint8Array): string
//     UTF-8 バイト列を文字列にデコードする。不正シーケンスは
//     U+FFFD に置換する。
// - byteLength(text: string): number
//     UTF-8 エンコード後のバイト長を返す（実体化せず計算する）。
// - prefixCharLength(text: string, byteLength: number): number
//     UTF-8 で指定バイト長を満たす最長 prefix の UTF-16 length を返す。
// - normalizeByteOffset(text: string, byteOffset: number): number
//     文字列長を超えるバイトオフセットを安全に丸める。
const __lspUtf8 = {
  encodeBytes(text: string): Uint8Array {
    const safeText = String(text ?? "");
    const length = __lspUtf8.byteLength(safeText);
    const out = new Uint8Array(length);
    let writeIndex = 0;
    let readIndex = 0;
    while (readIndex < safeText.length) {
      const codePoint = safeText.codePointAt(readIndex);
      if (codePoint == null) {
        break;
      }
      if (codePoint <= 0x7f) {
        out[writeIndex] = codePoint;
        writeIndex = writeIndex + 1;
        readIndex = readIndex + 1;
      } else if (codePoint <= 0x7ff) {
        out[writeIndex] = 0xc0 | (codePoint >> 6);
        out[writeIndex + 1] = 0x80 | (codePoint & 0x3f);
        writeIndex = writeIndex + 2;
        readIndex = readIndex + 1;
      } else if (codePoint <= 0xffff) {
        out[writeIndex] = 0xe0 | (codePoint >> 12);
        out[writeIndex + 1] = 0x80 | ((codePoint >> 6) & 0x3f);
        out[writeIndex + 2] = 0x80 | (codePoint & 0x3f);
        writeIndex = writeIndex + 3;
        readIndex = readIndex + 1;
      } else {
        out[writeIndex] = 0xf0 | (codePoint >> 18);
        out[writeIndex + 1] = 0x80 | ((codePoint >> 12) & 0x3f);
        out[writeIndex + 2] = 0x80 | ((codePoint >> 6) & 0x3f);
        out[writeIndex + 3] = 0x80 | (codePoint & 0x3f);
        writeIndex = writeIndex + 4;
        readIndex = readIndex + 2;
      }
    }
    return out;
  },

  decodeBytes(bytes: Uint8Array): string {
    if (!(bytes instanceof Uint8Array)) {
      throw new TypeError("decodeBytes expects a Uint8Array");
    }
    let result = "";
    let index = 0;
    while (index < bytes.length) {
      const byte0 = bytes[index];
      let codePoint = 0xfffd;
      let consumed = 1;
      if (byte0 <= 0x7f) {
        codePoint = byte0;
        consumed = 1;
      } else if ((byte0 & 0xe0) === 0xc0 && index + 1 < bytes.length) {
        codePoint = ((byte0 & 0x1f) << 6) | (bytes[index + 1] & 0x3f);
        consumed = 2;
      } else if ((byte0 & 0xf0) === 0xe0 && index + 2 < bytes.length) {
        codePoint =
          ((byte0 & 0x0f) << 12) |
          ((bytes[index + 1] & 0x3f) << 6) |
          (bytes[index + 2] & 0x3f);
        consumed = 3;
      } else if ((byte0 & 0xf8) === 0xf0 && index + 3 < bytes.length) {
        codePoint =
          ((byte0 & 0x07) << 18) |
          ((bytes[index + 1] & 0x3f) << 12) |
          ((bytes[index + 2] & 0x3f) << 6) |
          (bytes[index + 3] & 0x3f);
        consumed = 4;
      }
      result = result + String.fromCodePoint(codePoint);
      index = index + consumed;
    }
    return result;
  },

  byteLength(text: string): number {
    const safeText = String(text ?? "");
    let length = 0;
    let index = 0;
    while (index < safeText.length) {
      const codePoint = safeText.codePointAt(index);
      if (codePoint == null) {
        break;
      }
      if (codePoint > 0xffff) {
        index = index + 2;
      } else {
        index = index + 1;
      }
      if (codePoint <= 0x7f) {
        length = length + 1;
      } else if (codePoint <= 0x7ff) {
        length = length + 2;
      } else if (codePoint <= 0xffff) {
        length = length + 3;
      } else {
        length = length + 4;
      }
    }
    return length;
  },

  prefixCharLength(text: string, byteLength: number): number {
    const safeText = String(text ?? "");
    const targetBytes = Math.max(0, Number(byteLength) || 0);
    let consumedBytes = 0;
    let index = 0;
    while (index < safeText.length && consumedBytes < targetBytes) {
      const codePoint = safeText.codePointAt(index);
      let charByteLength = 4;
      let charLength = 1;
      if (codePoint != null && codePoint > 0xffff) {
        charLength = 2;
      }
      if (codePoint != null && codePoint <= 0x7f) {
        charByteLength = 1;
      } else if (codePoint != null && codePoint <= 0x7ff) {
        charByteLength = 2;
      } else if (codePoint != null && codePoint <= 0xffff) {
        charByteLength = 3;
      }
      if (consumedBytes + charByteLength > targetBytes) {
        break;
      }
      consumedBytes = consumedBytes + charByteLength;
      index = index + charLength;
    }
    return index;
  },

  normalizeByteOffset(text: string, byteOffset: number): number {
    const safeText = String(text ?? "");
    const target = Math.max(0, Math.min(Number(byteOffset) || 0, __lspUtf8.byteLength(safeText)));
    let consumedBytes = 0;
    let index = 0;
    while (index < safeText.length && consumedBytes < target) {
      const codePoint = safeText.codePointAt(index);
      let charByteLength = 4;
      let charLength = 1;
      if (codePoint != null && codePoint > 0xffff) {
        charLength = 2;
      }
      if (codePoint != null && codePoint <= 0x7f) {
        charByteLength = 1;
      } else if (codePoint != null && codePoint <= 0x7ff) {
        charByteLength = 2;
      } else if (codePoint != null && codePoint <= 0xffff) {
        charByteLength = 3;
      }
      if (consumedBytes + charByteLength > target) {
        break;
      }
      consumedBytes = consumedBytes + charByteLength;
      index = index + charLength;
    }
    return consumedBytes;
  },
};
