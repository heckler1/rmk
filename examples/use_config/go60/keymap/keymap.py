#!/usr/bin/env python3
"""Write keymap.html, a picture of the Go60 layers in ../keyboard.toml."""

import json
import re
from pathlib import Path

HERE = Path(__file__).parent

# x, y, rotation (centidegrees), rotation origin x, y; 100 = one key width.
# From MoErgo's app/boards/arm/go60/go60-layouts.dtsi, in keymap order.
PHYSICAL = [
    (0, 50, 0, 0, 0),
    (100, 50, 0, 0, 0),
    (200, 0, 0, 0, 0),
    (300, 0, 0, 0, 0),
    (400, 0, 0, 0, 0),
    (500, 0, 0, 0, 0),
    (1150, 0, 0, 0, 0),
    (1250, 0, 0, 0, 0),
    (1350, 0, 0, 0, 0),
    (1450, 0, 0, 0, 0),
    (1550, 50, 0, 0, 0),
    (1650, 50, 0, 0, 0),
    (0, 150, 0, 0, 0),
    (100, 150, 0, 0, 0),
    (200, 100, 0, 0, 0),
    (300, 100, 0, 0, 0),
    (400, 100, 0, 0, 0),
    (500, 100, 0, 0, 0),
    (1150, 100, 0, 0, 0),
    (1250, 100, 0, 0, 0),
    (1350, 100, 0, 0, 0),
    (1450, 100, 0, 0, 0),
    (1550, 150, 0, 0, 0),
    (1650, 150, 0, 0, 0),
    (0, 250, 0, 0, 0),
    (100, 250, 0, 0, 0),
    (200, 200, 0, 0, 0),
    (300, 200, 0, 0, 0),
    (400, 200, 0, 0, 0),
    (500, 200, 0, 0, 0),
    (1150, 200, 0, 0, 0),
    (1250, 200, 0, 0, 0),
    (1350, 200, 0, 0, 0),
    (1450, 200, 0, 0, 0),
    (1550, 250, 0, 0, 0),
    (1650, 250, 0, 0, 0),
    (0, 350, 0, 0, 0),
    (100, 350, 0, 0, 0),
    (200, 300, 0, 0, 0),
    (300, 300, 0, 0, 0),
    (400, 300, 0, 0, 0),
    (500, 300, 0, 0, 0),
    (1150, 300, 0, 0, 0),
    (1250, 300, 0, 0, 0),
    (1350, 300, 0, 0, 0),
    (1450, 300, 0, 0, 0),
    (1550, 350, 0, 0, 0),
    (1650, 350, 0, 0, 0),
    (200, 400, 0, 0, 0),
    (300, 400, 0, 0, 0),
    (400, 400, 0, 0, 0),
    (1250, 400, 0, 0, 0),
    (1350, 400, 0, 0, 0),
    (1450, 400, 0, 0, 0),
    (520, 410, 1250, 520, 410),
    (640, 440, 2500, 640, 440),
    (750, 495, 3700, 750, 495),
    (915, 550, -3700, 915, 550),
    (1015, 480, -2500, 1015, 480),
    (1130, 430, -1250, 1130, 430),
]
MATRIX = (
    [(r, c) for r in range(4) for c in [*range(6), *range(8, 14)]]
    + [(4, 2), (4, 3), (4, 4), (4, 9), (4, 10), (4, 11)]
    + [(0, 6), (1, 6), (2, 6), (2, 7), (1, 7), (0, 7)]
)

src = (HERE.parent / "keyboard.toml").read_text()
layers = []
for m in re.finditer(r'\[\[keymap\.layer\]\]\s*name = "([^"]+)"\s*keys = """(.*?)"""', src, re.S):
    # Split on whitespace outside parentheses, so `TH(A, Macro(21))` stays one key.
    keys = re.findall(r"(?:[^\s(]|\((?:[^()]|\([^()]*\))*\))+", m[2])
    layers.append((m[1], keys[: len(PHYSICAL)]))  # drop the hidden (4,0) output toggle

NAMES = {
    "Escape": "Esc",
    "Backspace": "⌫",
    "Tab": "Tab",
    "LShift": "⇧",
    "Left": "←",
    "Right": "→",
    "Up": "↑",
    "Down": "↓",
    "Enter": "⏎",
    "Space": "Space",
    "Equal": "=",
    "Semicolon": ";",
    "Backslash": "\\",
    "Grave": "`",
    "Comma": ",",
    "Dot": ".",
    "LeftBracket": "[",
    "RightBracket": "]",
    "Quote": "'",
    "Slash": "/",
    "Minus": "-",
    "PageUp": "PgUp",
    "PageDown": "PgDn",
    "Home": "Home",
    "End": "End",
    "AudioVolDown": "Vol −",
    "AudioVolUp": "Vol +",
    "AudioMute": "Mute",
    "MediaPrevTrack": "⏮",
    "MediaNextTrack": "⏭",
    "MediaPlayPause": "⏯",
    "BrightnessDown": "Bright −",
    "BrightnessUp": "Bright +",
    "MouseBtn1": "Click",
    "Bootloader": "Boot",
    "Reboot": "Reset",
    "RgbSpi": "Speed +",
    "RgbSpd": "Speed −",
    "RgbSai": "Sat +",
    "RgbSad": "Sat −",
    "RgbHui": "Hue +",
    "RgbHud": "Hue −",
    "RgbVai": "Glow +",
    "RgbVad": "Glow −",
    "RgbTog": "Glow on/off",
    "RgbModeForward": "Effect",
}
SHIFTED = dict(zip("1234567890=;\\`,.[]/'-", '!@#$%^&*()+:|~<>{}?"_'))
SHIFTED_CODES = {
    "Equal",
    "Semicolon",
    "Backslash",
    "Grave",
    "Comma",
    "Dot",
    "LeftBracket",
    "RightBracket",
    "Quote",
    "Slash",
    "Minus",
}
MODS = {"LCtrl": "⌃", "LShift": "⇧", "LGui": "⌘", "LAlt": "⌥"}
MOD_WORD = {"LCtrl": "Ctrl", "LAlt": "Option", "LGui": "Cmd", "LShift": "Shift"}
# TD(n) and User(n) labels follow the morse order and src/*.rs in this example.
TD = ["LCtrl", "LAlt", "LGui", "LShift", None]
USER = {
    **{
        i: (
            f"BT {i + 1}",
            "bt",
            f"Selects Bluetooth profile {i + 1} and switches output to Bluetooth.",
        )
        for i in range(4)
    },
    6: ("Clear BT", "danger", "Clears the pairing for the active Bluetooth profile."),
    20: ("USB", "bt", "Switches output to USB."),
    21: (
        "Status",
        "system",
        "Shows battery and Bluetooth status on the left half’s LEDs for 10 seconds.",
    ),
}


def name(code):
    if re.fullmatch(r"Kc\d", code):
        return code[2]
    if re.fullmatch(r"[A-Z]", code):
        return code
    if re.fullmatch(r"F\d+", code):
        return code
    return NAMES.get(code, code)


def describe(code):
    """Return dict(tap, hold, dbl, kind, desc)."""
    if code == "_":
        return dict(kind="trans", desc="Transparent: uses the key from the layer below.")
    if code == "No":
        return dict(kind="none", desc="Does nothing.")
    m = re.fullmatch(r"TH\((\w+), Macro\((\d+)\)\)", code)
    if m:
        base = name(m[1])
        if len(base) == 1 and base.isalpha():
            return dict(
                tap=base,
                hold="⇧",
                kind="alpha",
                desc=f"Tap for {base.lower()}, hold for {base} (autoshift).",
            )
        sh = SHIFTED.get(base, "⇧" + base)
        kind = "mod" if m[1] in NAMES and m[1] not in SHIFTED_CODES else "alpha"
        return dict(
            tap=base, hold=sh, kind=kind, desc=f"Tap for {base}, hold for {sh} (autoshift)."
        )
    m = re.fullmatch(r"MO\((\d)\)", code)
    if m:
        n = layers[int(m[1])][0]
        return dict(tap=n, kind=f"layer{m[1]}", desc=f"Hold for the {n} layer.")
    m = re.fullmatch(r"TO\((\d)\)", code)
    if m:
        n = layers[int(m[1])][0]
        return dict(
            tap=f"→ {n}", kind=f"layer{m[1]}", desc=f"Switches to the {n} layer and stays there."
        )
    m = re.fullmatch(r"TD\((\d)\)", code)
    if m:
        td = TD[int(m[1])]
        if td is None:
            return dict(dbl="Lock", kind="system", desc="Double-tap to lock the Mac (⌃⌘Q).")
        mod = td
        return dict(
            tap=MODS[mod],
            hold=MOD_WORD[mod],
            dbl="latch 10s",
            kind="mod",
            desc=f"One-shot {MOD_WORD[mod]}: applies to the next key. Double-tap to latch it on. The latch times out after 10 seconds, or double-tap again to release it sooner.",
        )
    m = re.fullmatch(r"WM\((\w+), (\w+)\)", code)
    if m:
        base = name(m[1])
        if m[2] == "LShift" and base in SHIFTED:
            return dict(tap=SHIFTED[base], kind="alpha", desc=f"Types {SHIFTED[base]}.")
        label = MODS[m[2]] + base
        extra = {
            "⌃←": "Moves one macOS space left.",
            "⌃→": "Moves one macOS space right.",
            "⌃↑": "Opens Mission Control.",
        }.get(label, f"Sends {MOD_WORD[m[2]]}+{base}.")
        return dict(tap=label, kind="nav" if base in "←→↑" else "mod", desc=extra)
    m = re.fullmatch(r"User(\d+)", code)
    if m:
        lab, kind, d = USER[int(m[1])]
        return dict(tap=lab, kind=kind, desc=d)
    if code == "Macro(45)":
        return dict(
            tap="Clear all BT",
            kind="danger",
            desc="Clears every Bluetooth pairing and ends on profile 1.",
        )
    n = name(code)
    if code.startswith("Rgb"):
        return dict(tap=n, kind="rgb", desc=f"Underglow: {n}.")
    if code in ("Bootloader", "Reboot"):
        return dict(
            tap=n,
            kind="danger",
            desc="Restarts this half into the UF2 bootloader."
            if code == "Bootloader"
            else "Restarts this half.",
        )
    if code.startswith(("Audio", "Media", "Brightness")):
        return dict(tap=n, kind="media", desc=f"{n}.")
    if code in ("Left", "Right", "Up", "Down", "PageUp", "PageDown", "Home", "End"):
        return dict(tap=n, kind="nav", desc=f"{code}.")
    if re.fullmatch(r"F\d+", code):
        return dict(tap=n, kind="fn", desc=f"{code}.")
    if code in ("Escape", "Tab", "LShift", "Backspace", "Enter", "Space", "MouseBtn1"):
        return dict(tap=n, kind="mod", desc=f"{code}.")
    return dict(tap=n, kind="alpha", desc=f"{code}.")


data = {
    "keys": [
        dict(
            x=x,
            y=y,
            rot=rot / 100,
            rx=rx,
            ry=ry,
            pos=f"({r},{c})",
            side="left" if x < 875 else "right",
            thumb=i >= 54,
        )
        for i, ((x, y, rot, rx, ry), (r, c)) in enumerate(zip(PHYSICAL, MATRIX))
    ],
    "layers": [
        dict(name=n, keys=[dict(code=k, **describe(k)) for k in keys]) for n, keys in layers
    ],
}
page = (
    (HERE / "template.html").read_text().replace("__DATA__", json.dumps(data, ensure_ascii=False))
)
(HERE / "keymap.html").write_text(page, encoding="utf-8")
