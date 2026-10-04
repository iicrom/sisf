//! 解密密钥：只在运行时从 `data/System.json` 的 `encryptionKey` 取。
//!
//! 源码里**不许出现那串 hex**（AGENTS.md M0.4；`key_never_hardcoded` 钉住这条）。
//! MV 的读点是 `Decrypter.readEncryptionkey`：`encryptionKey.split(/(.{2})/).filter(Boolean)`
//! 再逐对 `parseInt(..,16)` ⇒ 32 个 hex 字符折成 16 字节。

use crate::decrypt::HEADER_LEN;

/// 密钥取不到。DoD 口径是"不猜密钥"，所以这里全是硬错误，没有默认值。
#[derive(Debug, PartialEq, Eq)]
pub enum KeyError {
    /// `data/System.json` 里没有 `encryptionKey` 这个键。
    MissingField,
    /// 键在，但长度不是 32 个 hex 字符（附带读到的长度）。
    BadLength(usize),
    /// 长度对，但不是 hex。
    NotHex,
    /// JSON 本身读不动。
    Malformed(String),
}

impl std::fmt::Display for KeyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            KeyError::MissingField => write!(f, "System.json 没有 encryptionKey 字段"),
            KeyError::BadLength(n) => {
                write!(f, "encryptionKey 长度是 {n}，需要 32 个 hex 字符")
            }
            KeyError::NotHex => write!(f, "encryptionKey 含非 hex 字符"),
            KeyError::Malformed(e) => write!(f, "System.json 解析失败: {e}"),
        }
    }
}

impl std::error::Error for KeyError {}

/// 从 `data/System.json` 的文本里取密钥。
pub fn from_system_json(src: &str) -> Result<[u8; HEADER_LEN], KeyError> {
    let value: serde_json::Value =
        serde_json::from_str(src).map_err(|e| KeyError::Malformed(e.to_string()))?;
    // 上游写的是小写 hex，但 MV 只做 parseInt(16) ⇒ 大小写都能解。
    let hex = match value.get("encryptionKey") {
        None => return Err(KeyError::MissingField),
        Some(serde_json::Value::String(s)) => s.as_str(),
        Some(other) => {
            return Err(KeyError::BadLength(other.to_string().chars().count()));
        }
    };
    from_hex(hex)
}

/// 32 个 hex 字符 → 16 字节。
pub fn from_hex(hex: &str) -> Result<[u8; HEADER_LEN], KeyError> {
    let chars: Vec<char> = hex.chars().collect();
    if chars.len() != HEADER_LEN * 2 {
        return Err(KeyError::BadLength(chars.len()));
    }
    let mut key = [0u8; HEADER_LEN];
    for (i, pair) in chars.chunks(2).enumerate() {
        let hi = hex_val(pair[0]).ok_or(KeyError::NotHex)?;
        let lo = hex_val(pair[1]).ok_or(KeyError::NotHex)?;
        key[i] = hi << 4 | lo;
    }
    Ok(key)
}

fn hex_val(c: char) -> Option<u8> {
    c.to_digit(16).and_then(|d| u8::try_from(d).ok())
}

#[cfg(test)]
mod tests {
    //! 机制那三条（纯函数、不碰磁盘 ⇒ 两条腿都数得到）。真数据那两条（拿 `System.json` 与真
    //! 素材对拍、以及"源码里 grep 不到密钥"）在 `pray/src/key.rs` —— 这一层不许知道某一个
    //! 工程在哪，而"扫源码"那一条扫的是**消费方的源码**，天然住在消费方。
    use super::*;

    // 纯机制判据也要在浏览器/node 那条腿上数得到 —— `#[test]` 在 wasm32 上不会被
    // `wasm-bindgen-test-runner` 登记，得用同名别名（同 `shapes`/`plugins`/`platform` 的写法）。
    #[cfg(target_arch = "wasm32")]
    use wasm_bindgen_test::wasm_bindgen_test as test;

    /// 一条合格的样例，与任何真密钥无关 —— 只用来构造 JSON。
    const FAKE_HEX: &str = "aabbccddeeff00112233445566778899";

    fn system_json(value: serde_json::Value) -> String {
        serde_json::json!({"gameTitle": "t", "encryptionKey": value}).to_string()
    }

    #[test]
    fn key_missing_errors() {
        assert_eq!(
            from_system_json(&serde_json::json!({"gameTitle": "t"}).to_string()),
            Err(KeyError::MissingField)
        );
    }

    #[test]
    fn key_bad_length_and_non_hex_error() {
        assert_eq!(
            from_system_json(&system_json("abcd".into())),
            Err(KeyError::BadLength(4))
        );
        assert_eq!(
            from_system_json(&system_json("zz".repeat(16).into())),
            Err(KeyError::NotHex)
        );
        // 不是字符串那一格：长度按 `to_string()` 的字符数报（`123` ⇒ 3）。
        assert_eq!(
            from_system_json(&system_json(serde_json::json!(123))),
            Err(KeyError::BadLength(3))
        );
        assert!(matches!(
            from_system_json("not json"),
            Err(KeyError::Malformed(_))
        ));
    }

    #[test]
    fn hex_roundtrip_and_case_insensitive() {
        let key = from_hex(FAKE_HEX).unwrap();
        assert_eq!(key[0], 0xaa);
        assert_eq!(key[15], 0x99);
        // 上游写的是小写 hex，但 MV 只做 `parseInt(.., 16)` ⇒ 大写必须解成同一份。
        assert_eq!(from_hex(&FAKE_HEX.to_ascii_uppercase()).unwrap(), key);
    }
}
