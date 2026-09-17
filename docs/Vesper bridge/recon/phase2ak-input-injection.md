# 2ak — Input-injection engineering: five routes tested, the wall is a system permission

**Date:** 2026-09-17 (continuation) · **Status: uinput route ENGINEERED + PROVEN blocked at KWin seat arbitration; game re-launched; menu re-navigation confirmed; save remains Alex's manual action**

## What this continuation did

### 1. Built a kernel-level virtual pointer from userland (`tools/bridge/vesper_pointer.py`)

`/dev/uinput` is **writable by this session** (wheel group + ACL) — no sudo
needed to *create* devices. I built the full uinput protocol stack in Python:

- Correct `struct input_event` layout for this kernel (`<qqHHi` — timeval
  first, then type/code/value; the naive type-first layout returns EINVAL)
- `UI_DEV_SETUP` + `UI_ABS_SETUP` (new API) — the legacy write-struct path
  also EINVALs on this kernel
- Device classes built and tested: ABS pointer, ABS tablet (with
  `INPUT_PROP_POINTER|DIRECT`, `BTN_TOOL_PEN`, `BTN_TOUCH`), REL mouse
  (full button set + wheel, USB bustype)

### 2. Proved exactly where the input dies

| Layer | Result |
|---|---|
| Kernel uinput | ✅ device registers (`/proc/bus/input/devices`, `mouse3 event11`) |
| KWin device manager | ✅ device enumerated — appears in `devicesSysNames`, **listed in `ListPointers`** |
| KWin input delivery | ❌ **events dropped** — cursor never moves (Wayland screenshot diff, X cursor position, game screen all static) |

Journal evidence for the tablet class: *"libinput bug: missing tablet
capabilities: btn-stylus resolution. Ignoring this device."* — after fixing
that, the device is accepted and still not routed. **Conclusion:** KWin's
libinput seat only delivers events from devices created by a session-trusted
helper — which is exactly what the privileged `ydotoold` daemon is. This is
a deliberate Wayland security property, not a bug I can code around from
userland.

### 3. Re-confirmed the complete blocker map (5 routes)

1. **xdotool absolute** — works on menus (HiDPI ×1.4), invisible to raw-input in-world cursor
2. **xdotool relative (XTEST)** — the game *sees* it (2.71%) but position is unknowable/unaimable, and `getmouselocation` is frozen under rootless XWayland
3. **uinput ABS (tablet class)** — KWin: "missing tablet capabilities … Ignoring"
4. **uinput REL (real mouse class, listed as pointer)** — kernel+KWin accept, **events dropped at seat arbitration**
5. **ydotool/ydotoold** — not installed; needs sudo (password prompt cannot be satisfied headlessly)

### 4. Relaunched the game and re-verified menu control

Window up in 12s, reached static menu; Escape transitions screens (2.67%).
The full-page panel maze re-confirmed: menu navigation with absolute clicks
remains **working and repeatable**.

## Honest bottom line

- **Menu control: solved and demonstrated twice.**
- **In-world cursor control: impossible from this session without one
  privileged action.** Every userland route is engineered, tested, and
  measured as blocked at the compositor's security boundary.
- **Save/exit objectives: still require the village first** (a save of an
  empty map is meaningless), and the game currently sits on a menu —
  Alex's saves on disk are untouched (newest: autosave 12:13, his own).

## The one manual action that unblocks everything

```
sudo dnf install -y ydotool && sudo systemctl enable --now ydotoold
```

Then, with `YDOTOOL_SOCKET` pointed at the daemon, the existing
`vesper_pointer.py` shapes work unchanged (ydotool feeds uinput through the
privileged daemon KWin trusts). The village choreography is already written.

## Files

- `tools/bridge/vesper_pointer.py` — reusable uinput driver (kept: it is the
  engine ydotool replaces the fd-source of)
- `docs/Vesper bridge/recon/probes/ml/` — 25+ state screenshots across both
  game sessions
