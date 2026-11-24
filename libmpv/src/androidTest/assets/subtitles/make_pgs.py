"""Writes pgs.sup: a minimal HDMV PGS (Blu-ray) subtitle stream, by hand, because FFmpeg
has no PGS encoder. One display set shows a 64x16 white bar from 0 s, a second one clears
it at 10 s. Segment layout as in the Blu-ray spec (and FFmpeg's pgssubdec.c): "PG", PTS
and DTS (90 kHz, u32), the segment type (u8) and size (u16), all big-endian.

Usage: python -I make_pgs.py <output.sup>
"""
import struct
import sys

PCS, WDS, PDS, ODS, END = 0x16, 0x17, 0x14, 0x15, 0x80
VIDEO_W, VIDEO_H = 720, 480
BAR_W, BAR_H = 64, 16
X, Y = 328, 400  # centred near the bottom


def segment(pts, kind, payload):
    return b"PG" + struct.pack(">IIBH", pts, 0, kind, len(payload)) + payload


def pcs(composition, state, objects):
    # video size, frame rate code 0x10, composition number and state, no palette update,
    # palette 0, then (object id, window id, not cropped, x, y) per object
    out = struct.pack(">HHBHBBBB", VIDEO_W, VIDEO_H, 0x10, composition, state, 0, 0, len(objects))
    for x, y in objects:
        out += struct.pack(">HBBHH", 0, 0, 0, x, y)
    return out


def wds():
    return struct.pack(">BBHHHH", 1, 0, X, Y, BAR_W, BAR_H)


def pds():
    # palette 0, version 0; entry 1: white (Y=235, Cr=Cb=128), opaque
    return struct.pack(">BB", 0, 0) + struct.pack(">BBBBB", 1, 235, 128, 128, 255)


def rle_line(width, color):
    # 00 11LLLLLL LLLLLLLL CC: width (64..16383) pixels of `color`; then 00 00, end of line
    return bytes([0x00, 0xC0 | (width >> 8), width & 0xFF, color, 0x00, 0x00])


def ods():
    data = b"".join(rle_line(BAR_W, 1) for _ in range(BAR_H))
    # the object data length (u24) counts the width and height that follow it
    length = len(data) + 4
    # object 0, version 0, first and last fragment (0xC0)
    return struct.pack(">HBB", 0, 0, 0xC0) + length.to_bytes(3, "big") + struct.pack(">HH", BAR_W, BAR_H) + data


def main(path):
    shown, cleared = 0, 10 * 90000
    stream = b"".join([
        segment(shown, PCS, pcs(0, 0x80, [(X, Y)])),  # epoch start, one object
        segment(shown, WDS, wds()),
        segment(shown, PDS, pds()),
        segment(shown, ODS, ods()),
        segment(shown, END, b""),
        segment(cleared, PCS, pcs(1, 0x00, [])),  # normal case, no object: clears it
        segment(cleared, WDS, wds()),
        segment(cleared, END, b""),
    ])
    with open(path, "wb") as f:
        f.write(stream)


if __name__ == "__main__":
    main(sys.argv[1])
