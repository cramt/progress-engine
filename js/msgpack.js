"use strict";
// Job results come back MessagePack-encoded (FINDINGS.md S7). Decode only -
// nothing in the protocol ever asks us to encode - so this is a few hundred
// bytes instead of a dependency the sandbox would have to be able to resolve.
((globalThis) => {
  globalThis.__delverDecodeMsgpack = (u8) => {
    const view = new DataView(u8.buffer, u8.byteOffset, u8.byteLength);
    let pos = 0;

    const str = (len) => {
      // Manual UTF-8: the sandbox has no TextDecoder, and these are short.
      let out = "";
      const end = pos + len;
      while (pos < end) {
        const b = u8[pos++];
        if (b < 0x80) {
          out += String.fromCharCode(b);
        } else if (b < 0xe0) {
          out += String.fromCharCode(((b & 0x1f) << 6) | (u8[pos++] & 0x3f));
        } else if (b < 0xf0) {
          out += String.fromCharCode(
            ((b & 0x0f) << 12) | ((u8[pos++] & 0x3f) << 6) | (u8[pos++] & 0x3f),
          );
        } else {
          const cp =
            (((b & 0x07) << 18) |
              ((u8[pos++] & 0x3f) << 12) |
              ((u8[pos++] & 0x3f) << 6) |
              (u8[pos++] & 0x3f)) - 0x10000;
          out += String.fromCharCode(0xd800 + (cp >> 10), 0xdc00 + (cp & 0x3ff));
        }
      }
      return out;
    };

    const value = () => {
      const tag = u8[pos++];
      if (tag < 0x80) return tag;                         // positive fixint
      if (tag >= 0xe0) return tag - 0x100;                // negative fixint
      if (tag < 0x90) {                                   // fixmap
        return map(tag & 0x0f);
      }
      if (tag < 0xa0) return array(tag & 0x0f);           // fixarray
      if (tag < 0xc0) return str(tag & 0x1f);             // fixstr

      switch (tag) {
        case 0xc0: return null;
        case 0xc2: return false;
        case 0xc3: return true;
        case 0xc4: return bin(u8[pos++]);
        case 0xc5: { const n = view.getUint16(pos); pos += 2; return bin(n); }
        case 0xc6: { const n = view.getUint32(pos); pos += 4; return bin(n); }
        case 0xca: { const v = view.getFloat32(pos); pos += 4; return v; }
        case 0xcb: { const v = view.getFloat64(pos); pos += 8; return v; }
        case 0xcc: return u8[pos++];
        case 0xcd: { const v = view.getUint16(pos); pos += 2; return v; }
        case 0xce: { const v = view.getUint32(pos); pos += 4; return v; }
        case 0xcf: { const v = view.getBigUint64(pos); pos += 8; return Number(v); }
        case 0xd0: { const v = view.getInt8(pos); pos += 1; return v; }
        case 0xd1: { const v = view.getInt16(pos); pos += 2; return v; }
        case 0xd2: { const v = view.getInt32(pos); pos += 4; return v; }
        case 0xd3: { const v = view.getBigInt64(pos); pos += 8; return Number(v); }
        case 0xd9: return str(u8[pos++]);
        case 0xda: { const n = view.getUint16(pos); pos += 2; return str(n); }
        case 0xdb: { const n = view.getUint32(pos); pos += 4; return str(n); }
        case 0xdc: { const n = view.getUint16(pos); pos += 2; return array(n); }
        case 0xdd: { const n = view.getUint32(pos); pos += 4; return array(n); }
        case 0xde: { const n = view.getUint16(pos); pos += 2; return map(n); }
        case 0xdf: { const n = view.getUint32(pos); pos += 4; return map(n); }
        default: throw new Error(`unsupported msgpack tag 0x${tag.toString(16)}`);
      }
    };

    const bin = (len) => {
      const out = u8.slice(pos, pos + len);
      pos += len;
      return out;
    };
    const array = (len) => {
      const out = new Array(len);
      for (let i = 0; i < len; i++) out[i] = value();
      return out;
    };
    const map = (len) => {
      const out = {};
      for (let i = 0; i < len; i++) out[value()] = value();
      return out;
    };

    return value();
  };
})(globalThis);
