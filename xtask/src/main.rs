//! xtask：仓库统一构建/验证入口（`cargo xtask <子命令>`）。
//!
//! 子命令：
//! - `check-all`  feature 组合 `cargo check`（win `--all-features` / wasm32 `web` / android
//!   `native` / wasm32 `web+全部解码链`，见 `WEB_AUDIO_FEATURES`）
//! - `fmt`        `cargo fmt --all`（附加 `--check` 透传）
//! - `clippy`     workspace clippy（win + wasm，`-D warnings`）
//! - `web`        构建 wasm 示例（`cargo xtask web [示例名]`，缺省 `hello_window`）+ wasm-bindgen + 本地静态服务
//! - `golden`     离屏 golden 比对（`cargo xtask golden [--bless]`，跑 `GOLDENS` 里全部场景，仅本机 Windows 生效）
//! - `wasm-test`  wasm-pack 浏览器测试（T12 实装）
//! - `android`    cargo-ndk 构建 + adb 安装（T12 实装）

use std::env;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Command, exit};

const WASM_TARGET: &str = "wasm32-unknown-unknown";
const ANDROID_TARGET: &str = "aarch64-linux-android";
const WEB_PORT: u16 = 8080;
/// `cargo xtask web` 给 wasm 示例编的 feature 组合：平台 + T7 的四条解码链。
///
/// 为什么带上解码：`--features web` 单开时 `play_bytes` 一律 `UnsupportedFormat`，
/// 浏览器里就变成"看着动、听着没声"，`audio_demo` 的听测没法做。SoundFont 引擎不附带
/// （`midi` 只影响能否解 `.mid`，见 `docs/audio.md`）。
const WEB_AUDIO_FEATURES: &str = "web,ogg,mp3,wav,midi";
/// 离屏 golden 测试清单：`(crate, 测试文件)`，`cargo xtask golden` 逐个跑。
const GOLDENS: &[(&str, &str)] = &[
    ("sisf-sprite", "golden_sprite"),
    ("sisf-text", "golden_text"),
];

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    let sub = args.first().map(String::as_str).unwrap_or("");
    let rest: &[String] = if args.is_empty() { &[] } else { &args[1..] };
    let code = match sub {
        "check-all" => check_all(),
        "fmt" => run(
            "cargo",
            &[OsString::from("fmt"), OsString::from("--all")],
            rest_args(rest),
        ),
        "clippy" => clippy(),
        "web" => web(rest.first().map(String::as_str).unwrap_or("hello_window")),
        "golden" => golden(rest),
        "wasm-test" => not_implemented("wasm-test", "T12"),
        "android" => not_implemented("android", "T12"),
        _ => usage(),
    };
    if code != 0 {
        eprintln!("xtask `{sub}` 失败（exit {code}）");
        exit(code);
    }
}

fn usage() -> i32 {
    eprintln!("用法: cargo xtask <check-all|fmt|clippy|web|golden|wasm-test|android>");
    2
}

fn not_implemented(name: &str, stage: &str) -> i32 {
    eprintln!("`xtask {name}` 将在阶段 {stage} 实装（见 todo.md）");
    3
}

type OsString = std::ffi::OsString;

fn rest_args(rest: &[String]) -> Vec<OsString> {
    rest.iter().map(OsString::from).collect()
}

fn run(program: &str, fixed: &[OsString], extra: Vec<OsString>) -> i32 {
    let status = Command::new(program)
        .args(fixed)
        .args(&extra)
        .status()
        .unwrap_or_else(|e| panic!("无法启动 `{program}`: {e}"));
    status.code().unwrap_or(1)
}

fn check(cmd: &[&str]) -> i32 {
    println!("==> cargo {}", cmd.join(" "));
    let status = Command::new("cargo")
        .args(cmd)
        .status()
        .unwrap_or_else(|e| panic!("无法启动 cargo: {e}"));
    status.code().unwrap_or(1)
}

/// 离屏 golden：跑各 crate 的 golden 场景，与 `tests/golden/` 基线逐通道比对。
/// `--bless` 刷新基线（首次落图也用这个）。
///
/// 这些测试整个 `#![cfg(windows)]`：wgpu 30 没有软件光栅后端，离屏同样要真 adapter，
/// 所以在其它平台会编译成 0 个用例并通过 —— 只有本机有 DX12/Vulkan 时才是真比对。
fn golden(rest: &[String]) -> i32 {
    let mut bless = false;
    for arg in rest {
        match arg.as_str() {
            "--bless" => bless = true,
            other => {
                eprintln!("未知参数 `{other}`；用法: cargo xtask golden [--bless]");
                return 2;
            }
        }
    }
    for (package, test) in GOLDENS {
        println!(
            "==> cargo test -p {package} --test {test}{}",
            if bless {
                "  (bless：重写基线)"
            } else {
                ""
            }
        );
        let mut cmd = Command::new("cargo");
        cmd.args(["test", "-p", package, "--test", test]);
        if bless {
            cmd.env("SISF_GOLDEN", "bless");
        }
        let status = cmd
            .status()
            .unwrap_or_else(|e| panic!("无法启动 cargo: {e}"));
        let code = status.code().unwrap_or(1);
        if code != 0 {
            eprintln!("golden 在 {package}/{test} 失败");
            return code;
        }
    }
    0
}

fn check_all() -> i32 {
    let combos: &[(&str, Vec<&str>)] = &[
        (
            "native(宿主)",
            vec!["check", "-p", "sisf", "--all-targets", "--all-features"],
        ),
        (
            "wasm32",
            vec![
                "check",
                "-p",
                "sisf",
                "--all-targets",
                "--target",
                WASM_TARGET,
                "--no-default-features",
                "--features",
                "web",
            ],
        ),
        (
            "android",
            vec![
                "check",
                "-p",
                "sisf",
                "--all-targets",
                "--target",
                ANDROID_TARGET,
                "--no-default-features",
                "--features",
                "native",
            ],
        ),
        (
            "wasm32 音频全开(ogg/mp3/wav/midi)",
            vec![
                "check",
                "-p",
                "sisf",
                "--all-targets",
                "--target",
                WASM_TARGET,
                "--no-default-features",
                "--features",
                WEB_AUDIO_FEATURES,
            ],
        ),
    ];
    for (label, args) in combos {
        println!("---- check-all: {label} ----");
        let code = check(args);
        if code != 0 {
            eprintln!("check-all 在 {label} 组合失败");
            return code;
        }
    }
    0
}

fn clippy() -> i32 {
    let code = check(&[
        "clippy",
        "--workspace",
        "--all-targets",
        "--",
        "-D",
        "warnings",
    ]);
    if code != 0 {
        return code;
    }
    check(&[
        "clippy",
        "-p",
        "sisf",
        "--all-targets",
        "--target",
        WASM_TARGET,
        "--no-default-features",
        "--features",
        "web",
        "--",
        "-D",
        "warnings",
    ])
}

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .to_path_buf()
}

fn web(example: &str) -> i32 {
    let code = check(&[
        "build",
        "-p",
        "sisf",
        "--example",
        example,
        "--target",
        WASM_TARGET,
        "--no-default-features",
        "--features",
        WEB_AUDIO_FEATURES,
    ]);
    if code != 0 {
        return code;
    }
    let root = workspace_root();
    let wasm = root
        .join("target")
        .join(WASM_TARGET)
        .join("debug")
        .join("examples")
        .join(format!("{example}.wasm"));
    if !wasm.exists() {
        eprintln!("找不到产物 {}", wasm.display());
        return 4;
    }
    let out_dir = root.join("web").join("pkg");
    let status = Command::new("wasm-bindgen")
        .args(["--target", "web", "--out-dir"])
        .arg(&out_dir)
        .arg(&wasm)
        .status();
    match status {
        Ok(s) if s.success() => {}
        Ok(s) => {
            eprintln!("wasm-bindgen 失败（exit {:?}）", s.code());
            return s.code().unwrap_or(5);
        }
        Err(e) => {
            eprintln!(
                "wasm-bindgen 不可用：{e}（cargo install wasm-bindgen-cli --version 与 wasm-bindgen 钉版一致）"
            );
            return 5;
        }
    }
    serve_dir(&root.join("web"))
}

fn serve_dir(dir: &Path) -> i32 {
    let addr = format!("127.0.0.1:{WEB_PORT}");
    let listener = match TcpListener::bind(&addr) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("无法监听 {addr}: {e}");
            return 6;
        }
    };
    println!("服务已启动: http://{addr}/index.html");
    println!("Ctrl+C 退出。WebGL2 回退访问 http://{addr}/index.html?wgl");
    println!(
        "换示例：cargo xtask web <示例名>，然后访问 http://{addr}/?ex=<示例名>（见 web/main.js）"
    );
    for stream in listener.incoming() {
        match stream {
            Ok(s) => handle_http(s, dir),
            Err(e) => eprintln!("连接错误: {e}"),
        }
    }
    0
}

fn handle_http(mut stream: TcpStream, dir: &Path) {
    let mut reader = BufReader::new(stream.try_clone().unwrap());
    let mut line = String::new();
    if reader.read_line(&mut line).is_err() {
        return;
    }
    // 只支持最简单的 GET
    let mut parts = line.split_whitespace();
    if parts.next() != Some("GET") {
        let _ = write!(
            &mut stream,
            "HTTP/1.1 405 Method Not Allowed\r\nConnection: close\r\n\r\n"
        );
        return;
    }
    let path = parts.next().unwrap_or("/");
    let rel = path
        .split(['?'])
        .next()
        .unwrap_or("/")
        .trim_start_matches('/');
    let rel = if rel.is_empty() { "index.html" } else { rel };
    let file = dir.join(rel);
    let canonical_dir = dir.canonicalize().unwrap_or_else(|_| dir.to_path_buf());
    let canonical = file.canonicalize().unwrap_or_else(|_| file.clone());
    if !canonical.starts_with(&canonical_dir) || !canonical.is_file() {
        let _ = write!(
            &mut stream,
            "HTTP/1.1 404 Not Found\r\nConnection: close\r\n\r\n"
        );
        return;
    }
    let mut bytes = Vec::new();
    if std::fs::File::open(&canonical)
        .and_then(|mut f| f.read_to_end(&mut bytes))
        .is_err()
    {
        let _ = write!(
            &mut stream,
            "HTTP/1.1 500 Internal Server Error\r\nConnection: close\r\n\r\n"
        );
        return;
    }
    let ext = canonical.extension().and_then(|e| e.to_str()).unwrap_or("");
    let ctype = match ext {
        "html" => "text/html",
        "js" => "text/javascript",
        "wasm" => "application/wasm",
        "css" => "text/css",
        _ => "application/octet-stream",
    };
    // `no-store` 是有意的：这里没有 `ETag`/`Last-Modified` 之类的验证器，浏览器只能启发式缓存，
    // 于是改完代码重编后普通刷新还在跑旧的 32MB wasm（看着像"修了没生效"）。开发服务宁可每次都读盘。
    let head = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: {ctype}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n",
        bytes.len()
    );
    let _ = stream.write_all(head.as_bytes());
    let _ = stream.write_all(&bytes);
}
