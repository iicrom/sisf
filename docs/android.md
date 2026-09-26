# Android 构建与安装（T2 起适用）

引擎的 Android 路径已接入 `winit 0.30` + `android-activity 0.6`，`cargo xtask check-all`
会检查 `aarch64-linux-android`。真机构建需要 NDK 与 `cargo-ndk`，属于外部工具链，
本文件记录完整步骤；真机手测清单见文末。

## 1. 工具链

```sh
rustup target add aarch64-linux-android armv7-linux-androideabi
# Android Studio 或独立 NDK（r26+），并导出：
export ANDROID_NDK_HOME="$HOME/Android/Sdk/ndk/<version>"   # Windows: %LOCALAPPDATA%\Android\Sdk\ndk\<version>
cargo install cargo-ndk --locked
```

装完先跑体检，四件事一次看全（NDK 环境变量、`cargo-ndk`、`adb`、rustup 目标、已连设备）：

```sh
cargo xtask android
```

体检只读环境、不动产物，退出码恒 0；**构建**那条才会硬失败：
包名是引擎自己的 crate ⇒ exit 9（rlib 链不进 APK，见第 2 节），
`ANDROID_NDK_HOME`/`NDK_HOME` 缺失 ⇒ exit 8，都不静默。
判据顺序是"先包名后环境"——所以 exit 9 这条在没有 NDK 的机器上也验得了，不会被环境缺失抢先遮住。

## 2. cdylib 入口模板

游戏 crate 需要 `crate-type = ["cdylib"]`，并提供 `android_main`：

```rust
#[cfg(target_os = "android")]
#[no_mangle]
fn android_main(android_app: android_activity::AndroidApp) {
    use sisf::app::platform;
    if let Err(e) = platform::android::run_with_android_app(MyApp::new(), AppConfig::default(), android_app) {
        eprintln!("android_main 失败: {e}");
    }
}
```

`android-activity` 由 `sisf-app` 的 `native` feature 在 android target 上自动启用
（`Cargo.toml` 的 target 段，features `native-activity`）。

## 3. 清单（AndroidManifest.xml）

`native-activity` 模式要求：

```xml
<manifest xmlns:android="http://schemas.android.com/apk/res/android">
  <application android:nativeDebuggable="true">
    <activity android:name="android.app.NativeActivity"
              android:exported="true"
              android:configChanges="orientation|keyboardHidden|screenSize">
      <meta-data android:name="android.app.lib_name" android:value="my_game"/>  <!-- 去掉 lib 前缀 -->
      <intent-filter>
        <action android:name="android.intent.action.MAIN"/>
        <category android:name="android.intent.category.LAUNCHER"/>
      </intent-filter>
    </activity>
  </application>
  <uses-permission android:name="android.permission.WAKE_LOCK"/>
</manifest>
```

## 4. 构建与安装

```sh
cargo xtask android build my_game        # .so 落 android/app/src/main/jniLibs/<abi>/
adb install -r android/app/build/outputs/apk/debug/app-debug.apk
adb logcat -s rust_stdout rust_stderr    # 观察 println!/eprintln!
```

`xtask android build` 跑的就是
`cargo ndk -t arm64-v8a -o android/app/src/main/jniLibs build --release -p <crate>`，
只多做了两件事：没 NDK 时明确报错，以及挡住"填了引擎 crate"这种装不进 APK 的产物。

**填的是游戏 crate，不是示例名**：`cargo build --example` 在 android 上出来是 ELF 可执行文件，
不是 `lib<name>.so`，`NativeActivity` 的 `android.app.lib_name` 加载不了它。首次上机建议用任何
支持 Rust 的 APK 脚手架（`cargo generate` 的 `cargo-apk`/`android-gradle` 模板均可）；
引擎侧不生成 APK，只提供 `cdylib`。

## 5. surface 生命周期约定（已实现）

- `Resumed`（winit → `Runner::resumed`）：建窗口、建 `Surface` 与 `Renderer`。
- `Suspended`（`Runner::suspended` 或可见性失效）：上报 `Lifecycle::Suspended`，
  **释放 `Renderer`（含 `Surface`）与 GPU 资源**，冻结累加器不吃挂起时间。
- 再次 `Resumed`：重新走 `platform::start_renderer_init`，按当前物理尺寸重配交换链。
  Android 必须重配，否则 `Surface::configure` 会持有失效的 ANativeWindow。

## 6. 真机手测清单（T12 交付清单本身，执行待有设备时回填）

⚠️ **未执行**。T12 这轮的开发机上 `cargo xtask android` 量到的是：`ANDROID_NDK_HOME` 未设置、
`cargo-ndk` 未安装、`adb 1.0.41` 在 PATH、`aarch64-linux-android` 目标已装、**连接设备 0 台**。
下面五条要的是真手指，任何"清单已勾"的写法都是编的。回填时每条记：机型 + Android 版本 +
`adb logcat` 里对应的关键行。

1. 冷启动出窗口并清成 `AppConfig::clear` 色。
2. 触摸事件坐标经 `FrameCtx::map_pointer` 落到虚拟分辨率内。
3. 电源键息屏 → 亮屏恢复：不 panic、画面恢复、`Lifecycle` 顺序为 `Suspended → Resumed`。
4. 切后台 ≥30s 回前台：同上，且音频不炸（T7 后加验）。
5. 旋屏（`configChanges` 已声明）：只重算投影，不重建 Activity。
