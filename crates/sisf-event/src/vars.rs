//! 变量存储与条件求值：开关 / 整数 / 命名文本槽，加上一台可存盘的确定性 RNG。
//!
//! 三态而不是"一个 `Vec<Value>`"是 RPG Maker 留下的形状（`$gameSwitches` /
//! `$gameVariables` 各一套），照抄的好处是 T11 的转换层不必在中间造映射。三条线各自
//! 用有序容器，于是**存档是稳定的字节**：同一份状态永远写出同一串 RON，读档后的 diff
//! 只反映真正的变化（`HashMap` 做不到这点，它的迭代序每进程都不同）。
//!
//! ## `Rand` 为什么自带 RNG 而不是 `rand` crate
//!
//! 事件里的随机数要能复现（golden 式的回归、以及"读档后同一件事不再变一次"），所以
//! 种子是状态的一部分、跟着存档走。这里用 splitmix64：几十位整数运算、无依赖、序列足够
//! 均匀（DoD-3 不想为几个骰子引一个 crate）。
//!
//! ## 求值的两处"宽容"
//!
//! - 除数为 0 ⇒ 结果 0；`Mod` 同理。配置写错不该有"事件卡住"这种后果。
//! - 文本槽参与算术时按十进制解析，解析不出就是 0。真要比文字请走
//!   [`Cond::TextEq`]（它不做数字转换）。

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::command::{Cond, Expr, Slot};

/// splitmix64：状态一个 `u64`，输出取高 32 位（够小整数区间用）。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Rng {
    state: u64,
}

impl Rng {
    pub const fn seeded(seed: u64) -> Self {
        Self { state: seed }
    }

    /// 下一个 64 位散列值。
    pub fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// 闭区间 `[lo, hi]` 上的整数；`hi <= lo` 时恒为 `lo`（区间写反是配置错，不是 panic）。
    pub fn range(&mut self, lo: i64, hi: i64) -> i64 {
        if hi <= lo {
            return lo;
        }
        let span = (hi - lo) as u64 + 1;
        // 取模偏置在 span ≪ 2^64 时可忽略；事件里的骰子不需要密码学均匀。
        lo + (self.next_u64() % span) as i64
    }
}

impl Default for Rng {
    fn default() -> Self {
        Self::seeded(0x1234_5678_9ABC_DEF0)
    }
}

/// 一个游戏的事件变量状态。
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct VarStore {
    #[serde(default)]
    switches: BTreeSet<u32>,
    #[serde(default)]
    ints: BTreeMap<u32, i64>,
    #[serde(default)]
    vars: BTreeMap<String, String>,
    #[serde(default)]
    rng: Rng,
}

impl VarStore {
    pub fn new() -> Self {
        Self::default()
    }

    /// 指定随机种子（回归测试与"同一存档同一序列"用）。
    pub fn seeded(seed: u64) -> Self {
        Self {
            rng: Rng::seeded(seed),
            ..Self::default()
        }
    }

    pub fn rng_mut(&mut self) -> &mut Rng {
        &mut self.rng
    }

    // --- 三态读写 -------------------------------------------------------------

    pub fn is_on(&self, id: u32) -> bool {
        self.switches.contains(&id)
    }

    pub fn set_switch(&mut self, id: u32, on: bool) {
        if on {
            self.switches.insert(id);
        } else {
            self.switches.remove(&id);
        }
    }

    /// 已打开的开关（存档与调试面板要看全集，而不是"最近一次"）。
    pub fn switches(&self) -> impl Iterator<Item = u32> + '_ {
        self.switches.iter().copied()
    }

    /// 取整数槽；没设过的槽是 `0`（RPG Maker 的 `$game_variables` 同义）。
    pub fn int(&self, id: u32) -> i64 {
        self.ints.get(&id).copied().unwrap_or(0)
    }

    pub fn set_int(&mut self, id: u32, value: i64) {
        self.ints.insert(id, value);
    }

    pub fn ints(&self) -> impl Iterator<Item = (u32, i64)> + '_ {
        self.ints.iter().map(|(k, v)| (*k, *v))
    }

    /// 取文本槽；没设过就是空串（画进消息窗时就是"没有说话人"）。
    pub fn text(&self, name: &str) -> &str {
        self.vars.get(name).map(String::as_str).unwrap_or("")
    }

    pub fn set_text(&mut self, name: impl Into<String>, value: impl Into<String>) {
        self.vars.insert(name.into(), value.into());
    }

    pub fn texts(&self) -> impl Iterator<Item = (&str, &str)> {
        self.vars.iter().map(|(k, v)| (k.as_str(), v.as_str()))
    }

    /// 槽的当前数值：开关 `0/1`、整数原值、文本按十进制解析（解析不出 = 0）。
    pub fn numeric(&self, slot: &Slot) -> i64 {
        match slot {
            Slot::Switch(id) => i64::from(self.is_on(*id)),
            Slot::Int(id) => self.int(*id),
            Slot::Var(name) => self.text(name).trim().parse().unwrap_or(0),
        }
    }

    /// 槽的当前文本：数值槽写成十进制（开关就是 `0`/`1`），所以 `TextEq` 对三态都成立。
    pub fn textual(&self, slot: &Slot) -> String {
        match slot {
            Slot::Var(name) => self.text(name).to_owned(),
            other => self.numeric(other).to_string(),
        }
    }

    /// 写槽。数值走 `numeric` 的读法回写，所以 `Var("hp")` 里存的永远是十进制文本。
    pub fn set_numeric(&mut self, slot: &Slot, value: i64) {
        match slot {
            Slot::Switch(id) => self.set_switch(*id, value != 0),
            Slot::Int(id) => self.set_int(*id, value),
            Slot::Var(name) => {
                let v = value.to_string();
                self.vars.insert(name.clone(), v);
            }
        }
    }

    /// `SetVar`：把 `op` 作用在槽的现值上。
    ///
    /// `Tog` 只对开关有意义，数值槽按"非 0 → 0，0 → 1"处理（这是 `Switch` 的语义外推，
    /// 不是随手写的：至少它可逆）。
    pub fn apply(&mut self, slot: &Slot, op: crate::command::SetOp, value: i64) {
        use crate::command::SetOp;
        let cur = self.numeric(slot);
        let next = match op {
            SetOp::Set => value,
            SetOp::Add => cur.wrapping_add(value),
            SetOp::Sub => cur.wrapping_sub(value),
            SetOp::Mul => cur.wrapping_mul(value),
            SetOp::Div => divide(cur, value),
            SetOp::Mod => modulo(cur, value),
            SetOp::Tog => {
                if cur == 0 {
                    1
                } else {
                    0
                }
            }
        };
        self.set_numeric(slot, next);
    }

    /// 求值。`Expr::Rand` 会推进 RNG（所以本方法要 `&mut self`），其余分支纯读。
    pub fn eval(&mut self, expr: &Expr) -> i64 {
        match expr {
            Expr::Lit(n) => *n,
            Expr::Slot(slot) => self.numeric(slot),
            Expr::Rand { lo, hi } => self.rng.range(*lo, *hi),
            Expr::Neg(a) => self.eval(a).wrapping_neg(),
            Expr::Add(a, b) => {
                let a = self.eval(a);
                let b = self.eval(b);
                a.wrapping_add(b)
            }
            Expr::Sub(a, b) => {
                let a = self.eval(a);
                let b = self.eval(b);
                a.wrapping_sub(b)
            }
            Expr::Mul(a, b) => {
                let a = self.eval(a);
                let b = self.eval(b);
                a.wrapping_mul(b)
            }
            Expr::Div(a, b) => {
                let a = self.eval(a);
                let b = self.eval(b);
                divide(a, b)
            }
            Expr::Mod(a, b) => {
                let a = self.eval(a);
                let b = self.eval(b);
                modulo(a, b)
            }
        }
    }

    /// 条件求值（`All([])` 恒真、`Any([])` 恒假 —— 与折叠单位元一致）。
    pub fn test(&mut self, cond: &Cond) -> bool {
        match cond {
            Cond::Switch { id, on } => self.is_on(*id) == *on,
            Cond::Cmp { lhs, op, rhs } => {
                let a = self.eval(lhs);
                let b = self.eval(rhs);
                op.test(a, b)
            }
            Cond::TextEq { slot, value } => &self.textual(slot) == value,
            Cond::Not(c) => !self.test(c),
            Cond::All(v) => v.iter().all(|c| self.test(c)),
            Cond::Any(v) => v.iter().any(|c| self.test(c)),
        }
    }

    /// 存档写成 RON。
    pub fn to_ron(&self) -> String {
        ron::ser::to_string_pretty(self, ron::ser::PrettyConfig::default())
            .expect("三态存储只含字符串与整数，序列化不会失败")
    }

    /// 读档（字段可缺省 ⇒ 旧存档加新字段不会读不出来）。
    pub fn from_ron(src: &str) -> Result<Self, crate::command::CommandError> {
        ron::from_str(src).map_err(|e| crate::command::CommandError::Ron(e.to_string()))
    }
}

/// 除零 ⇒ 0（见模块文档）。
fn divide(a: i64, b: i64) -> i64 {
    if b == 0 { 0 } else { a.wrapping_div(b) }
}

fn modulo(a: i64, b: i64) -> i64 {
    if b == 0 { 0 } else { a.wrapping_rem(b) }
}

#[cfg(test)]
mod tests {
    use super::*;
    // T12：`cargo xtask wasm-test` 在 wasm 上跑这一组（见 docs/platforms.md）。
    use crate::command::{CmpOp, Cond, Expr, SetOp, Slot};
    #[cfg(target_arch = "wasm32")]
    use wasm_bindgen_test::wasm_bindgen_test as test;

    fn e(i: i64) -> Box<Expr> {
        Box::new(Expr::Lit(i))
    }

    /// 算术：优先级由树决定（这里就是表达式树本身），除零不炸。
    #[test]
    fn arithmetic_is_total() {
        let mut v = VarStore::new();
        v.set_int(1, 17);
        let expr = Expr::Sub(
            Box::new(Expr::Mul(Box::new(Expr::Slot(Slot::int(1))), e(3))),
            e(1),
        );
        assert_eq!(v.eval(&expr), 50);
        assert_eq!(v.eval(&Expr::Div(e(9), e(0))), 0);
        assert_eq!(v.eval(&Expr::Mod(e(9), e(0))), 0);
        // 向零取整：负数两侧都试一遍，别悄悄变成 floor。
        assert_eq!(v.eval(&Expr::Div(e(-9), e(2))), -4);
        assert_eq!(v.eval(&Expr::Mod(e(-9), e(2))), -1);
        assert_eq!(v.eval(&Expr::Neg(Box::new(Expr::Lit(-5)))), 5);
    }

    /// 三态各自的缺省值与 `SetOp` 的全部取值。
    #[test]
    fn slots_default_and_ops_apply() {
        let mut v = VarStore::new();
        assert_eq!(v.int(9), 0);
        assert!(!v.is_on(3));
        assert_eq!(v.text("name"), "");

        v.apply(&Slot::switch(3), SetOp::Set, 5);
        assert!(v.is_on(3), "非 0 即真");
        v.apply(&Slot::switch(3), SetOp::Tog, 0);
        assert!(!v.is_on(3));

        v.apply(&Slot::int(1), SetOp::Set, 10);
        for (op, arg, want) in [
            (SetOp::Add, 5, 15),
            (SetOp::Sub, 3, 12),
            (SetOp::Mul, 2, 24),
            (SetOp::Div, 4, 6),
            (SetOp::Mod, 4, 2),
            (SetOp::Div, 0, 0),
            (SetOp::Tog, 0, 1),
        ] {
            v.apply(&Slot::int(1), op, arg);
            assert_eq!(v.int(1), want, "{op:?} {arg}");
        }

        // 文本槽：写数字就是十进制文本，参与算术时按十进制读回。
        v.apply(&Slot::var("hp"), SetOp::Set, 7);
        assert_eq!(v.text("hp"), "7");
        v.apply(&Slot::var("hp"), SetOp::Add, 3);
        assert_eq!(v.numeric(&Slot::var("hp")), 10);
        // 解析不出的文本按 0 算，但 `TextEq` 仍看得见原文。
        v.set_text("mode", "暗");
        assert_eq!(v.numeric(&Slot::var("mode")), 0);
        assert!(v.test(&Cond::TextEq {
            slot: Slot::var("mode"),
            value: "暗".into()
        }));
        // 数值槽也能被 TextEq 比（十进制表示）。
        assert!(v.test(&Cond::TextEq {
            slot: Slot::int(1),
            value: "1".into()
        }));
    }

    /// 验收：嵌套 `If`。
    #[test]
    fn nested_conditions_compose() {
        let mut v = VarStore::new();
        v.set_switch(1, true);
        v.set_int(1, 4);
        v.set_text("leader", "艾达");
        let outer = Cond::All(vec![
            Cond::Switch { id: 1, on: true },
            Cond::Any(vec![
                Cond::Cmp {
                    lhs: Expr::Slot(Slot::int(1)),
                    op: CmpOp::Gt,
                    rhs: Expr::Lit(10),
                },
                Cond::Not(Box::new(Cond::TextEq {
                    slot: Slot::var("leader"),
                    value: "鲍勃".into(),
                })),
            ]),
        ]);
        assert!(v.test(&outer));
        // 把内层的 `Any` 关掉：换掉文本槽即可让整体为假。
        v.set_text("leader", "鲍勃");
        assert!(!v.test(&outer));
        // 单位元：空 `All` 恒真、空 `Any` 恒假（`Cond::always()` 就是前者）。
        assert!(v.test(&Cond::always()));
        assert!(!v.test(&Cond::Any(vec![])));
    }

    /// `Rand` 跟着种子走：同种子同序列（存档回归的前提），且落在闭区间内。
    #[test]
    fn rng_is_deterministic_and_bounded() {
        let cond = |seed: u64| -> Vec<i64> {
            let mut v = VarStore::seeded(seed);
            (0..8)
                .map(|_| v.eval(&Expr::Rand { lo: 3, hi: 6 }))
                .collect()
        };
        assert_eq!(cond(7), cond(7));
        assert_ne!(cond(7), cond(8));
        assert!(cond(7).iter().all(|n| (3..=6).contains(n)));
        // 退化区间恒为下界（写反也是）。
        let mut v = VarStore::seeded(1);
        assert_eq!(v.eval(&Expr::Rand { lo: 5, hi: 5 }), 5);
        assert_eq!(v.eval(&Expr::Rand { lo: 9, hi: 2 }), 9);
    }

    /// 验收：存档往返一致，而且同一状态永远写出同一串字节。
    #[test]
    fn save_roundtrips_and_is_stable() {
        let mut v = VarStore::seeded(42);
        v.set_switch(2, true);
        v.set_switch(9, true);
        v.set_int(1, -7);
        v.set_text("leader", "艾达");
        let once = v.to_ron();
        assert_eq!(once, v.to_ron(), "有序容器 ⇒ 存档字节稳定");
        let back = VarStore::from_ron(&once).expect("读得回来");
        assert_eq!(back, v);
        // 读档后随机序列接着走，而不是从头再摇一遍。
        assert_eq!(back.rng.clone().next_u64(), v.rng.clone().next_u64());
        // 缺字段的旧存档读得开（`serde(default)`）。
        assert_eq!(
            VarStore::from_ron("(switches: [1],)").expect("部分字段可省"),
            {
                let mut fresh = VarStore::new();
                fresh.set_switch(1, true);
                fresh
            }
        );
    }
}
