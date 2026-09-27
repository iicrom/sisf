#!/usr/bin/env python3
"""生成 `examples/assets/mv/` —— 一份**自造的**最小 RPG Maker MV 风格工程。

为什么自己造而不拷一份真实工程：MV 的 RTP 素材有授权问题，而 T11 要证的只是
"散文件的 MV 工程能装进这台引擎"，尺寸/配色/图块号全部自定，反而能把每一条
通路（通行位、事件页条件、降级计数、图块事件）钉在可核对的取值上。

跑法（在仓库任意处）：

    python examples/assets/mv/gen.py

只依赖标准库（`zlib` + `binascii`，PNG 手写），音频从 `crates/sisf-media/tests/fixtures/`
复制。生成之后整树复制到 `web/assets/mv/`（wasm 那份是副本，改一处要改两处）：

    cp -r examples/assets/mv web/assets/mv

## 美术约定（本自造工程独有，**不是** MV 的 A1–E 编号）

- `graphics/tilesets/Shell.png`：8 列 × 4 行、每格 48px（384×192）。图块号 `id` 落在
  `cell = (id - 1) % 32`，即 `col = cell % 8`、`row = cell // 8`。MV 真实图块集是
  A1–A5 + B–E 七张图按 256 号一段编号，本引擎不做那套编号（见 `docs/porting.md`），
  所以图块号到子矩形的映射由宿主决定 —— 这里定的就是上面那一条。
- `graphics/characters/Actor*.png`：144×192 = 3 列（步帧）× 4 行（朝向）单角色，
  每格 48px。行序恒为 `下、左、右、上`；**站立帧在中间那一列**（MV 的惯例），
  所以宿主挂 `DirPattern::RpgMaker4x3` 时必须按列序 `[1, 0, 2]` 重排帧表。
  `characterIndex`（MV 一张图塞四个角色）不建模：一个文件一个角色。
"""

import json
import os
import shutil
import struct
import zlib
from binascii import crc32

HERE = os.path.dirname(os.path.abspath(__file__))
REPO = os.path.abspath(os.path.join(HERE, "..", "..", ".."))
FIXTURES = os.path.join(REPO, "crates", "sisf-media", "tests", "fixtures")

TILE_PX = 48
MAP_W, MAP_H = 24, 18

# 图块号：与 `TILES` 表一一对应（`cell = (id-1) % 32`）。
GRASS, PATH, WATER, TREE, ROCK, WALL, BRIDGE, FLOWER, STELE, LIT, SAND = (
    1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11,
)
NAMES = {
    GRASS: "grass",
    PATH: "path",
    WATER: "water",
    TREE: "tree",
    ROCK: "rock",
    WALL: "wall",
    BRIDGE: "bridge",
    FLOWER: "flower",
    STELE: "stele",
    LIT: "lit-stele",
    SAND: "sand",
}
# 四向都不通（MV 的 flags 位是"阻挡"：bit0 下 / bit1 左 / bit2 右 / bit3 上）。
BLOCK = 0x0F
# 花是"无表态"装饰（0x10 = NO_EFFECT）：压在路面上时由路面说了算。
NO_EFFECT = 0x10
OPAQUE = {WATER: BLOCK, TREE: BLOCK, ROCK: BLOCK, WALL: BLOCK, STELE: BLOCK, LIT: BLOCK}
IGNORE = {FLOWER: NO_EFFECT}


# --------------------------------------------------------------------- PNG


def chunk(tag, data):
    return (
        struct.pack(">I", len(data))
        + tag
        + data
        + struct.pack(">I", crc32(tag + data) & 0xFFFFFFFF)
    )


def write_png(path, w, h, rgba):
    """写一张 8bit RGBA（color type 6）PNG，每行 filter 0。"""
    assert len(rgba) == w * h * 4
    os.makedirs(os.path.dirname(path), exist_ok=True)
    rows = b"".join(
        b"\x00" + rgba[y * w * 4 : (y + 1) * w * 4] for y in range(h)
    )
    png = (
        b"\x89PNG\r\n\x1a\n"
        + chunk(b"IHDR", struct.pack(">IIBBBBB", w, h, 8, 6, 0, 0, 0))
        + chunk(b"IDAT", zlib.compress(rows, 9))
        + chunk(b"IEND", b"")
    )
    with open(path, "wb") as f:
        f.write(png)
    return len(png)


def px(buf, w, x, y, rgba):
    i = (y * w + x) * 4
    buf[i : i + 4] = bytes(rgba)


# ----------------------------------------------------------------- 图块美术


def tile_color(tile_id, x, y):
    """格内像素 (x, y) 的 RGBA。48px 一格的确定性图案，没有随机数。"""
    name = NAMES.get(tile_id, "filler")
    if name == "filler":
        # 空格子刷成斜纹洋红：手测时一眼能看出"这块美术根本没被用到"。
        c = (200, 24, 152) if (x // 6 + y // 6) % 2 == 0 else (120, 12, 96)
        return (*c, 255)
    if name == "grass":
        speck = (x * 7 + y * 13) % 29 == 0
        return (*((86, 146, 74) if not speck else (110, 176, 92)), 255)
    if name == "path":
        grit = (x * 11 + y * 5) % 23 == 0
        base = (146, 118, 84) if not grit else (168, 138, 98)
        return (*base, 255)
    if name == "water":
        wave = (y % 12) < 2
        return (*((52, 96, 168) if not wave else (92, 140, 200)), 255)
    if name == "bridge":
        plank = (x % 12) < 2
        edge = y < 4 or y >= TILE_PX - 4
        return (*((96, 68, 44) if edge else ((120, 86, 56) if plank else (184, 140, 92))), 255)
    if name == "sand":
        dot = (x * 3 + y * 7) % 17 == 0
        return (*((214, 190, 138) if not dot else (232, 210, 160)), 255)
    if name == "tree":
        trunk = y >= 32 and 20 <= x <= 27
        canopy = (x - 24) ** 2 + (y - 17) ** 2 <= 17 ** 2
        if trunk:
            return (92, 62, 40, 255)
        if canopy:
            dark = (x + y) % 9 == 0
            return (*((34, 84, 44) if not dark else (52, 112, 58)), 255)
        return (74, 126, 66, 255)
    if name == "rock":
        lit = (x - 14) ** 2 + (y - 14) ** 2 <= 13 ** 2
        return (*((150, 152, 164) if lit else (104, 106, 118)), 255)
    if name == "wall":
        brick = (y % 16) < 2 or ((x + (16 if (y // 16) % 2 else 0)) % 24) < 2
        return (*((84, 62, 58) if brick else (148, 108, 96)), 255)
    if name == "flower":
        # 四瓣 + 芯，其余全透明：这一层压在路面上，透明度必须真的有用。
        head = ((x - 16) ** 2 + (y - 18) ** 2 <= 16) or (
            (x - 31) ** 2 + (y - 18) ** 2 <= 16
        ) or ((x - 24) ** 2 + (y - 11) ** 2 <= 16) or (
            (x - 24) ** 2 + (y - 26) ** 2 <= 16
        )
        core = (x - 24) ** 2 + (y - 18) ** 2 <= 9
        stem = 30 <= y < 44 and 23 <= x <= 25
        if core:
            return (250, 226, 96, 255)
        if head:
            return (224, 96, 158, 255)
        if stem:
            return (58, 122, 58, 255)
        return (0, 0, 0, 0)
    if name in ("stele", "lit-stele"):
        lit = name == "lit-stele"
        base = (58, 96, 62)  # 底下透出草地
        slab = 8 <= x <= 39 and 6 <= y
        edge = slab and (8 <= x <= 9 or 38 <= x <= 39 or y >= 44)
        glyph = slab and (16 <= x <= 31) and (18 <= y <= 34) and (x + y) % 5 < 2
        if glyph:
            return (*((96, 240, 224) if lit else (96, 100, 116)), 255)
        if edge:
            return (*((120, 150, 160) if lit else (96, 98, 108)), 255)
        if slab:
            return (*((176, 232, 226) if lit else (146, 150, 162)), 255)
        return (*base, 255)
    raise AssertionError(f"未知图块 {tile_id}")


def build_tileset():
    cols, rows = 8, 4
    w, h = cols * TILE_PX, rows * TILE_PX
    buf = bytearray(w * h * 4)
    for cell in range(cols * rows):
        tile_id = cell + 1
        col, row = cell % cols, cell // cols
        for y in range(TILE_PX):
            for x in range(TILE_PX):
                px(buf, w, col * TILE_PX + x, row * TILE_PX + y, tile_color(tile_id, x, y))
    return w, h, buf


# --------------------------------------------------------------- 角色美术

# 行的视觉差异：眼睛/身体偏移，让"朝哪边走"在 48px 的小人上看得出来。
ROW_EYE = {0: (0, 4), 1: (-3, 2), 2: (3, 2), 3: (0, -3)}
ROBES = {
    "Actor1": {"robe": (58, 86, 168), "trim": (206, 190, 120), "skin": (236, 200, 168)},
    "Actor2": {"robe": (76, 172, 96), "trim": (150, 226, 150), "skin": (214, 244, 190)},
}


def char_color(sheet, col, row, x, y):
    p = ROBES[sheet]
    body_top = 14 if sheet == "Actor1" else 20
    # 影子：脚底一条半透明黑，验证图集里的 alpha 通路一路走到着色器。
    if 44 <= y and 12 <= x <= 35:
        return (0, 0, 0, 96)
    if sheet == "Actor2":
        # 史莱姆：一个半球 + 两只眼睛。
        if (x - 24) ** 2 + (y - 34) ** 2 <= 14 ** 2 + 40:
            dx = 0 if col == 1 else (-2 if col == 0 else 2)
            if (x - 24 + dx) ** 2 + (y - 30) ** 2 <= 30:
                return (*p["trim"], 255)
            return (*p["robe"], 255)
        return (0, 0, 0, 0)
    # 长老：斗篷（梯形）+ 头 + 胡子，脚随步帧左右分开。
    dx = 0 if col == 1 else (-2 if col == 0 else 2)
    head = 6 <= y < 18 and 16 <= x <= 31
    cape = y >= body_top and abs(x - (24 + dx // 2)) <= 6 + (y - body_top) * 0.55
    foot = y >= 40 and abs(x - (24 + dx * 2)) <= 4
    if head:
        ox, oy = ROW_EYE[row]
        eye = abs(x - (20 + ox)) <= 1 and abs(y - (12 + oy)) <= 1
        if eye:
            return (18, 18, 28, 255)
        return (*p["skin"], 255)
    if foot:
        return (58, 44, 34, 255)
    if cape:
        if y >= 36 or y == body_top:
            return (*p["trim"], 255)
        return (*p["robe"], 255)
    return (0, 0, 0, 0)


def build_character(sheet):
    cols, rows = 3, 4
    w, h = cols * TILE_PX, rows * TILE_PX
    buf = bytearray(w * h * 4)
    for row in range(rows):
        for col in range(cols):
            for y in range(TILE_PX):
                for x in range(TILE_PX):
                    px(buf, w, col * TILE_PX + x, row * TILE_PX + y,
                       char_color(sheet, col, row, x, y))
    return w, h, buf


# ------------------------------------------------------------------- 地图


def is_pond(x, y):
    return 15 <= x <= 19 and 6 <= y <= 11


def road(x, y):
    """两条路都不碰最外圈的树墙：碰上了那条格的 z0 是"树"，而路面在 z1 会先表态 = 可通行，
    看起来就像树墙上开了个门。"""
    return (y == 8 and 2 <= x <= 21) or (x == 7 and 3 <= y <= 15)


def base_tile(x, y):
    if x == 0 or y == 0 or x == MAP_W - 1 or y == MAP_H - 1:
        return TREE
    if is_pond(x, y):
        return WATER
    return GRASS


LAYER1 = {
    (10, 4): ROCK,
    (13, 12): ROCK,
    (5, 14): ROCK,
    (18, 15): ROCK,
}
WALL_RUN = [(x, 3) for x in range(10, 15) if x != 12]
FLOWERS = [(4, 7), (5, 7), (9, 9), (10, 9), (3, 12), (20, 4), (8, 10)]


def layer1(x, y):
    if (x, y) in LAYER1:
        return LAYER1[(x, y)]
    if (x, y) in WALL_RUN:
        return WALL
    if is_pond(x, y):
        return BRIDGE if road(x, y) else 0
    if road(x, y):
        return PATH
    if 2 <= x <= 4 and 12 <= y <= 14:
        return SAND
    return 0


def layer2(x, y):
    return FLOWER if (x, y) in FLOWERS else 0


def region(x, y):
    return 3 if (x, y) in [(16, 8), (17, 8)] else 0


def build_map_data():
    """MV 的 `data` 是 6 个整层：`[(z*H+Y)*W+X]`。本引擎只读 z=0..2（+z=5 区域号）。"""
    layers = [
        [base_tile(x, y) for x in range(MAP_W) for y in range(MAP_H)],
        [layer1(x, y) for x in range(MAP_W) for y in range(MAP_H)],
        [layer2(x, y) for x in range(MAP_W) for y in range(MAP_H)],
        [0] * (MAP_W * MAP_H),
        [0] * (MAP_W * MAP_H),
        [region(x, y) for x in range(MAP_W) for y in range(MAP_H)],
    ]
    out = []
    for z in range(6):
        for y in range(MAP_H):
            for x in range(MAP_W):
                out.append(layers[z][y * MAP_W + x])
    assert len(out) == 6 * MAP_W * MAP_H
    return out


# ------------------------------------------------------------------ 事件


def cmd(code, indent=0, params=None):
    return {"code": code, "indent": indent, "parameters": params if params is not None else []}


def audio_ref(name, volume=90):
    return {"name": name, "volume": volume, "pitch": 100, "pan": 0}


def say(lines, indent=0):
    """`101` 文字 + `401` 续行（MV 的说话人参数留空：本引擎没有脸部图通路）。"""
    out = [cmd(101, indent, ["", 0, 0, 2])]
    out += [cmd(401, indent, [line]) for line in lines]
    return out


def condition(**kw):
    base = dict(
        actorId=1, actorValid=False, itemId=1, itemValid=False,
        selfSwitchCh="A", selfSwitchValid=False,
        switch1Id=1, switch1Valid=False, switch2Id=1, switch2Valid=False,
        variableId=1, variableValid=False, variableValue=0,
    )
    base.update(kw)
    return base


def image(tile_id=0, character="", direction=2, index=0):
    return {
        "tileId": tile_id,
        "characterName": character,
        "direction": direction,
        "pattern": 0,
        "characterIndex": index,
    }


def page(cond, img, lst, **kw):
    base = dict(
        conditions=cond, directionFix=False, image=img, list=lst,
        moveFrequency=3, moveRoute={"list": [], "repeat": True, "skippable": False, "wait": False},
        moveSpeed=3, moveType=0, priorityType=1, stepAnime=False, through=False,
        trigger=0, walkAnime=True,
    )
    base.update(kw)
    return base


def elder():
    """事件 1「长老」：`101/401` + `102` 选项 + `402/403` 分支 + `121/122/123` +
    `250` SE + `251`（降级计数）+ `230` 等待 + `201` 转移（→ `Custom`，宿主不接 ⇒ 事件机说话）。
    取消那一支写独立开关 A ⇒ 换到"你又回来了"那一页。

    **页序**照 MV 的编辑器惯例：条件更严的那一页放前面（`refresh` 取第一个成立的页，
    空条件那一页永远成立，放前面就把后面的页全盖住了）。"""
    talk = say(["桥的那头有什么？", "我年轻时走到过尽头。"])
    talk.append(cmd(102, 0, [["去看一看", "再听听", "走开"], 0, 0, 2]))
    talk.append(cmd(402, 0, [0]))
    talk += [
        cmd(121, 1, [3, 3, 0]),
        cmd(250, 1, [audio_ref("Confirm01")]),
    ] + say(["石碑亮了起来，走过去看看。"], 1)
    talk.append(cmd(402, 0, [1]))
    talk += [cmd(122, 1, [1, 1, 1, 0, 1]), cmd(111, 1, [1, 1, 0, 2, 0])]
    talk += say(["他讲了三遍同一个故事。"], 2)
    talk += [cmd(411, 1, []), cmd(230, 2, [30]), cmd(251, 2, [])]
    talk.append(cmd(402, 0, [2]))
    talk += say(["那就走开吧。"], 1)
    talk.append(cmd(403, 0, []))
    talk += [cmd(123, 1, ["A", 0])]
    return {
        "id": 1, "name": "长老", "x": 6, "y": 8, "note": "",
        "pages": [
            page(
                condition(selfSwitchCh="A", selfSwitchValid=True),
                image(character="Actor1", direction=8),
                say(["你又回来了。"]) + [cmd(250, 0, [audio_ref("Confirm01")])],
            ),
            page(condition(), image(character="Actor1", direction=4), talk),
        ],
    }


def stele():
    """事件 2「石碑」：图块事件（`tileId`）+ 开关 3 决定亮/暗两页；亮页尾一条 `261` 影片
    ⇒ 宿主的安全跳过通路。两页都挡路（图块 flags = 四向不通）。"""
    dark = say(["一块没有字的石碑。", "桥到那里就断了。"])
    lit = say(["碑上的字浮出来了：", "此路不通。"]) + [cmd(261, 0, ["ending"])]
    return {
        "id": 2, "name": "石碑", "x": 16, "y": 8, "note": "",
        "pages": [
            page(condition(switch1Id=3, switch1Valid=True), image(tile_id=LIT), lit),
            page(condition(), image(tile_id=STELE), dark),
        ],
    }


def bell():
    """事件 3「远处的钟声」：自动执行（trigger 3）的一页写开关 4 ON，另一页是它后面的
    空页 ⇒ 开关一开，`refresh` 取到的是那一页，这个事件就从图上消失（同时也不再重跑）。
    列表本身走 `221` 淡出 → 文字 → `250` SE → `121` → `222` 淡入。"""
    lst = [cmd(221, 0, [])] + say(["（远处传来一声钟响。）"]) + [
        cmd(250, 0, [audio_ref("Confirm01", 70)]),
        cmd(121, 0, [4, 4, 0]),
        cmd(222, 0, []),
    ]
    return {
        "id": 3, "name": "钟声", "x": 0, "y": 0, "note": "",
        "pages": [
            # 第 0 页：开关 4 已经 ON ⇒ 什么都没有（这一页在 = "这个事件消失了"）。
            page(condition(switch1Id=4, switch1Valid=True), image(), [], priorityType=0),
            page(condition(), image(), lst, priorityType=0, trigger=3),
        ],
    }


def slime():
    """事件 4「史莱姆」：玩家接触（trigger 1）+ 随机自主移动（moveType 1）。
    结尾 `249` ME + `214` 擦除（本图内不再出现）。
    `moveFrequency` 取 3 而不是更花哨的值：宿主侧精灵是**逐像素追赶自己的格子**，
    一拍一跳（阈值 30*(5-freq) ≈ 60 拍）的话像素追得上，freq=8 会让它永远在跳。"""
    lst = say(["咕叽！"]) + [cmd(249, 0, [audio_ref("Battle01")]), cmd(214, 0, [])]
    return {
        "id": 4, "name": "史莱姆", "x": 4, "y": 12, "note": "",
        "pages": [
            page(
                condition(),
                image(character="Actor2", direction=2),
                lst,
                trigger=1, moveType=1, moveFrequency=3, moveSpeed=4,
            )
        ],
    }


def build_map():
    return {
        "displayName": "Shell",
        "width": MAP_W,
        "height": MAP_H,
        "tilesetId": 1,
        "events": [None, elder(), stele(), bell(), slime()],
        "data": build_map_data(),
        "parallaxName": "",
        "scrollType": 0,
        "disableDashing": False,
        # 进图自动播 BGM：`$dataMap.bgm` 走 `MvAudio::sound("bgm")` ⇒ `audio/bgm/Field01.ogg`。
        "autoplayBgm": True,
        "bgm": audio_ref("Field01", 90),
        "autoplayBgs": False,
        "bgs": audio_ref("", 0),
    }


def build_tilesets():
    flags = [0] * 8192
    for tid in OPAQUE:
        flags[tid] = OPAQUE[tid]
    for tid, bit in IGNORE.items():
        flags[tid] = bit
    # 未用到的 12..32 号保持 0（全通行）：越界的图块号也照这个语义（`flag()` 返回 0）。
    one = {
        "id": 1,
        "mode": 1,
        "note": "",
        "tilesetNames": ["", "", "", "", "", "Shell", "", "", ""],
        "flags": flags,
    }
    # MV 的 `Tilesets.json` 是顶层数组，下标 0 是 `null` 洞。
    return [None, one]


def build_system():
    switches = ["", "", "", "石碑点亮", "钟声听过"] + [""] * 58
    variables = ["", "听过次数", "备用"] + [""] * 57
    return {
        "gameTitle": "SISF rpgmv_shell",
        "gameoverMe": audio_ref("Battle01", 90),
        "locale": "en_US",
        "terminal": 1,
        "titleBgm": audio_ref("Field01", 90),
        "versionId": 1,
        "audioDirectory": "audio",
        "dataPath": "data/",
        "images": {},
        "switches": switches,
        "variables": variables,
        "titleId": 1,
        "gameoverId": 1,
        "editCount": 12,
        # 出生点归 `System.json`（`Scene_Boot.startGamePlayer`）：路上第 2 格、朝右，
        # 往右走 4 格就撞见长老 —— 验收要的那条链从这一步开始。
        "startMapId": 1,
        "startX": 2,
        "startY": 8,
        "startDirection": 6,
    }


def dump(rel, obj):
    path = os.path.join(HERE, rel)
    os.makedirs(os.path.dirname(path), exist_ok=True)
    with open(path, "w", encoding="utf-8", newline="\n") as f:
        json.dump(obj, f, ensure_ascii=False, separators=(",", ":"))
    return os.path.getsize(path)


def main():
    sizes = []
    sizes.append(("data/Map001.json", dump("data/Map001.json", build_map())))
    sizes.append(("data/Tilesets.json", dump("data/Tilesets.json", build_tilesets())))
    sizes.append(("data/System.json", dump("data/System.json", build_system())))

    for rel, (w, h, buf) in [
        ("graphics/tilesets/Shell.png", build_tileset()),
        ("graphics/characters/Actor1.png", build_character("Actor1")),
        ("graphics/characters/Actor2.png", build_character("Actor2")),
    ]:
        n = write_png(os.path.join(HERE, rel), w, h, buf)
        sizes.append((f"{rel} ({w}x{h})", n))

    # 音频：真实工程里是 .ogg，本仓库的解码链也吃 .ogg —— 直接复制那份 fixture。
    src = os.path.join(FIXTURES, "tone_2s.ogg")
    for rel in ["audio/bgm/Field01.ogg", "audio/se/Confirm01.ogg", "audio/me/Battle01.ogg"]:
        dst = os.path.join(HERE, rel)
        os.makedirs(os.path.dirname(dst), exist_ok=True)
        shutil.copyfile(src, dst)
        sizes.append((rel, os.path.getsize(dst)))

    for rel, n in sizes:
        print(f"{rel:36s} {n:>9,d} 字节")


if __name__ == "__main__":
    main()
