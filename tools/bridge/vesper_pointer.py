#!/usr/bin/env python3
"""Vesper Bridge kernel-level pointer for Manor Lords (raw-input games).

Creates a uinput virtual absolute-pointer device (EV_ABS) — the same class
of device a drawing tablet is — which raw-input games consume directly.
Absolute positioning means no drift: to click real (x,y) on the 2880x1800
screen we emit ABS_X=x, ABS_Y=y, then BTN_LEFT press/release.

Usage: vesper_pointer.py move|click|drag <args>
Written for phase 2ak; requires /dev/uinput writable (wheel group here).
"""
import fcntl
import os
import struct
import sys
import time

UI_SET_EVBIT = 0x40045564
UI_SET_KEYBIT = 0x40045565
UI_SET_ABSBIT = 0x40045566
UI_DEV_SETUP = 0x405c5503
UI_ABS_SETUP = 0x401c5504
UI_DEV_CREATE = 0x5501
UI_DEV_DESTROY = 0x5502
ABS_X, ABS_Y = 0, 1
BTN_LEFT = 0x110
EV_KEY, EV_ABS, EV_SYN = 0x01, 0x03, 0x00
SCREEN_W, SCREEN_H = 2879, 1799
# struct input_event = { struct timeval time; __u16 type; __u16 code; __s32 value; }
# timeval on 64-bit = two longs (8+8). Layout: sec, usec, type, code, value.
INPUT_EVENT = struct.Struct('<qqHHi')


class Pointer:
    def __init__(self):
        self.fd = os.open('/dev/uinput', os.O_WRONLY | os.O_NONBLOCK)
        for bit in (EV_KEY, EV_ABS, EV_SYN):
            fcntl.ioctl(self.fd, UI_SET_EVBIT, bit)
        fcntl.ioctl(self.fd, UI_SET_KEYBIT, BTN_LEFT)
        fcntl.ioctl(self.fd, UI_SET_ABSBIT, ABS_X)
        fcntl.ioctl(self.fd, UI_SET_ABSBIT, ABS_Y)
        setup = (
            struct.pack('<4H', 0x06, 0x1235, 0x0002, 0x0001)
            + b'vesper-bridge-pointer'.ljust(80, b'\0')
            + struct.pack('<I', 0)
        )
        fcntl.ioctl(self.fd, UI_DEV_SETUP, setup)
        for slot, mx in ((0, SCREEN_W), (1, SCREEN_H)):
            fcntl.ioctl(self.fd, UI_ABS_SETUP, struct.pack('<i6i', slot, 0, 0, mx, 0, 0, 0))
        fcntl.ioctl(self.fd, UI_DEV_CREATE)
        time.sleep(0.4)  # let libinput enumerate

    def emit(self, etype, code, value):
        os.write(self.fd, INPUT_EVENT.pack(0, 0, etype, code, value))

    def sync(self):
        self.emit(EV_SYN, 0, 0)

    def move(self, x, y):
        self.emit(EV_ABS, ABS_X, max(0, min(SCREEN_W, int(x))))
        self.emit(EV_ABS, ABS_Y, max(0, min(SCREEN_H, int(y))))
        self.sync()

    def click(self, x=None, y=None, hold=0.06):
        if x is not None:
            self.move(x, y)
            time.sleep(0.12)
        self.emit(EV_KEY, BTN_LEFT, 1)
        self.sync()
        time.sleep(hold)
        self.emit(EV_KEY, BTN_LEFT, 0)
        self.sync()

    def drag(self, x0, y0, x1, y1, steps=14):
        self.move(x0, y0)
        time.sleep(0.15)
        self.emit(EV_KEY, BTN_LEFT, 1)
        self.sync()
        time.sleep(0.1)
        for i in range(1, steps + 1):
            self.move(x0 + (x1 - x0) * i // steps, y0 + (y1 - y0) * i // steps)
            time.sleep(0.05)
        self.emit(EV_KEY, BTN_LEFT, 0)
        self.sync()

    def close(self):
        fcntl.ioctl(self.fd, UI_DEV_DESTROY)
        os.close(self.fd)


def main():
    if len(sys.argv) < 2:
        print(__doc__)
        return 2
    p = Pointer()
    try:
        cmd = sys.argv[1]
        if cmd == 'move':
            p.move(int(sys.argv[2]), int(sys.argv[3]))
        elif cmd == 'click':
            p.click(int(sys.argv[2]), int(sys.argv[3]))
        elif cmd == 'drag':
            p.drag(*(int(v) for v in sys.argv[2:6]))
        elif cmd == 'probe':
            # visibility probe: small L movement then back
            p.move(1000, 900); time.sleep(0.4)
            p.move(1200, 900); time.sleep(0.4)
            print('probe done')
        return 0
    finally:
        time.sleep(0.3)
        p.close()


if __name__ == '__main__':
    sys.exit(main())
