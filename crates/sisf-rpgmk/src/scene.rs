//! 最小场景层：`Scene_Map` + `Game_Character`（T11）。
//!
//! 对位的是 MV 的 `Game_Map` / `Game_CharacterBase` / `Game_Event` 三件里**这一层真需要**
//! 的部分：谁能站到哪一格、事件页什么时候换、自动事件往哪儿走。它不管绘制（宿主拿
//! [`Scene_Map::chars`] 去摆 sprite）、不管解释器（宿主拿 [`Scene_Map::page`] 上的
//! `trigger` 去起 `EventRunner`）—— 那两件事引擎已经有了，在这里再造一份只会两边打架。
//!
//! ## 一拍 = MV 的一帧逻辑（60fps）
//!
//! [`Scene_Map::tick`] 的语义是 `Game_CharacterBase.update`，单位是 MV 的那个 60fps。
//! 引擎主循环的拍长由 [`crate::translate::Options`] 决定，折算归宿主。这一层不引入第二个
//! 时钟，否则"移动速度"会有两套单位。
//!
//! ## 位置是格子，不是像素
//!
//! MV 分 `_x/_y`（格子）与 `_realX/_realY`（像素，用来插值）。这里只留格子那一份：插值是
//! 精灵层的事（引擎的 `MoveRoute`/`Tween` 已经在做），场景层再存一份浮点位置就是第二个
//! 真相源。
//!
//! ## 自主移动只建模 `moveType` 1（随机）
//!
//! `Game_Event.updateSelfMovement` 的 switch 只有 1/2/3 三个case：0（固定）什么都不做；
//! 2（`moveTypeTowardPlayer`，向玩家接近，靠 `isNearThePlayer()` 那句 `sx + sy < 20` 决定
//! 要不要贴人）在没有战斗的移植验证里价值最低；3（`moveTypeCustom`）要的是路线执行器 ——
//! 而路线在引擎侧属于精灵层的 `MoveRoute`，所以
//! [`translate_route`](crate::translate::translate_route) 把路线翻成引擎的形状，执行归宿主
//! 摆 sprite。这两条在 `docs/porting.md` 里逐条标了状态，不做"看起来支持了"的空实现
//! （DoD-4）。
//!
//! ## 与 MV 的三处有意差异
//!
//! 1. 没有视口概念 ⇒ `isNearTheScreen()` 恒真（小图全在屏内；大图由宿主裁剪）。
//! 2. 玩家不参与"被撞"判定 —— 与 MV 一致（那里 `isCollidedWithCharacters` 查的是
//!    `$gameMap.events()`，事件可以走上玩家那一格，这正是"事件接触"触发器的实现方式）。
//! 3. 一次格子移动在一拍内完成（见上"位置是格子"）⇒ `_stopCount` 每拍 +1、走成功清零，
//!    节奏与 `checkStop(30 * (5 - freq))` 一致。

use crate::game_vars::GameVars;
use crate::map::{EventPage, MapDef, TilesetDef};

/// 玩家在 [`Scene_Map::chars`] 里的编号（MV 用 `$gamePlayer` 单例，事件从 1 起，
/// `events[0]` 那个 `null` 洞正好对上）。
pub const PLAYER: u32 = 0;

/// RPG Maker 方向：2 下 / 4 左 / 6 右 / 8 上。`reverseDir(d) = 10 - d`（MV 原式）。
pub const fn reverse_dir(d: u8) -> u8 {
    10 - d
}

/// 四个朝向（`moveRandom` 的骰子面）。
pub const DIRS: [u8; 4] = [2, 4, 6, 8];

/// 地图上的一个角色：玩家，或某个事件的当前页。
///
/// 名字带下划线是故意的：移植者手里拿的是 MV 的 `Game_Character`，改名会让
/// "这格对应上游哪个对象"这件事失去索引（`Scene_Map` 同理）。
#[allow(non_camel_case_types)]
#[derive(Clone, Debug, PartialEq)]
pub struct Game_Character {
    /// [`PLAYER`] 或 MV 的事件号（= `MapDef::events` 的下标）。
    pub id: u32,
    pub x: u32,
    pub y: u32,
    /// 2/4/6/8。
    pub dir: u8,
    /// 当前生效的事件页下标；`None` = 这一帧这个角色不存在（MV 的 `_pageIndex = -1`）。
    /// 玩家没有"页"，恒为 `None` 而照旧可见 —— 判可见请用 [`Self::visible`]。
    pub page: Option<usize>,
    /// `Game_Interpreter.command214`（Erase Event）之后：本图内不再出现，`refresh` 也救不
    /// 回来（MV 把 `_erased` 写进存档，所以"读档才复活"）。
    erased: bool,
    /// `_stopCount`：站着没动的帧数。
    stop_count: u32,
}

impl Game_Character {
    fn new(id: u32, x: u32, y: u32) -> Self {
        Self {
            id,
            x,
            y,
            dir: 2,
            page: None,
            erased: false,
            stop_count: 0,
        }
    }

    pub fn is_player(&self) -> bool {
        self.id == PLAYER
    }

    /// 玩家恒可见；事件要"有当前页"（页条件全不成立 / 被擦除 ⇒ 这一帧不存在）。
    pub fn visible(&self) -> bool {
        self.is_player() || (self.page.is_some() && !self.erased)
    }

    pub fn pos(&self) -> (u32, u32) {
        (self.x, self.y)
    }

    pub fn erased(&self) -> bool {
        self.erased
    }

    /// `_stopCount > 30 * (5 - freq)`（`Game_Event.stopCountThreshold` + `checkStop`）。
    /// `freq` 6..8 时阈值是负数 ⇒ 每拍都想动（MV 的"最高速"就是这个行为）。
    fn gate(move_frequency: u8) -> i64 {
        30 * (5 - i64::from(move_frequency))
    }
}

/// 一轮 [`Scene_Map::refresh`] 的账单（测试与调试面板要看"到底换了几页"）。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Refresh {
    /// 当前页下标发生变化的角色数（含出现与消失）。
    pub changed: usize,
    /// 刷完之后看不见的事件数（页条件全不成立 / 被擦除）。
    pub hidden: usize,
    /// 判为"不成立"的**页**数，其条件是这一层没有通路的那两类（物品 / 队伍，见
    /// `PageCondition::has_unsupported`）。按页计，所以同一轮里重复刷新会重复计数。
    pub unsupported: usize,
}

/// 一拍的账单。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Tick {
    /// 这一拍开头有没有因为 `requestRefresh` 重算事件页。
    pub refreshed: Option<Refresh>,
    /// 成功走了一格的角色数。
    pub moved: usize,
}

/// 一张图上的全部角色。
#[allow(non_camel_case_types)]
#[derive(Clone, Debug)]
pub struct Scene_Map {
    map_id: u32,
    map: MapDef,
    tileset: TilesetDef,
    chars: Vec<Game_Character>,
}

impl Scene_Map {
    /// 进图。`start` 是玩家落点（图号得显式给：`data/Map003.json` 的那个 3 在文件名里，
    /// `MapDef` 自己没有 `id` 字段，而自我开关的键要用它）。
    ///
    /// 建完角色表就叫一次 [`Self::refresh`]：MV 的 `Game_Map.setup` 最后一步也是刷新，
    /// 不刷的话"进图就该出现的事件"要等到第一次开关写入才现身。朝向也在那里定 ——
    /// 新建事件的初始朝向是**当前页**的 `image.direction`，不是第一页的。
    pub fn enter(
        map_id: u32,
        map: MapDef,
        tileset: TilesetDef,
        start: (u32, u32, u8),
        vars: &GameVars,
    ) -> Self {
        let mut player = Game_Character::new(PLAYER, start.0, start.1);
        if DIRS.contains(&start.2) {
            player.dir = start.2;
        }
        let mut s = Self {
            map_id,
            map,
            tileset,
            chars: vec![player],
        };
        for id in s.map.event_ids() {
            let e = s.map.event(id).expect("event_ids 只报非空的洞");
            s.chars.push(Game_Character::new(id, e.x, e.y));
        }
        s.refresh(vars);
        s
    }

    pub fn map(&self) -> &MapDef {
        &self.map
    }

    pub fn map_id(&self) -> u32 {
        self.map_id
    }

    pub fn tileset(&self) -> &TilesetDef {
        &self.tileset
    }

    /// 下标 0 是玩家，其余是事件（按事件号升序）。
    pub fn chars(&self) -> &[Game_Character] {
        &self.chars
    }

    pub fn player(&self) -> &Game_Character {
        &self.chars[0]
    }

    /// 按 MV 的事件号取角色（不是按下标）。
    pub fn get(&self, id: u32) -> Option<&Game_Character> {
        self.chars.iter().find(|c| c.id == id)
    }

    /// 某个角色当前页的原始数据（宿主取图像、触发器、`move_speed` 都在这一条上）。
    pub fn page(&self, id: u32) -> Option<&EventPage> {
        let c = self.get(id)?;
        let page = c.page?;
        self.map.event(id)?.pages.get(page)
    }

    /// 站在 `(x, y)` 上、**当前页是图块**的事件图块号（`Game_Map.tileEventsXy`）。
    /// 顺序按事件号升序，与 MV 的 `events()` 一致。
    pub fn event_tiles(&self, x: u32, y: u32) -> Vec<u32> {
        self.chars
            .iter()
            .filter(|c| !c.is_player() && c.visible() && c.pos() == (x, y))
            .filter_map(|c| self.page(c.id).and_then(EventPage::tile_id))
            .collect()
    }

    /// 重算所有事件页：取**第一个**条件成立的页（`Game_Event.refresh` 从下标 0 往后找，
    /// 找到就 `setupPage` 并 `break`）；一页都不成立 ⇒ 消失。
    ///
    /// 顺序要紧：把"开门后"那一页放在第一页，开关一开它就盖住原来的样子。
    pub fn refresh(&mut self, vars: &GameVars) -> Refresh {
        let mut r = Refresh::default();
        for c in self.chars.iter_mut().skip(1) {
            let Some(ev) = self.map.event(c.id) else {
                continue;
            };
            let mut next = None;
            if !c.erased {
                for (i, p) in ev.pages.iter().enumerate() {
                    if p.conditions.has_unsupported() {
                        r.unsupported += 1;
                    }
                    if p.conditions.meets(vars, self.map_id, c.id) {
                        next = Some(i);
                        break;
                    }
                }
            }
            if next == c.page {
                if !c.visible() {
                    r.hidden += 1;
                }
                continue;
            }
            r.changed += 1;
            c.page = next;
            c.stop_count = 0;
            // `setupPageSettings`：换页连朝向一起换（`direction` 0 = "保持上次朝向"）。
            if let Some(d) = next
                .and_then(|i| ev.pages.get(i))
                .map(|p| p.image.direction)
                .filter(|d| DIRS.contains(d))
            {
                c.dir = d;
            }
            if !c.visible() {
                r.hidden += 1;
            }
        }
        r
    }

    /// 一拍 MV 逻辑：先看 `requestRefresh` 要不要重算页，再让可见事件按 `moveType` 自己动。
    ///
    /// 玩家这一步什么都不做（他要等宿主的动作）。
    pub fn tick(&mut self, vars: &mut GameVars) -> Tick {
        let refreshed = if vars.take_refresh() {
            Some(self.refresh(vars))
        } else {
            None
        };
        let mut out = Tick {
            refreshed,
            moved: 0,
        };

        // 两段式：第一段只问"想往哪走"（读页数据要 `&self`、掷骰要 `&mut Rng`），
        // 第二段才真的挪（要 `&mut self`）。合成一段会在借用检查器上撞车。
        let mut attempts: Vec<(u32, u8)> = Vec::new();
        {
            // `rng_mut` 而不是 `store_mut`：抽骰子不是"状态变了要说一句话"，用它取旗
            // 会让每一拍都重算事件页（`setupPage` 定的朝向被反复刷掉）。
            let rng = vars.rng_mut();
            for i in 1..self.chars.len() {
                let (id, freq, dir, x, y) = {
                    let c = &self.chars[i];
                    let Some(page) = self.page(c.id) else {
                        continue;
                    };
                    if !c.visible() || page.move_type != 1 {
                        continue;
                    }
                    (c.id, page.move_frequency, c.dir, c.x, c.y)
                };
                self.chars[i].stop_count = self.chars[i].stop_count.saturating_add(1);
                if i64::from(self.chars[i].stop_count) <= Game_Character::gate(freq) {
                    continue;
                }
                // `moveTypeRandom`：0/1 随机方向、2..4 沿当前朝向、5 重置停止计数。
                let d = match rng.range(0, 5) {
                    0 | 1 => DIRS[rng.range(0, 3) as usize],
                    2..=4 => dir,
                    _ => {
                        self.chars[i].stop_count = 0;
                        continue;
                    }
                };
                if self.can_pass(id, x, y, d) {
                    attempts.push((id, d));
                }
            }
        }
        for (id, dir) in attempts {
            if self.move_straight(id, dir) {
                out.moved += 1;
            }
        }
        out
    }

    /// `Game_CharacterBase.moveStraight`：**失败也要转向**（对着墙按住方向要能换朝向），
    /// 但那一页写了 `directionFix` 就不许转。走通了才 `increaseSteps`（清 `_stopCount`）。
    pub fn move_straight(&mut self, id: u32, dir: u8) -> bool {
        let Some(i) = self.chars.iter().position(|c| c.id == id) else {
            return false;
        };
        let (x, y) = (self.chars[i].x, self.chars[i].y);
        let ok = self.can_pass(id, x, y, dir);
        let fixed = self.page(id).is_some_and(|p| p.direction_fix);
        if !fixed && DIRS.contains(&dir) {
            self.chars[i].dir = dir;
        }
        if ok {
            let (nx, ny) = self.map.step(x, y, dir);
            self.chars[i].x = nx;
            self.chars[i].y = ny;
            self.chars[i].stop_count = 0;
        }
        ok
    }

    /// 某个角色朝向前方一格的落点。已经在闭合边界外 ⇒ `None`（MV 的 `isValid` 先否）。
    /// 宿主的"决定键触发"就是拿这个格子去查 [`Self::events_at`]。
    pub fn front(&self, id: u32) -> Option<(u32, u32)> {
        let c = self.get(id)?;
        self.step_valid(c.x, c.y, c.dir)
    }

    /// 站在该格上的可见事件（`Game_Map.eventsXy`：不分朝向、不看优先级）。宿主用它判
    /// "玩家接触 / 事件接触"触发器。
    pub fn events_at(&self, x: u32, y: u32) -> Vec<u32> {
        self.chars
            .iter()
            .filter(|c| !c.is_player() && c.visible() && c.pos() == (x, y))
            .map(|c| c.id)
            .collect()
    }

    /// `Game_Interpreter.command214`：擦除事件。本图内永久，读档才回来。
    pub fn erase(&mut self, id: u32) -> bool {
        let Some(c) = self.chars.iter_mut().find(|c| c.id == id) else {
            return false;
        };
        c.erased = true;
        c.page = None;
        true
    }

    /// `Game_CharacterBase.canPass`：`isValid` → `through` → 两边都点头的图块判定 → 撞角色。
    ///
    /// `through`（穿墙）在页上，而 MV 是在 `isValid` **之后**才放行 —— 闭合边界上穿墙的
    /// 事件也出不去图外，这一条顺序照抄。
    pub fn can_pass(&self, id: u32, x: u32, y: u32, dir: u8) -> bool {
        if self.get(id).is_none() {
            return false;
        }
        let Some((nx, ny)) = self.step_valid(x, y, dir) else {
            return false;
        };
        if self.page(id).is_some_and(|p| p.through) {
            return true;
        }
        if !self.map_passable(x, y, dir, nx, ny) {
            return false;
        }
        !self.collided(nx, ny, id)
    }

    /// 落点。`roundX/roundY` 会夹到边界内，所以"能不能走"要先看**未取整**的坐标在不在图内
    /// （MV 的 `isValid`）—— 闭合边界上这一步就是 `None`。
    fn step_valid(&self, x: u32, y: u32, dir: u8) -> Option<(u32, u32)> {
        let (dx, dy): (i64, i64) = match dir {
            2 => (0, 1),
            4 => (-1, 0),
            6 => (1, 0),
            8 => (0, -1),
            _ => return None,
        };
        let (nx, ny) = (i64::from(x) + dx, i64::from(y) + dy);
        if nx < 0 || ny < 0 || nx >= i64::from(self.map.width) || ny >= i64::from(self.map.height) {
            return None;
        }
        Some((self.map.round_x(nx), self.map.round_y(ny)))
    }

    /// `isMapPassable`：**出发格朝 `dir` 通** 且 **目标格朝反方向通**，两边都要点头。
    /// 各格的事件图块各算各的。
    fn map_passable(&self, x: u32, y: u32, dir: u8, nx: u32, ny: u32) -> bool {
        self.map
            .passable(&self.tileset, x, y, dir, &self.event_tiles(x, y))
            && self.map.passable(
                &self.tileset,
                nx,
                ny,
                reverse_dir(dir),
                &self.event_tiles(nx, ny),
            )
    }

    /// `isCollidedWithEvents`：目标格上有没有"可见 + 不穿墙 + `priorityType` 1（与玩家同级）"
    /// 的**其他事件**。`priorityType` 0（底下）与 2（上方）从不挡路，事件也不挡事件以外的
    /// 谁（玩家不在这张名单里）。
    fn collided(&self, x: u32, y: u32, who: u32) -> bool {
        self.chars.iter().any(|o| {
            o.id != who
                && !o.is_player()
                && o.visible()
                && o.pos() == (x, y)
                && self
                    .page(o.id)
                    .is_some_and(|p| p.priority_type == 1 && !p.through)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::SystemDef;
    use crate::map::TilesetDef;
    use sisf_event::Slot;
    // T12 约定：这一组是纯逻辑，wasm 上照样跑（`cargo xtask wasm-test`）。
    #[cfg(target_arch = "wasm32")]
    use wasm_bindgen_test::wasm_bindgen_test as test;

    /// 一张 5x4 的图。z=2 的 (4,0) 是 7 号图块（四向不通）= 右上角那堵墙。
    /// 事件 1「长老」在 (2,1)：开关 1 开 ⇒ 第 0 页（变成一块**图块**碑，挡路）；
    /// 关 ⇒ 第 1 页（角色图 + `directionFix`）。事件 2「史莱姆」`moveType` 1 +
    /// `moveFrequency` 8（阈值负数 ⇒ 每拍掷骰）。事件 3 是 `null` 洞。
    const MAP_JSON: &str = r#"{
        "width":5,"height":4,"tilesetId":1,"scrollType":0,"displayName":"Shell","events":[
            null,
            {"id":1,"name":"Elder","x":2,"y":1,"pages":[
                {"conditions":{"switch1Id":1,"switch1Valid":true},
                 "image":{"characterName":"","tileId":7,"direction":2},
                 "moveType":0,"moveSpeed":3,"moveFrequency":3,"priorityType":1,
                 "directionFix":false,"trigger":0,"list":[]},
                {"conditions":{},"image":{"characterName":"Actor1","tileId":0,"direction":8},
                 "moveType":0,"moveSpeed":3,"moveFrequency":3,"priorityType":1,
                 "directionFix":true,"trigger":0,"list":[]}
            ]},
            {"id":2,"name":"Slime","x":1,"y":2,"pages":[
                {"conditions":{},"image":{"characterName":"Slime","tileId":0,"direction":2},
                 "moveType":1,"moveSpeed":4,"moveFrequency":8,"priorityType":1,
                 "directionFix":false,"trigger":0,"list":[]}
            ]},
            null
        ],
        "data":[0,0,0,0,0, 0,0,0,0,0, 0,0,0,0,0, 0,0,0,0,0,
                0,0,0,0,0, 0,0,0,0,0, 0,0,0,0,0, 0,0,0,0,0,
                0,0,0,0,7, 0,0,0,0,0, 0,0,0,0,0, 0,0,0,0,0,
                0,0,0,0,0, 0,0,0,0,0, 0,0,0,0,0, 0,0,0,0,0,
                0,0,0,0,0, 0,0,0,0,0, 0,0,0,0,0, 0,0,0,0,0,
                0,0,0,0,0, 0,0,0,0,0, 0,0,0,0,0, 0,0,0,0,0]
    }"#;

    fn scene(vars: &GameVars) -> Scene_Map {
        let map = MapDef::from_json(MAP_JSON).expect("地图");
        // 只有 7 号图块"四向都不通"，其它一律 0 = 全通（MV 的 flags 就是这么稀疏）。
        let mut ts = TilesetDef {
            flags: vec![0; 8],
            ..TilesetDef::default()
        };
        ts.flags[7] = 0b1111;
        Scene_Map::enter(1, map, ts, (0, 0, 6), vars)
    }

    /// 验收（T11）：事件按条件出现/消失 —— 含"换页顺手改了通行"这条 MV 才有的耦合。
    #[test]
    fn events_appear_and_disappear_with_conditions() {
        let mut g = GameVars::new();
        let mut s = scene(&g);
        // 开关 1 没开 ⇒ 第一页轮不到，取第二页（角色图，不贡献图块号）。
        assert_eq!(s.get(1).unwrap().page, Some(1));
        assert!(s.event_tiles(2, 1).is_empty());
        assert_eq!(s.get(1).unwrap().dir, 8, "初始朝向来自当前页而不是第一页");
        // 开开关 ⇒ 换到第一页：一块图块碑。它进 `checkPassage`（图块层），而长老这个人
        // 本来就进 `isCollidedWithCharacters`（`priorityType` 1）—— 两道闸各自钉一次，
        // 只验 `can_pass` 会分不清是哪道闸拦的。
        g.set_switch(1, true);
        assert_eq!(s.refresh(&g).changed, 1);
        assert_eq!(s.get(1).unwrap().page, Some(0));
        assert_eq!(s.event_tiles(2, 1), vec![7]);
        assert!(
            !s.map().passable(s.tileset(), 2, 1, 2, &s.event_tiles(2, 1)),
            "碑自己不通"
        );
        assert!(!s.can_pass(PLAYER, 2, 2, 8), "碑 + 站着的人都挡");
        // 关掉 ⇒ 又是角色页：图块闸打开，人闸还在。三个状态都验才叫"按条件出现/消失"。
        g.set_switch(1, false);
        assert_eq!(s.refresh(&g).changed, 1);
        assert!(s.event_tiles(2, 1).is_empty(), "角色页不贡献图块号");
        assert!(
            s.map().passable(s.tileset(), 2, 1, 2, &[]),
            "格子在图块层面空出来了"
        );
        assert!(!s.can_pass(PLAYER, 2, 2, 8), "但长老还站在那儿挡人");
        // 擦除后 `refresh` 救不回来（`_erased` 是运行期旗标，不是页条件）。
        s.erase(1);
        assert!(!s.get(1).unwrap().visible());
        assert_eq!(s.refresh(&g).hidden, 1, "只剩史莱姆可见");
        // 自我开关走 VM 那条路（`Slot::Var(ss_key)`）时页条件也读得懂。
        let mut g2 = GameVars::new();
        g2.store_mut()
            .set_numeric(&Slot::Var(GameVars::ss_key(1, 2, "A")), 1);
        assert!(g2.self_switch(1, 2, "A"));
    }

    /// 验收（T11）：图块碰撞 —— 出发格与目标格两边都点头、边界外不通、撞同级事件不通。
    #[test]
    fn passage_needs_both_sides_and_blocks_characters() {
        let g = GameVars::new();
        let mut s = scene(&g);
        // 墙在 (4,0)：从 (3,0) 朝右，出发格自己是通的，是**目标格朝左**不通。
        assert!(!s.can_pass(PLAYER, 3, 0, 6));
        assert!(s.can_pass(PLAYER, 3, 0, 2));
        assert!(!s.can_pass(PLAYER, 0, 0, 0), "非法朝向直接否");
        // 闭合边界：走出图外 = `isValid` 先否，不是绕回（`scrollType` 0）。
        assert!(!s.can_pass(PLAYER, 0, 0, 4));
        // 撞人：走到 (1,1)，朝右是长老（`priorityType` 1）。
        assert!(s.move_straight(PLAYER, 2));
        assert!(s.move_straight(PLAYER, 6));
        assert_eq!(s.player().pos(), (1, 1));
        assert!(!s.move_straight(PLAYER, 6), "同级事件挡着");
        assert_eq!(s.player().dir, 6, "MV 的 moveStraight 失败也 setDirection");
        assert_eq!(s.player().pos(), (1, 1), "没走成 ⇒ 格子号一位都不动");
        // 擦除之后同一格就能进了（挡路的从来不是图块，是那个人）。
        s.erase(1);
        assert!(s.move_straight(PLAYER, 6));
        assert_eq!(s.player().pos(), (2, 1));
        // `directionFix` 那一页（长老第 1 页）：走成了也不许转。
        let g2 = GameVars::new();
        let mut s2 = scene(&g2);
        assert!(s2.move_straight(1, 6));
        assert_eq!(s2.get(1).unwrap().pos(), (3, 1));
        assert_eq!(s2.get(1).unwrap().dir, 8, "directionFix 拦转向，走路照旧");
        // 接触触发器用的两个查询（朝向还被 `directionFix` 钉在 8 ⇒ 前方是 (3,0)）。
        assert_eq!(s2.front(1), Some((3, 0)));
        assert_eq!(s2.events_at(3, 1), vec![1]);
    }

    /// 验收（T11）：随机走得动、走不进墙，而且同一颗种子走出同一条路。
    #[test]
    fn random_walk_moves_avoids_walls_and_reproduces() {
        let run = |seed: u64| {
            let mut g = GameVars::seeded(seed);
            let mut s = scene(&g);
            let mut path = vec![s.get(2).unwrap().pos()];
            for _ in 0..120 {
                s.tick(&mut g);
                path.push(s.get(2).unwrap().pos());
            }
            path
        };
        let a = run(11);
        let b = run(11);
        assert_eq!(a, b, "同一颗种子必须走出同一条路（RNG 是存档的一部分）");
        assert!(
            a.windows(2).any(|w| w[0] != w[1]),
            "120 拍一步没走 = 自主移动根本没接上"
        );
        for &(x, y) in &a {
            assert!(x < 5 && y < 4, "走出了图外");
            assert!(!(x == 4 && y == 0), "走进了墙");
        }
        // 走一格 = `_stopCount` 清零；阈值是负数（`freq` 8）⇒ 每拍都掷骰。
        let mut g = GameVars::seeded(3);
        let mut s = scene(&g);
        let t = s.tick(&mut g);
        assert!(t.moved <= 1, "只有史莱姆会自己走");
        assert!(
            t.moved == 1 || s.get(2).unwrap().stop_count >= 1,
            "没走成时停止计数要往上走"
        );
        assert_eq!(s.player().pos(), (0, 0), "玩家不参与自主移动");
        // 静止型（`moveType` 0）站定 —— 200 拍之后原位。
        let mut g2 = GameVars::new();
        let mut s2 = scene(&g2);
        for _ in 0..200 {
            s2.tick(&mut g2);
        }
        assert_eq!(s2.get(1).map(Game_Character::pos), Some((2, 1)));
    }

    /// `tick` 只在被要求的那一拍重算页（每拍重算会把 `setupPage` 定下的朝向反复刷掉）。
    #[test]
    fn tick_refreshes_only_when_asked() {
        let mut g = GameVars::new();
        let mut s = scene(&g);
        assert!(s.tick(&mut g).refreshed.is_none());
        g.set_switch(1, true);
        let t = s.tick(&mut g);
        assert_eq!(t.refreshed.map(|r| r.changed), Some(1));
        assert_eq!(s.get(1).unwrap().page, Some(0));
        assert!(s.tick(&mut g).refreshed.is_none(), "旗已经取走");
        // 手动 refresh 不消费旗，也不该重复记账。
        assert_eq!(s.refresh(&g).changed, 0);
        // 名字表一绑定就按 MV 的 `id < length` 拦写：这张表只有 1 号开关。
        let sys = SystemDef {
            switches: vec![String::new(), "开门".into()],
            ..SystemDef::default()
        };
        let mut bound = GameVars::with_system(&sys);
        assert!(bound.set_switch(1, true));
        assert!(!bound.set_switch(2, true), "len 是 2 ⇒ 2 号出界");
    }
}
