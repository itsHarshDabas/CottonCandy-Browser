"""WCAG 2.x contrast for CottonCandy's tokens, with overlays composited onto the surfaces they sit on.

Usage: python3 docs/design/contrast.py   (exits 1 if any text or icon pair is below AA)
Tokens: docs/design/2026-10-20-cottoncandy-ui.md section 2. Dark values are grace-candyland's Theme.qml.
"""
import sys


def rgb(value):
    if isinstance(value, tuple):
        return value
    value = value.lstrip("#")
    return tuple(int(value[i : i + 2], 16) for i in (0, 2, 4))


def over(overlay, base):
    """overlay = (r, g, b, a) composited over an opaque base."""
    r, g, b, a = overlay
    base = rgb(base)
    return tuple(round(a * c + (1 - a) * bc) for c, bc in zip((r, g, b), base))


def lum(color):
    def channel(c):
        c /= 255
        return c / 12.92 if c <= 0.04045 else ((c + 0.055) / 1.055) ** 2.4

    r, g, b = (channel(c) for c in rgb(color))
    return 0.2126 * r + 0.7152 * g + 0.0722 * b


def ratio(a, b):
    la, lb = sorted((lum(a), lum(b)), reverse=True)
    return (la + 0.05) / (lb + 0.05)


def hexs(color):
    return "#%02x%02x%02x" % rgb(color)


def darken_to(color, background, target):
    """Darken (towards black) until contrast against background reaches target."""
    r, g, b = rgb(color)
    step = 0
    while ratio((r, g, b), background) < target and step < 200:
        r, g, b = (max(0, round(c * 0.98)) for c in (r, g, b))
        step += 1
    return (r, g, b)


W = lambda a: (255, 255, 255, a)  # noqa: E731
K = lambda a: (0, 0, 0, a)  # noqa: E731

ACCENTS = {
    "blue": ("#3584e4", "#2f75cb", "#4b92e7"),
    "teal": ("#2190a4", "#1d8092", "#399cae"),
    "green": ("#3a944a", "#348442", "#50a05e"),
    "yellow": ("#c88800", "#9c6a00", "#c88800"),
    "orange": ("#ed5b00", "#c74c00", "#ee650f"),
    "red": ("#e62d42", "#dd2b3f", "#ec6271"),
    "pink": ("#d56199", "#b55282", "#d96ba0"),
    "purple": ("#9141ac", "#9141ac", "#b37bc6"),
    "slate": ("#6f8396", "#657788", "#8092a3"),
}

DARK = dict(
    desktop="#121419", window="#1c1e25", titlebar="#181a20", sidebar="#17191f", panel="#17191f",
    hair=W(0.08), subtle=W(0.05), card=W(0.045), control=W(0.065), controlStrong=W(0.09),
    controlHover=W(0.12), selected=W(0.14), track=W(0.12),
    textStrong="#f1f2f6", text="#e6e8ee", textControl="#d9dce5", textSecondary="#8a90a0",
    textTertiary="#7d8392", success="#7ed99c", error="#ff8a8a",
)
LIGHT = dict(
    desktop="#f3f4f6", window="#ffffff", titlebar="#ebecef", sidebar="#f0f1f4", panel="#fafafb",
    hair=K(0.08), subtle=K(0.06), card=K(0.035), control=K(0.05), controlStrong=K(0.07),
    controlHover=K(0.08), selected=K(0.10), track=K(0.10),
    textStrong="#14161a", text="#1d1f24", textControl="#2b2f37", textSecondary="#575d6a",
    textTertiary="#868b97", success="#1a7f37", error="#c01c28",
)


def pairs(t):
    """(label, foreground, background, minimum) for every text or icon pairing the UI uses."""
    tb = t["titlebar"]
    field = over(t["control"], tb)
    out = [
        ("active tab title: textStrong on titlebar+selected", t["textStrong"], over(t["selected"], tb), 4.5),
        ("inactive tab title: textSecondary on titlebar", t["textSecondary"], tb, 4.5),
        ("hovered tab title: text on titlebar+controlHover", t["text"], over(t["controlHover"], tb), 4.5),
        ("URL host: text on titlebar+control", t["text"], field, 4.5),
        ("URL path: textSecondary on titlebar+control", t["textSecondary"], field, 4.5),
        ("placeholder: textSecondary on titlebar+control", t["textSecondary"], field, 4.5),
        ("button label: textControl on titlebar+controlStrong", t["textControl"], over(t["controlStrong"], tb), 4.5),
        ("toolbar icon: textControl on titlebar", t["textControl"], tb, 3.0),
        ("disabled icon: textTertiary on titlebar (exempt, shown for info)", t["textTertiary"], tb, 0),
        ("menu text: text on panel", t["text"], t["panel"], 4.5),
        ("menu secondary: textSecondary on panel", t["textSecondary"], t["panel"], 4.5),
        ("selected row: textStrong on panel+selected", t["textStrong"], over(t["selected"], t["panel"]), 4.5),
        ("side panel text: text on sidebar", t["text"], t["sidebar"], 4.5),
        ("side panel secondary: textSecondary on sidebar", t["textSecondary"], t["sidebar"], 4.5),
        ("group label: textSecondary on sidebar", t["textSecondary"], t["sidebar"], 4.5),
        ("NTP label: textSecondary on desktop", t["textSecondary"], t["desktop"], 4.5),
        ("NTP placeholder: textSecondary on desktop+control", t["textSecondary"], over(t["control"], t["desktop"]), 4.5),
        ("NTP card text: text on desktop+card", t["text"], over(t["card"], t["desktop"]), 4.5),
        ("error text on window", t["error"], t["window"], 4.5),
        ("error text on panel", t["error"], t["panel"], 4.5),
        ("secure lock icon: success on titlebar+control", t["success"], field, 3.0),
        ("hairline edge: hair on titlebar (decorative, shown for info)", over(t["hair"], tb), tb, 0),
    ]
    return out


def report(name, t, accent_text):
    print("\n## %s theme" % name)
    worst = []
    for label, fg, bg, need in pairs(t):
        r = ratio(fg, bg)
        flag = "ok " if r >= need else "LOW"
        if need == 0:
            flag = "-- "
        print("  %s %5.2f  (need %.1f)  %-62s %s on %s" % (flag, r, need, label, hexs(fg), hexs(bg)))
        if need and r < need:
            worst.append(label)
    print("\n  accents (%s):" % name)
    surfaces = {k: t[k] for k in ("titlebar", "panel", "sidebar", "desktop", "window")}
    surfaces["titlebar+control"] = over(t["control"], t["titlebar"])
    surfaces["panel+selected"] = over(t["selected"], t["panel"])
    text_surfaces = {k: v for k, v in surfaces.items() if k != "panel+selected"}
    for accent, (base, fill, fg) in ACCENTS.items():
        text_color = accent_text(accent)
        low_text = min(text_surfaces.items(), key=lambda kv: ratio(text_color, kv[1]))
        low_icon = min(surfaces.items(), key=lambda kv: ratio(text_color, kv[1]))
        on_fill = ratio("#ffffff", fill)
        ring = min(ratio(text_color, v) for v in surfaces.values())
        print(
            "    %-7s text %s  >= %.2f as text (worst: %s), >= %.2f as icon/ring (worst: %s) | white on fill %s %.2f"
            % (accent, hexs(text_color), ratio(text_color, low_text[1]), low_text[0],
               ratio(text_color, low_icon[1]), low_icon[0], fill, on_fill)
        )
        if ratio(text_color, low_text[1]) < 4.5:
            worst.append("accent text " + accent)
        if ring < 3:
            worst.append("accent icon/ring " + accent)
        if on_fill < 4.5:
            worst.append("white on fill " + accent)
    print("\n  failures:", worst or "none")
    return worst


if __name__ == "__main__":
    bad = report("Dark", DARK, lambda a: ACCENTS[a][2])
    # Light: the accent fill, darkened just enough to reach 4.5:1 on the darkest light surface it sits on.
    darkest = over(LIGHT["selected"], LIGHT["panel"])
    darkest = min((darkest, over(LIGHT["control"], LIGHT["titlebar"]), LIGHT["titlebar"]), key=lum)
    light_fg = {a: darken_to(ACCENTS[a][1], darkest, 4.5) for a in ACCENTS}
    bad += report("Light", LIGHT, lambda a: light_fg[a])
    print("\nlight fg tokens:", {a: hexs(c) for a, c in light_fg.items()})
    sys.exit(1 if bad else 0)
