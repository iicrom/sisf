//! 双端一致判据：同一份 JS 脚本 → 同一份 `Vec<Command>` 账单。
//!
//! 这些测试只走 [`JsBridge::command`]，**不碰 `EventCtx`**（`command` 本来也拿不到它），
//! 所以同一份断言能在 native（rquickjs / quickjs-ng）与 wasm（浏览器自带引擎，`cargo xtask
//! wasm-test` 走 node）两条腿上原样跑。两条腿唯一的差别是 `Engine::exec` 怎么调，prelude 与
//! 账本解析是同一份 Rust/JS ⇒ 断言相等就是在验"两颗引擎语义一致"，不是"看着一样"。

use super::*;
use crate::command::Instr;

// T12：`cargo xtask wasm-test` 在 wasm 上跑这一组（见 docs/platforms.md）。
#[cfg(target_arch = "wasm32")]
use wasm_bindgen_test::wasm_bindgen_test as test;

/// 跑一段脚本，把返回程序里的叶子指令抽出来。`None`（不认的名字）直接 panic —— 这些用例
/// 跑的都是本桥认领的主题。
fn run(bridge: &mut JsBridge, js: &str) -> Vec<Command> {
    let prog = bridge
        .command(SCRIPT_TOPIC, &[Param::Text(js.into())])
        .expect("脚本主题应被认领");
    prog.code
        .iter()
        .filter_map(|i| match i {
            Instr::Run(c) => Some(c.clone()),
            _ => None,
        })
        .collect()
}

#[test]
fn script_setvar_becomes_one_engine_command() {
    let mut b = JsBridge::new();
    let cmds = run(&mut b, "$gameVariables.setValue(5, 12);");
    assert_eq!(
        cmds,
        vec![Command::SetVar {
            slot: Slot::Int(5),
            op: SetOp::Set,
            value: Expr::Lit(12),
        }],
        "355 里一发 setValue 应翻成恰好一条 SetVar"
    );
}

#[test]
fn overlay_read_sees_earlier_write_in_same_script() {
    let mut b = JsBridge::new();
    let cmds = run(
        &mut b,
        "$gameVariables.setValue(1, 7); $gameVariables.setValue(2, $gameVariables.value(1) + 1);",
    );
    assert_eq!(
        cmds,
        vec![
            Command::SetVar {
                slot: Slot::Int(1),
                op: SetOp::Set,
                value: Expr::Lit(7)
            },
            Command::SetVar {
                slot: Slot::Int(2),
                op: SetOp::Set,
                value: Expr::Lit(8)
            },
        ],
        "同段脚本里第 2 发要读到第 1 写刚设的 7 ⇒ 8"
    );
}

#[test]
fn switch_write_maps_to_switch_slot() {
    let mut b = JsBridge::new();
    let cmds = run(&mut b, "$gameSwitches.setValue(10, true);");
    assert_eq!(
        cmds,
        vec![Command::SetVar {
            slot: Slot::Switch(10),
            op: SetOp::Set,
            value: Expr::Lit(1),
        }],
        "$gameSwitches.setValue(true) ⇒ Switch 槽写 1"
    );
}

#[test]
fn audio_and_text_map_to_named_commands() {
    let mut b = JsBridge::new();
    let cmds = run(
        &mut b,
        "SISF.playBgm('Boss01', 1.0, 500); SISF.showText('长老', '你来得正好', ['留下', '离开'], 3);",
    );
    assert_eq!(
        cmds,
        vec![
            Command::PlayBgm(Sound {
                name: "Boss01".into(),
                gain: 1.0,
                fade_in: 500.0,
            }),
            Command::ShowText {
                speaker: Some("长老".into()),
                body: "你来得正好".into(),
                choices: vec!["留下".into(), "离开".into()],
                result: Some(Slot::Int(3)),
                cursor_se: None,
                allow_cancel: false,
            },
        ],
        "playBgm/showText 落成引擎自己的 PlayBgm/ShowText（ShowText 由 VM 挂起、进 VM 的分支）"
    );
}

#[test]
fn transfer_and_custom_stay_customs() {
    let mut b = JsBridge::new();
    let cmds = run(
        &mut b,
        "SISF.transferPlayer(2, 12, 8); SISF.postCustom(7, {a: 1});",
    );
    assert_eq!(
        cmds,
        vec![
            Command::Custom(
                "rpgmv.transfer".into(),
                vec![Param::Int(2), Param::Int(12), Param::Int(8)]
            ),
            Command::Custom(
                "js.custom.7".into(),
                vec![Param::from_json(&serde_json::json!({"a": 1}))]
            ),
        ],
        "转移/自定义不在引擎词汇表里 ⇒ 保持 Custom 主题交宿主认领"
    );
}

#[test]
fn unknown_topic_is_not_claimed() {
    let mut b = JsBridge::new();
    // movie/unimplemented/宿主自己的名字都不该被 js 桥认领 ⇒ None ⇒ VM 登记告警（不是空实现）。
    assert!(
        b.command("rpgmv.movie", &[Param::Text("x".into())])
            .is_none()
    );
    assert!(
        b.command("rpgmv.unimplemented", &[Param::Int(251)])
            .is_none()
    );
}

#[test]
fn plugin_topic_is_handled_even_with_nothing_to_run() {
    let mut b = JsBridge::new();
    // 356 → SISF.plugin → 一条 plugin 账本（要读 VFS 才能兑现，记进 pending）。
    // 处理完了但没有指令要跑 ⇒ Some(空程序)，而不是 None（None 只留给"不认这个名字"）。
    let prog = b
        .command(PLUGIN_TOPIC, &[Param::Text("MyPlugin".into())])
        .expect("插件主题应被认领");
    assert!(prog.is_empty(), "插件加载当场不该产出引擎指令");
}

#[test]
fn deterministic_random_is_stable_across_two_fresh_bridges() {
    let js = "$gameVariables.setValue(1, SISF.randomInt(0, 100)); $gameVariables.setValue(2, SISF.randomInt(0, 100));";
    let a = run(&mut JsBridge::new(), js);
    let b = run(&mut JsBridge::new(), js);
    assert_eq!(a, b, "同一份脚本、同一空镜像 ⇒ 两次跑出完全相同的随机读数");
    // 随机数必须落在请求区间内（两腿同判据）。
    for cmd in &a {
        if let Command::SetVar {
            value: Expr::Lit(v),
            ..
        } = cmd
        {
            assert!((0..=100).contains(v), "SISF.randomInt(0,100) 越界：{v}");
        }
    }
}

#[test]
fn nonfinite_and_oversized_values_degrade_not_panic() {
    let mut b = JsBridge::new();
    // NaN 当 0；超 u32 的变量号被 parser 拒成一条错误 ⇒ run 返回 Err ⇒ command 交空程序、不 panic。
    let cmds = run(&mut b, "$gameVariables.setValue(1, NaN);");
    assert_eq!(
        cmds,
        vec![Command::SetVar {
            slot: Slot::Int(1),
            op: SetOp::Set,
            value: Expr::Lit(0),
        }],
        "NaN 折成 0，不卡事件"
    );
    // 越界变量号：整段降级为空程序（Some），而不是 panic。
    let prog = b
        .command(
            SCRIPT_TOPIC,
            &[Param::Text(
                "$gameVariables.setValue(5000000000, 1);".into(),
            )],
        )
        .expect("主题仍被认领");
    assert!(prog.is_empty(), "越界变量号应整段降级为空程序");
}
