# 生成 T7 音频解码测试 fixture（确定性正弦波，与 audio.rs 测试里的 tone() 一致）。
# 用法（在仓库任意处）：python crates/sisf-media/tests/fixtures/gen_audio.py
# WAV/BMP 式手写头（完全确定性，不经 ffmpeg）；OGG/MP3 由 ffmpeg 编码同一段正弦。
# MID 直接手写 SMF 字节（不引 MIDI 库）。
import os, struct, subprocess, sys

HERE = os.path.dirname(os.path.abspath(__file__))
SR_WAV = 22050   # WAV：低采样率立体声，顺带覆盖混音器的变速重采样
SR_LOSSY = 44100  # OGG/MP3：44.1k，MP3 走 MPEG-1 帧（1152 样本）而非 MPEG-2
SEC = 1.0
SEC_OGG = 2.0
LOOP_START = int(SR_LOSSY * 0.2)   # 0.2s → 8820 样本
LOOP_END = int(SR_LOSSY * 0.8)     # 0.8s → 35280 样本


def tone(n_frames, rate, sec):
    """交织立体声 s16le：左 = 440Hz 半幅，右 = 330Hz 四分之一幅（左右可辨，用于通道断言）。"""
    import math
    out = bytearray()
    for n in range(n_frames):
        t = n / rate
        l = int(32767 * 0.5 * math.sin(2 * math.pi * 440.0 * t))
        r = int(32767 * 0.25 * math.sin(2 * math.pi * 330.0 * t))
        out += struct.pack("<hh", max(-32768, min(32767, l)), max(-32768, min(32767, r)))
    return bytes(out)


def wav(path, rate, sec):
    data = tone(int(rate * sec), rate, sec)
    chans, bits = 2, 16
    byte_rate = rate * chans * bits // 8
    hdr = b"RIFF" + struct.pack("<I", 36 + len(data)) + b"WAVEfmt " + struct.pack(
        "<IHHIIHH", 16, 1, chans, rate, byte_rate, chans * bits // 8, bits
    ) + b"data" + struct.pack("<I", len(data))
    with open(path, "wb") as f:
        f.write(hdr + data)


def encode(path, rate, data, args):
    subprocess.run(["ffmpeg", "-y", "-loglevel", "error",
                    "-f", "s16le", "-ar", str(rate), "-ac", "2", "-i", "pipe:0"]
                   + args + [path], input=data, check=True)


def varlen(v):
    """SMF 的 delta time 是 1..4 字节的 **大端变长数**（每字节 7 位有效、高位续接）。
    早先用 struct.pack('<I', delta) 写成了定长小端，midly/rustysynth 都会解析成乱码事件。"""
    out = bytearray([v & 0x7F])
    v >>= 7
    while v:
        out.insert(0, (v & 0x7F) | 0x80)
        v >>= 7
    return bytes(out)


def midi(path):
    """格式 0、单轨、120bpm、tpqn=480：C4 E4 G4 C5 各一拍，最后一个音符落在 1.875s。

    第二拍开头插一条 CC#111（RPG Maker 的循环起点标记），用来验 `MidiLoop::RpgMaker`
    真按标记回绕（循环段是 E4 G4 C5 三音，而不是整段从头再来）。
    """
    us = 60_000_000 // 120
    events = [
        (0, bytes([0xFF, 0x51, 3]) + struct.pack(">I", us)[1:]),  # tempo：每拍 500000µs
        (0, [0xC0, 0]),                                    # 程序 0 = Acoustic Grand Piano
        (0, [0x90, 60, 100]),                              # C4 on  (tick 0)
        (360, [0x80, 60, 0]),                              # C4 off (tick 360)
        (120, [0xB0, 111, 0]),                             # CC#111 循环起点 (tick 480)
        (0, [0x90, 64, 100]),                              # E4 on
        (360, [0x80, 64, 0]),
        (120, [0x90, 67, 100]),                            # G4 on
        (360, [0x80, 67, 0]),
        (120, [0x90, 72, 100]),                            # C5 on
        (360, [0x80, 72, 0]),
        (0, [0xFF, 0x2F, 0]),                              # end of track
    ]
    track = b"".join(varlen(d) + bytes(b) for d, b in events)
    header = b"MThd" + struct.pack(">IHHH", 6, 0, 1, 480)
    body = b"MTrk" + struct.pack(">I", len(track)) + track
    with open(path, "wb") as f:
        f.write(header + body)


def main():
    os.chdir(HERE)
    # 只重生成点名的产物（`python gen_audio.py jingle.mid`）。ffmpeg 重编码会改动
    # tone_*.mp3/ogg 的字节，而音频测试是按这些 fixture 校准的，所以改 MIDI 时不该重编码。
    wanted = set(sys.argv[1:])
    jobs = {
        "tone_1s.wav": lambda: wav("tone_1s.wav", SR_WAV, SEC),
        "tone_1s.mp3": lambda: encode("tone_1s.mp3", SR_LOSSY,
                                      tone(int(SR_LOSSY * SEC), SR_LOSSY, SEC),
                                      ["-c:a", "libmp3lame", "-b:a", "128k"]),
        "tone_2s.ogg": lambda: encode("tone_2s.ogg", SR_LOSSY,
                                      tone(int(SR_LOSSY * SEC_OGG), SR_LOSSY, SEC_OGG),
                                      ["-c:a", "libvorbis", "-q:a", "0",
                                       "-metadata", f"LOOPSTART={LOOP_START}",
                                       "-metadata", f"LOOPEND={LOOP_END}"]),
        "jingle.mid": lambda: midi("jingle.mid"),
    }
    for name, job in jobs.items():
        if not wanted or name in wanted:
            job()
    unknown = wanted - set(jobs)
    if unknown:
        raise SystemExit(f"未知产物: {sorted(unknown)}，可选 {list(jobs)}")
    for name in jobs:
        print(f"{name}: {os.path.getsize(name)} 字节")
    print(f"OGG 循环标记: LOOPSTART={LOOP_START} LOOPEND={LOOP_END} @ {SR_LOSSY}Hz")


if __name__ == "__main__":
    main()
