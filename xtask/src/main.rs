//! xtask：仓库统一构建/验证入口（`cargo xtask <子命令>`）。
//!
//! 子命令：
//! - `check-all`  feature 组合 `cargo check`（win `--all-features` / wasm32 `web` / android
//!   `native` / wasm32 `web+全部解码链+移植层`，见 `web_features()`）
//! - `fmt`        `cargo fmt --all`（附加 `--check` 透传）
//! - `clippy`     workspace clippy（win + wasm，`-D warnings`）
//! - `web`        构建 wasm 示例（`cargo xtask web [示例名] [--no-serve]`，缺省 `hello_window`）
//!   + wasm-bindgen + 本地静态服务（`--no-serve` 给 CI：只出产物，不起服务）
//! - `golden`     离屏 golden 比对（`cargo xtask golden [--bless]`，跑 `GOLDENS` 里全部场景，仅本机 Windows 生效）
//! - `wasm-test`  纯逻辑测试跑在 wasm32 上（`cargo xtask wasm-test [透传给 harness 的参数]`）
//! - `android`    NDK/adb 体检 + cargo-ndk 构建流程（`cargo xtask android [build <游戏 crate>]`）

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
/// T11 起 wasm 侧还要能编"移植层 + 视频桩"那两条腿：`cargo xtask web rpgmv_shell` 用的就是
/// 这一串（拼在 [`WEB_AUDIO_FEATURES`] 后面）。T11 续再加 `js`：wasm 侧的 JS 运行时腿
/// （`sisf-event/js` = js-sys，用浏览器自带引擎，不拉 rquickjs 那条 C）。
///
/// 为什么并进**同一条**构建配方而不是给 `rpgmv_shell` 单开一条：CI 上传的是 `web/pkg`，
/// 本地手测与 CI 必须编的是同一个 feature 集，否则"CI 绿、浏览器里那条 Custom 没落地"
/// 这种红会分不清是代码还是配方。代价是其余示例也带上这两个 crate 的编译时间。
const WEB_PORTING_FEATURES: &str = "rpgmk,video,js";
/// [`WEB_AUDIO_FEATURES`] 与 [`WEB_PORTING_FEATURES`] 拼成的一串（唯一的拼法，两处共用）。
fn web_features() -> String {
    format!("{WEB_AUDIO_FEATURES},{WEB_PORTING_FEATURES}")
}
/// 离屏 golden 测试清单：`(crate, 测试文件)`，`cargo xtask golden` 逐个跑。
const GOLDENS: &[(&str, &str)] = &[
    ("sisf-sprite", "golden_sprite"),
    ("sisf-text", "golden_text"),
];
/// `cargo xtask wasm-test` 的清单：
/// `(package, 额外的 cargo 参数, 透传给 harness 的参数, 这一腿跑到的是什么)`。
///
/// 只列**纯逻辑**套件。`wasm32-unknown-unknown` 上没有文件系统、没有线程，
/// `std::time::Instant::now()` 还直接 panic（todo.md 的 T3 事实第 7 条），所以
/// `asset::vfs`（临时目录）、`asset::store`/`asset::tasks`（线程泵）、`media`（fixture 读盘）、
/// `text`（字体扫描 + 读盘）与 `render` 的 `gpu_tests`（要真 adapter）**不在这一腿**——
/// 它们仍由 `cargo test --workspace --all-features` 与 `cargo xtask golden` 覆盖。
///
/// 一个套件要能被这一腿跑到，它的 `mod tests` 里必须有
/// `#[cfg(target_arch = "wasm32")] use wasm_bindgen_test::wasm_bindgen_test as test;`
/// （没有那一行，`#[test]` 归 libtest 所有，而 wasm 的 runner 只认 `#[wasm_bindgen_test]`
/// ⇒ 0 个用例、退出码 0。下面那个 `passed == 0` 的检查就是为这种"绿着骗过去"准备的）。
///
/// `sisf-app` 拆成两腿是绕开 node 的退出断言，理由与判据见 [`APP_LEG_SPLIT`]。
const WASM_TESTS: &[(&str, &[&str], &[&str], &str)] = &[
    ("sisf-asset", &[], &[], "VfsPath 规范化（path）"),
    (
        "sisf-audio",
        &["--no-default-features"],
        &[],
        "mixer 确定性（不带 pump feature ⇒ 只有纯混音器）",
    ),
    (
        "sisf-render",
        &[],
        &[],
        "atlas 装箱 / batch 排序 / ring / vertex / clip",
    ),
    (
        "sisf-sprite",
        &[],
        &[],
        "arena / draw / sprite / anim / task",
    ),
    (
        "sisf-app",
        &["--no-default-features", "--features", "web"],
        &["--skip", "normalize", "--skip", "custom"],
        APP_LEG_SPLIT,
    ),
    (
        "sisf-app",
        &["--no-default-features", "--features", "web"],
        &["--skip", "frame", "--skip", "timing"],
        "normalize（winit 事件翻译）+ custom",
    ),
    (
        "sisf-event",
        &[],
        &[],
        "command / vars / runner / script + bus / input（事件 VM）",
    ),
    (
        "sisf-event",
        &["--features", "js"],
        &["js::"],
        "js 桥账本（宿主 JS 引擎，与 native 同一份脚本判据）",
    ),
    (
        "sisf-rpgmk",
        &[],
        &[],
        "map / translate / game_vars / scene（MV 兼容层纯逻辑）",
    ),
];
/// `sisf-app` 两腿的分法说明（第一腿的 `what` 字段）。
///
/// 全量 25 个用例在这个二进制上跑：`test result: ok. 25 passed` 打出来之后，
/// node **退出时**撞 libuv 断言 `!(handle->flags & UV_HANDLE_CLOSING),
/// file src\win\async.c, line 76`，退出码 `0xC0000409`（`cargo test` 报
/// `Error: Node failed with exit_code -1073740791`）。用例一条没失败，崩的是 harness。
/// 本机量到的边界：`frame`+`normalize` 一起 ≥13 个必崩，≤11 个不崩；
/// `frame`+`timing`(15)、`timing`+`normalize`+`custom`(16) 都不崩，`sisf-sprite` 51 个也不崩
/// ⇒ 触发的是"这个模块的堆足迹"，不是用例数、也不是输出量（`--format terse` 照样崩）。
/// 这是 node 24 on Windows 的事，不是引擎的；分成两腿只是让这一腿在本地与 CI 都能判定，
/// **不要**因为"反正是 harness 崩"就把退出码当噪音吞掉。
const APP_LEG_SPLIT: &str = "frame / timing（与 normalize 分腿，见 APP_LEG_SPLIT 注释）";

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
        "web" => web(rest),
        "golden" => golden(rest),
        "wasm-test" => wasm_test(rest),
        "android" => android(rest),
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
    let web_features = web_features();
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
            "wasm32 音频全开 + 移植层(ogg/mp3/wav/midi,rpgmk,video)",
            vec![
                "check",
                "-p",
                "sisf",
                "--all-targets",
                "--target",
                WASM_TARGET,
                "--no-default-features",
                "--features",
                &web_features,
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
    let web_features = web_features();
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
    // T11 补的两条腿。为什么第一条 `--workspace --all-targets` 不够：带
    // `required-features` 的 target 在 feature 不满足时是**静默跳过**，于是
    // `rpgmv_shell`（要 `image,ogg,video,rpgmk`）连同它的单测、以及 `sisf-rpgmk` 的
    // `archive` 模块（要 `archive`）在闸门里隐身 —— 实测：`crates/sisf/src/video.rs`
    // 的一个 `doc_lazy_continuation` 就藏在那儿绿过去了。这两条用的是 `check_all`
    // 第 1 / 第 4 组同样的 feature 口径（宿主 `--all-features`、wasm `web_features()`），
    // 只是把 check 换成 clippy；与测试闸门 `cargo test --workspace --all-features` 对齐。
    let code = check(&[
        "clippy",
        "--workspace",
        "--all-targets",
        "--all-features",
        "--",
        "-D",
        "warnings",
    ]);
    if code != 0 {
        return code;
    }
    let code = check(&[
        "clippy",
        "-p",
        "sisf",
        "--all-targets",
        "--target",
        WASM_TARGET,
        "--no-default-features",
        "--features",
        &web_features,
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

/// 收集子进程输出并原样回显，同时从 `test result:` 行取出通过数。
///
/// 回显是必要的而不是便利：`cargo test` 的失败详情只存在于 stdout，一旦 `output()`
/// 把管道吃掉又不在失败路径打印，用户就只剩一个退出码。
fn capture(cmd: &mut Command) -> (i32, usize) {
    let out = match cmd.output() {
        Ok(o) => o,
        Err(e) => {
            eprintln!("无法启动 cargo: {e}");
            return (1, 0);
        }
    };
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    print!("{stdout}");
    print!("{stderr}");
    let passed = stdout
        .lines()
        .filter_map(|l| l.strip_prefix("test result: ok. "))
        .filter_map(|l| l.split_whitespace().next())
        .filter_map(|n| n.parse::<usize>().ok())
        .sum();
    (out.status.code().unwrap_or(1), passed)
}

/// `cargo xtask wasm-test`：把纯逻辑套件真的跑在 wasm32 上。
///
/// `rest` 透传给 harness（`cargo test … -- <rest>`）；一旦带了透传就**不再**检查
/// "0 个用例"—— 这时 0 通过多半只是过滤没命中，而不是别名漏了。
///
/// 每腿单独起进程：一腿崩了能一眼是谁，而且 `sisf-app` 需要拆腿（[`APP_LEG_SPLIT`]）。
fn wasm_test(rest: &[String]) -> i32 {
    let mut total = 0usize;
    for (package, extra, harness, what) in WASM_TESTS {
        let mut args: Vec<String> = vec![
            "test".to_string(),
            "-p".to_string(),
            (*package).to_string(),
            "--target".to_string(),
            WASM_TARGET.to_string(),
        ];
        args.extend(extra.iter().map(|s| (*s).to_string()));
        println!("---- wasm-test: {package}（{what}）----");
        let mut cmd = Command::new("cargo");
        cmd.args(&args);
        cmd.arg("--");
        cmd.args(harness.iter().copied());
        cmd.args(rest);
        let (code, passed) = capture(&mut cmd);
        if code != 0 {
            eprintln!("wasm-test 在 {package} 失败（exit {code}）");
            return code;
        }
        if passed == 0 && rest.is_empty() {
            eprintln!(
                "wasm-test 在 {package} 通过 0 个用例 —— 这不是绿。\n\
                 wasm 的 runner 只执行 `#[wasm_bindgen_test]`，libtest 的 `#[test]` 会编译成\n\
                 存在但没人跑的函数。请在那个 `mod tests` 里补上（见 WASM_TESTS 的注释）：\n\
                 \x20   #[cfg(target_arch = \"wasm32\")] use wasm_bindgen_test::wasm_bindgen_test as test;"
            );
            return 7;
        }
        println!("（{package}：{passed} 个用例在 wasm32 上通过）");
        total += passed;
    }
    println!("wasm-test 合计：{total} 个用例通过（driver: wasm-bindgen-test-runner，默认 node）");
    0
}

/// `cargo xtask android`：工具链体检；`android build <游戏 crate>` 真跑 cargo-ndk。
///
/// 这里**不生成 APK**，也**不接受引擎自己的 crate**：引擎侧没有 `cdylib` 目标，
/// `android_main` 由游戏 crate 提供（`docs/android.md` 第 2 节）。示例（`--example`）在
/// android 上编出来是 ELF 可执行而不是 `.so`，`NativeActivity` 的 `lib_name` 根本加载不了，
/// 所以把示例名当默认值会留下一条"看着能跑、其实装不进 APK"的命令。
fn android(rest: &[String]) -> i32 {
    if let Some(first) = rest.first() {
        return match first.as_str() {
            "build" => match rest.get(1) {
                Some(package) => android_build(package),
                None => {
                    eprintln!(
                        "用法: cargo xtask android build <游戏 crate>\n\
                         要填的是**你自己的** crate（`crate-type = [\"cdylib\"]` + `android_main`，\n\
                         模板见 docs/android.md 第 2 节），不是 `sisf` 也不是示例名。"
                    );
                    2
                }
            },
            other => {
                eprintln!("未知子命令 `{other}`；用法: cargo xtask android [build <游戏 crate>]");
                2
            }
        };
    }
    android_doctor()
}

fn probe(program: &str, args: &[&str]) -> String {
    match Command::new(program).args(args).output() {
        Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).trim().to_string(),
        Ok(o) => String::from_utf8_lossy(&o.stderr).trim().to_string(),
        Err(_) => "（不在 PATH）".to_string(),
    }
}

fn android_doctor() -> i32 {
    println!("---- xtask android：工具链体检 ----");
    let ndk_home = env::var("ANDROID_NDK_HOME")
        .or_else(|_| env::var("NDK_HOME"))
        .unwrap_or_default();
    if ndk_home.is_empty() {
        println!("[ ] ANDROID_NDK_HOME / NDK_HOME 未设置");
    } else {
        println!("[x] ANDROID_NDK_HOME = {ndk_home}");
        if !Path::new(&ndk_home).join("toolchains").exists() {
            println!("    ⚠ toolchains/ 不存在，这个路径大概不是 NDK 根目录");
        }
    }
    println!("[.] cargo-ndk : {}", probe("cargo", &["ndk", "--version"]));
    println!("    （不在 PATH 就 `cargo install cargo-ndk --locked`，见 docs/android.md 第 1 节）");
    println!("[.] adb       : {}", probe("adb", &["version"]));
    println!("[.] rustup 目标 aarch64-linux-android：");
    let targets = probe("rustup", &["target", "list", "--installed"]);
    let has = targets.lines().any(|l| l.trim() == ANDROID_TARGET);
    println!(
        "    [{}] {}",
        if has { "x" } else { " " },
        if has {
            ANDROID_TARGET.to_string()
        } else {
            format!("缺 —— rustup target add {ANDROID_TARGET} armv7-linux-androideabi")
        }
    );
    println!("[.] 连接的设备：");
    let devices = probe("adb", &["devices"]);
    let listed: Vec<&str> = devices
        .lines()
        .skip(1)
        .filter(|l| l.split_whitespace().last() == Some("device"))
        .collect();
    if listed.is_empty() {
        println!("    （无设备。真机手测清单见 docs/android.md 第 6 节）");
    } else {
        for d in listed {
            println!("    {d}");
        }
    }
    println!(
        "\n下一步：cargo xtask android build <你的游戏 crate>（.so 落 jniLibs，清单与打包见 docs/android.md）"
    );
    0
}

fn android_build(package: &str) -> i32 {
    // 先判"这个包根本编不出 APK"，再判环境：请求本身不合法时，报 NDK 缺失是误导，
    // 而且这条判据没有 NDK 的机器上就验不了。
    if package.starts_with("sisf") {
        eprintln!(
            "`{package}` 是引擎自己的 crate，它是 rlib —— 链不进 APK。\n\
             要编的是带 `crate-type = [\"cdylib\"]` 和 `android_main` 的游戏 crate（docs/android.md 第 2 节）。"
        );
        return 9;
    }
    if env::var("ANDROID_NDK_HOME")
        .or_else(|_| env::var("NDK_HOME"))
        .unwrap_or_default()
        .is_empty()
    {
        eprintln!(
            "ANDROID_NDK_HOME / NDK_HOME 都没设置，cargo-ndk 找不到工具链。\n\
             安装与导出步骤见 docs/android.md 第 1 节；先跑 `cargo xtask android` 看体检结果。"
        );
        return 8;
    }
    // -o 直接指到脚手架约定的 jniLibs；引擎不碰 APK，所以这里到此为止。
    check(&[
        "ndk",
        "-t",
        "arm64-v8a",
        "-o",
        "android/app/src/main/jniLibs",
        "build",
        "--release",
        "-p",
        package,
    ])
}

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .to_path_buf()
}

/// `cargo xtask web [示例名] [--no-serve]`：构建 wasm 示例 + wasm-bindgen 到 `web/pkg`，
/// 再在 127.0.0.1:8080 起静态服务。
///
/// `--no-serve` 是给 CI 的：`serve_dir` 是个 `incoming()` 死循环，闸门里跑 `xtask web`
/// 会永远挂着；而 CI 要复用**同一条**构建配方（否则 CI 的产物与本地手测的产物可能不是
/// 一个 feature 集，那正是"CI 绿、浏览器里没声音"的形状）。
fn web(rest: &[String]) -> i32 {
    let mut serve = true;
    let mut example: Option<&str> = None;
    for arg in rest {
        match arg.as_str() {
            "--no-serve" => serve = false,
            other if other.starts_with('-') => {
                eprintln!("未知参数 `{other}`；用法: cargo xtask web [示例名] [--no-serve]");
                return 2;
            }
            other => example = Some(other),
        }
    }
    let example = example.unwrap_or("hello_window");
    let features = web_features();
    // release：debug 的 `tiled_map_bg.wasm` 是 32.5MB、release 8.2MB，而软件 WebGL 下每毫秒都值钱
    // —— 浏览器侧量到的"节拍噪声"有一半就是这个没优化的产物。诊断通道不受档位影响：
    // panic 钩子在 `platform/wasm.rs` 里无条件安装。
    let code = check(&[
        "build",
        "--release",
        "-p",
        "sisf",
        "--example",
        example,
        "--target",
        WASM_TARGET,
        "--no-default-features",
        "--features",
        &features,
    ]);
    if code != 0 {
        return code;
    }
    let root = workspace_root();
    let wasm = root
        .join("target")
        .join(WASM_TARGET)
        .join("release")
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
    if !serve {
        println!("已按 --no-serve 跳过静态服务，产物在 {}", out_dir.display());
        return 0;
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
