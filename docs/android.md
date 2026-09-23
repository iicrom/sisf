# Android 构建与安装（T2 起适用）

引擎的 Android 路径已接入 `winit 0.30` + `android-activity 0.6`，`cargo xtask check-all`
会检查 `aarch64-linux-android`。真机构建需要 NDK 与 `cargo-ndk`，属于外部工具链，
本文件记录完整步骤；真机手测清单见文末（正式执行排在 T12）。

## 1. 工具链

```sh
rustup target add aarch64-linux-android armv7-linux-androideabi
# Android Studio 或独立 NDK（r26+），并导出：
export ANDROID_NDK_HOME="$HOME/Android/Sdk/ndk/<version>"   # Windows: %LOCALAPPDATA%\Android\Sdk\ndk\<version>
cargo install cargo-ndk --locked
```

`xtask android` 目前只打印本文件的检查清单（T12 实装自动打包）。

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
cargo ndk -t arm64-v8a -o android/app/src/main/jniLibs build --release -p sisf --example hello_window
adb install -r android/app/build/outputs/apk/debug/app-debug.apk
adb logcat -s rust_stdout rust_stderr    # 观察 println!/eprintln!
```

首次上机建议用任何支持 Rust 的 APK 脚手架（`cargo generate` 的 `cargo-apk`/`android-gradle` 模板均可）；
引擎侧不生成 APK，只提供 `cdylib`。

## 5. surface 生命周期约定（已实现）

- `Resumed`（winit → `Runner::resumed`）：建窗口、建 `Surface` 与 `Renderer`。
- `Suspended`（`Runner::suspended` 或可见性失效）：上报 `Lifecycle::Suspended`，
  **释放 `Renderer`（含 `Surface`）与 GPU 资源**，冻结累加器不吃挂起时间。
- 再次 `Resumed`：重新走 `platform::start_renderer_init`，按当前物理尺寸重配交换链。
  Android 必须重配，否则 `Surface::configure` 会持有失效的 ANativeWindow。

## 6. 真机手测清单（T12 执行并回填结果）

1. 冷启动出窗口并清成 `AppConfig::clear` 色。
2. 触摸事件坐标经 `FrameCtx::map_pointer` 落到虚拟分辨率内。
3. 电源键息屏 → 亮屏恢复：不 panic、画面恢复、`Lifecycle` 顺序为 `Suspended → Resumed`。
4. 切后台 ≥30s 回前台：同上，且音频不炸（T7 后加验）。
5. 旋屏（`configChanges` 已声明）：只重算投影，不重建 Activity。
