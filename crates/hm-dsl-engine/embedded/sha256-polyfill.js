// FIPS 180-4 SHA-256 polyfill for QuickJS (no Web Crypto, no Node.js APIs).
// Exposes globalThis.createHash("sha256") matching the Node.js subset used by harmont-ts.
(function () {
  "use strict";

  var K = [
    0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5,
    0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5,
    0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3,
    0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
    0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc,
    0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
    0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7,
    0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
    0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13,
    0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
    0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3,
    0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
    0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5,
    0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
    0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208,
    0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
  ];

  function utf8ToBytes(str) {
    var bytes = [];
    for (var i = 0; i < str.length; i++) {
      var c = str.charCodeAt(i);
      if (c < 0x80) {
        bytes.push(c);
      } else if (c < 0x800) {
        bytes.push(0xc0 | (c >> 6), 0x80 | (c & 0x3f));
      } else if (c >= 0xd800 && c <= 0xdbff && i + 1 < str.length) {
        var next = str.charCodeAt(i + 1);
        if (next >= 0xdc00 && next <= 0xdfff) {
          var cp = ((c - 0xd800) << 10) + (next - 0xdc00) + 0x10000;
          bytes.push(
            0xf0 | (cp >> 18),
            0x80 | ((cp >> 12) & 0x3f),
            0x80 | ((cp >> 6) & 0x3f),
            0x80 | (cp & 0x3f)
          );
          i++;
        } else {
          bytes.push(0xef, 0xbf, 0xbd);
        }
      } else if (c >= 0xdc00 && c <= 0xdfff) {
        bytes.push(0xef, 0xbf, 0xbd);
      } else {
        bytes.push(0xe0 | (c >> 12), 0x80 | ((c >> 6) & 0x3f), 0x80 | (c & 0x3f));
      }
    }
    return bytes;
  }

  function rotr(x, n) { return ((x >>> n) | (x << (32 - n))) >>> 0; }

  function sha256(msgBytes) {
    var h0 = 0x6a09e667, h1 = 0xbb67ae85, h2 = 0x3c6ef372, h3 = 0xa54ff53a;
    var h4 = 0x510e527f, h5 = 0x9b05688c, h6 = 0x1f83d9ab, h7 = 0x5be0cd19;

    var msgLen = msgBytes.length;
    var bitLen = msgLen * 8;

    // Padding: append 0x80, then zeros, then 64-bit big-endian length.
    msgBytes.push(0x80);
    while (msgBytes.length % 64 !== 56) {
      msgBytes.push(0);
    }
    // Append bit length as 64-bit big-endian (we only support < 2^32 bits).
    for (var i = 56; i >= 0; i -= 8) {
      if (i >= 32) {
        msgBytes.push(0);
      } else {
        msgBytes.push((bitLen >>> i) & 0xff);
      }
    }

    var W = new Array(64);

    for (var offset = 0; offset < msgBytes.length; offset += 64) {
      for (var t = 0; t < 16; t++) {
        var base = offset + t * 4;
        W[t] = ((msgBytes[base] << 24) | (msgBytes[base + 1] << 16) |
                 (msgBytes[base + 2] << 8) | msgBytes[base + 3]) >>> 0;
      }
      for (var t = 16; t < 64; t++) {
        var s0 = rotr(W[t - 15], 7) ^ rotr(W[t - 15], 18) ^ (W[t - 15] >>> 3);
        var s1 = rotr(W[t - 2], 17) ^ rotr(W[t - 2], 19) ^ (W[t - 2] >>> 10);
        W[t] = (W[t - 16] + s0 + W[t - 7] + s1) >>> 0;
      }

      var a = h0, b = h1, c = h2, d = h3;
      var e = h4, f = h5, g = h6, h = h7;

      for (var t = 0; t < 64; t++) {
        var S1 = rotr(e, 6) ^ rotr(e, 11) ^ rotr(e, 25);
        var ch = (e & f) ^ (~e & g);
        var temp1 = (h + S1 + ch + K[t] + W[t]) >>> 0;
        var S0 = rotr(a, 2) ^ rotr(a, 13) ^ rotr(a, 22);
        var maj = (a & b) ^ (a & c) ^ (b & c);
        var temp2 = (S0 + maj) >>> 0;

        h = g; g = f; f = e;
        e = (d + temp1) >>> 0;
        d = c; c = b; b = a;
        a = (temp1 + temp2) >>> 0;
      }

      h0 = (h0 + a) >>> 0; h1 = (h1 + b) >>> 0;
      h2 = (h2 + c) >>> 0; h3 = (h3 + d) >>> 0;
      h4 = (h4 + e) >>> 0; h5 = (h5 + f) >>> 0;
      h6 = (h6 + g) >>> 0; h7 = (h7 + h) >>> 0;
    }

    return [h0, h1, h2, h3, h4, h5, h6, h7];
  }

  function toHex(words) {
    var hex = "";
    for (var i = 0; i < words.length; i++) {
      for (var j = 24; j >= 0; j -= 8) {
        var byte = (words[i] >>> j) & 0xff;
        hex += (byte < 16 ? "0" : "") + byte.toString(16);
      }
    }
    return hex;
  }

  function createHash(algo) {
    if (algo !== "sha256") {
      throw new Error("Unsupported algorithm: " + algo + " (only sha256 is available)");
    }
    var buffer = [];
    return {
      update: function (data, _encoding) {
        if (typeof data === "string") {
          var bytes = utf8ToBytes(data);
          for (var i = 0; i < bytes.length; i++) {
            buffer.push(bytes[i]);
          }
        } else {
          for (var i = 0; i < data.length; i++) {
            buffer.push(data[i]);
          }
        }
        return this;
      },
      digest: function (encoding) {
        var words = sha256(buffer);
        if (encoding === "hex") {
          return toHex(words);
        }
        throw new Error("Unsupported encoding: " + encoding + " (only hex is available)");
      },
    };
  }

  globalThis.createHash = createHash;
})();
