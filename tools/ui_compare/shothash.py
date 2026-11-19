#!/usr/bin/env python3
"""Prints a short hash of a .pam shot, leaving out the places that name
the build's commit and so differ between two builds by themselves:
DIRECT mode's banner (x 760..930, y 0..21 at 1280 wide) and the Simple
UI's pause menu's corner (x 610..710, y 225..245 at 1024 wide, the size
the offscreen driver gives --simple-ui)."""
import hashlib, sys
d = open(sys.argv[1], 'rb').read()
h, b = d.split(b'ENDHDR\n', 1)
W = int(h.split(b'WIDTH ')[1].split()[0])
masks = {1280: (760, 930, 0, 22), 1024: (610, 710, 225, 246)}
if W in masks:
    x0, x1, y0, y1 = masks[W]
    b = bytearray(b)
    for y in range(y0, y1):
        b[(y * W + x0) * 4:(y * W + x1) * 4] = bytes((x1 - x0) * 4)
print(hashlib.md5(h + bytes(b)).hexdigest()[:6])
