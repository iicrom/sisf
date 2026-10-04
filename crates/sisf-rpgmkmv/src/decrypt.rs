//! RPG Maker MV 的素材解密（照 `js/rpg_core.js` 的 `Decrypter.decryptArrayBuffer`）。
//!
//! 磁盘上的 `.rpgmvp/.rpgmvo/.rpgmvm` = 16 字节头 + 原文件，且**只有原文件的前 16 字节**
//! 与密钥逐位 XOR（上游的循环上界就是 `_headerlength`，后面的字节一个都不动）。
//! 头必须是这 16 字节逐位相等 —— 上游对不符的头 `throw`，本函数返回 [`None`]，
//! 由调用方决定是"明文直读"还是报错，不 panic、也不静默原样返回。

use std::borrow::Cow;

/// 头长，同时也是 XOR 的作用跨度（上游两个循环都用它）。
pub const HEADER_LEN: usize = 16;

/// 16 字节头：`SIGNATURE` `5250474d56000000` + `VER` `000301` + `REMAIN` `0000000000`。
pub const HEADER_MAGIC: [u8; HEADER_LEN] = [
    0x52, 0x50, 0x47, 0x4d, 0x56, 0x00, 0x00, 0x00, 0x00, 0x03, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00,
];

/// 这段字节是不是 MV 的加密容器（长度够且 16 字节头逐位相等）。
pub fn is_encrypted(data: &[u8]) -> bool {
    data.len() >= HEADER_LEN && data[..HEADER_LEN] == HEADER_MAGIC
}

/// 解一层：校验头 → 切掉 16 字节 → 剩下的前 16 字节与 `key` XOR。
///
/// 头不符返回 [`None`]。正文短于 16 字节时按上游的 typed-array 语义处理 —— 存在几个字节
/// 就 XOR 几个字节，不补齐、不 panic。
pub fn decrypt<'a>(bytes: &'a [u8], key: &[u8; HEADER_LEN]) -> Option<Cow<'a, [u8]>> {
    if !is_encrypted(bytes) {
        return None;
    }
    let mut out = bytes[HEADER_LEN..].to_vec();
    for (b, k) in out.iter_mut().zip(key) {
        *b ^= *k;
    }
    Some(Cow::Owned(out))
}

/// 需要解密的扩展名 → 磁盘上的加密扩展名（上游 `extToEncryptExt` 只认这三条）。
pub fn encrypted_ext(ext: &str) -> Option<&'static str> {
    match ext {
        "png" => Some("rpgmvp"),
        "ogg" => Some("rpgmvo"),
        "m4a" => Some("rpgmvm"),
        _ => None,
    }
}
