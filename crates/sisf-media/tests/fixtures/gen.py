# 生成 T4 图像解码测试 fixture（确定性像素图案，与 image.rs 测试中的 pattern() 一致）。
# 用法（在仓库任意处）：python crates/sisf-media/tests/fixtures/gen.py
# 仅依赖 ffmpeg（png/webp-lossless/jpg/gif 转码）；BMP 直接手写。
import os, struct, subprocess, sys

HERE = os.path.dirname(os.path.abspath(__file__))

def clamp(v): return max(0, min(255, v))

def pattern(x, y, w, h, phase=0):
    """确定性 RGBA 图案。alpha 故意 <255 变体以覆盖预乘路径。"""
    r = clamp(x * 255 // max(w - 1, 1))
    g = clamp(y * 255 // max(h - 1, 1))
    b = (x * 37 + y * 91 + phase * 7) & 0xFF
    a = 255 - ((x + 3 * y + phase) & 0x3F) * 2
    return (r, g, b, a)

def raw_rgba(w, h, phase=0):
    return b"".join(struct.pack("4B", *pattern(x, y, w, h, phase))
                    for y in range(h) for x in range(w))

def ffmpeg_raw_to(path, w, h, args, pix_fmt="rgba", phase=0):
    data = raw_rgba(w, h, phase) if pix_fmt == "rgba" else b"".join(
        struct.pack("3B", *pattern(x, y, w, h, phase)[:3])
        for y in range(h) for x in range(w))
    subprocess.run(["ffmpeg", "-y", "-loglevel", "error"] + args + [path],
                   input=data, check=True)

def main():
    os.chdir(HERE)
    # 64x48 RGBA PNG（无损：测试逐像素断言 = 预乘后的 pattern）
    ffmpeg_raw_to("img_64x48.png", 64, 48,
                  ["-f", "rawvideo", "-pix_fmt", "rgba", "-s", "64x48", "-i", "pipe:0",
                   "-c:v", "png"])
    # 40x40 无损 WebP
    ffmpeg_raw_to("img_40x40.webp", 40, 40,
                  ["-f", "rawvideo", "-pix_fmt", "rgba", "-s", "40x40", "-i", "pipe:0",
                   "-c:v", "libwebp", "-lossless", "1"])
    # 80x60 JPEG（有损：测试只断言尺寸 + 平均色）
    ffmpeg_raw_to("img_80x60.jpg", 80, 60,
                  ["-f", "rawvideo", "-pix_fmt", "rgb24", "-s", "80x60", "-i", "pipe:0",
                   "-c:v", "mjpeg", "-q:v", "4"], pix_fmt="rgb24")
    # 24bpp bottom-up BMP
    write_bmp("img_24x18_24bpp.bmp", 24, 18, alpha=False)
    # 32bpp BMP（带 alpha 位掩码 BI_BITFIELDS）
    write_bmp("img_16x16_32bpp.bmp", 16, 16, alpha=True)
    # 2 帧 GIF（多帧→AnimDef；只断言帧数/尺寸）
    frames = raw_rgba(24, 24, 0) + raw_rgba(24, 24, 1)
    subprocess.run(["ffmpeg", "-y", "-loglevel", "error",
                    "-f", "rawvideo", "-pix_fmt", "rgba", "-framerate", "5",
                    "-s", "24x24", "-i", "pipe:0", "-loop", "0", "img_24x24.gif"],
                   input=frames, check=True)
    print("fixtures written:", sorted(os.listdir(".")))

def write_bmp(path, w, h, alpha):
    bpp = 32 if alpha else 24
    row_bytes = ((w * bpp // 8 + 3) // 4) * 4
    pix_size = row_bytes * h
    header = 14
    dib = 40 + (16 if alpha else 0)  # BI_BITFIELDS 时掩码紧跟 DIB 头
    data_off = header + dib
    f = open(path, "wb")
    f.write(b"BM" + struct.pack("<IHHI", data_off + pix_size, 0, 0, data_off))
    f.write(struct.pack("<IiiHHIIiiII", dib, w, h, 1, bpp,
                        3 if alpha else 0, pix_size, 2835, 2835,
                        0 if not alpha else 0, 0))
    if alpha:
        f.write(struct.pack("<IIII", 0x00FF0000, 0x0000FF00, 0x000000FF, 0xFF000000))
    for y in range(h - 1, -1, -1):  # bottom-up
        row = bytearray()
        for x in range(w):
            r, g, b, a = pattern(x, y, w, h)
            if alpha:
                row += struct.pack("<BBBB", b, g, r, a)
            else:
                row += struct.pack("<BBB", b, g, r)
        row += bytes(row_bytes - len(row))
        f.write(bytes(row))
    f.close()

if __name__ == "__main__":
    sys.exit(main())
