#!/usr/bin/env python3
"""Render a `tmux capture-pane -e` screen as HTML for review pages."""

from __future__ import annotations

import argparse
import html
import re
import sys

# xterm default 16-colour palette.
BASE16 = [
    "#000000", "#cd0000", "#00cd00", "#cdcd00", "#0000ee", "#cd00cd", "#00cdcd", "#e5e5e5",
    "#7f7f7f", "#ff0000", "#00ff00", "#ffff00", "#5c5cff", "#ff00ff", "#00ffff", "#ffffff",
]
DEFAULT_FG = "#d0d0d0"
DEFAULT_BG = "#111111"
SGR = re.compile(r"\x1b\[([0-9;:]*)m")
OTHER_ESCAPE = re.compile(r"\x1b(\[[0-9;?]*[A-Za-ln-z]|\][^\x07]*\x07|[()][A-Z0-9])")


def xterm256(n: int) -> str:
    if n < 16:
        return BASE16[n]
    if n < 232:
        n -= 16
        steps = [0, 95, 135, 175, 215, 255]
        return "#%02x%02x%02x" % (steps[n // 36], steps[(n // 6) % 6], steps[n % 6])
    level = 8 + (n - 232) * 10
    return "#%02x%02x%02x" % (level, level, level)


class Style:
    def __init__(self) -> None:
        self.reset()

    def reset(self) -> None:
        self.fg: str | None = None
        self.bg: str | None = None
        self.bold = self.dim = self.italic = self.underline = self.reverse = False

    def apply(self, params: str) -> None:
        codes = [int(p) if p else 0 for p in re.split("[;:]", params)] if params else [0]
        i = 0
        while i < len(codes):
            c = codes[i]
            if c == 0:
                self.reset()
            elif c == 1:
                self.bold = True
            elif c == 2:
                self.dim = True
            elif c == 3:
                self.italic = True
            elif c == 4:
                self.underline = True
            elif c == 7:
                self.reverse = True
            elif c == 22:
                self.bold = self.dim = False
            elif c == 23:
                self.italic = False
            elif c == 24:
                self.underline = False
            elif c == 27:
                self.reverse = False
            elif 30 <= c <= 37:
                self.fg = BASE16[c - 30]
            elif 90 <= c <= 97:
                self.fg = BASE16[c - 90 + 8]
            elif c == 39:
                self.fg = None
            elif 40 <= c <= 47:
                self.bg = BASE16[c - 40]
            elif 100 <= c <= 107:
                self.bg = BASE16[c - 100 + 8]
            elif c == 49:
                self.bg = None
            elif c in (38, 48) and i + 1 < len(codes):
                colour = None
                if codes[i + 1] == 5 and i + 2 < len(codes):
                    colour = xterm256(codes[i + 2])
                    i += 2
                elif codes[i + 1] == 2 and i + 4 < len(codes):
                    colour = "#%02x%02x%02x" % tuple(codes[i + 2 : i + 5])
                    i += 4
                if c == 38:
                    self.fg = colour
                else:
                    self.bg = colour
            i += 1

    def css(self) -> str:
        fg, bg = self.fg, self.bg
        if self.reverse:
            fg, bg = (bg or DEFAULT_BG), (fg or DEFAULT_FG)
        parts = []
        if fg:
            parts.append(f"color:{fg}")
        if bg:
            parts.append(f"background:{bg}")
        if self.bold:
            parts.append("font-weight:bold")
        if self.dim:
            parts.append("opacity:.6")
        if self.italic:
            parts.append("font-style:italic")
        if self.underline:
            parts.append("text-decoration:underline")
        return ";".join(parts)


def convert(text: str) -> str:
    style = Style()
    out: list[str] = []
    open_span = False
    pos = 0
    text = OTHER_ESCAPE.sub("", text)
    for match in SGR.finditer(text):
        chunk = text[pos : match.start()]
        if chunk:
            out.append(html.escape(chunk))
        if open_span:
            out.append("</span>")
            open_span = False
        style.apply(match.group(1))
        css = style.css()
        if css:
            out.append(f'<span style="{css}">')
            open_span = True
        pos = match.end()
    out.append(html.escape(text[pos:]))
    if open_span:
        out.append("</span>")
    return "".join(out)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--fragment", action="store_true", help="emit only the <pre> element")
    args = parser.parse_args()
    pre = (
        f'<pre class="term" style="color:{DEFAULT_FG};background:{DEFAULT_BG};'
        f'line-height:1.15;font-family:ui-monospace,Menlo,monospace;padding:8px;margin:0">'
        f"{convert(sys.stdin.read())}</pre>"
    )
    if args.fragment:
        print(pre)
    else:
        print(f"<!doctype html><meta charset=utf-8><body style='background:{DEFAULT_BG}'>{pre}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
