#!/usr/bin/env python3
"""Builds app/src/main/assets/emoji.tsv, the keyboard's emoji list.

Inputs (Unicode data, Unicode License v3), downloaded to a folder given as
the only argument (default: next to this script), under these names:
  emoji-test.txt   https://unicode.org/Public/emoji/latest/emoji-test.txt
  es.xml, en.xml   cldr/common/annotations/{es,en}.xml
  es_d.xml, en_d.xml  cldr/common/annotationsDerived/{es,en}.xml

Output, one line each:
  #group <tab icon>
  <emoji>\t<search words: Spanish and English, lowercase, no accents>

Skin-tone variants are left out: the plain emoji is the one people pick, and
they would triple the list. The keyboard drops, at runtime, whatever the
phone's font cannot draw.
"""
import re
import sys
import unicodedata
import xml.etree.ElementTree as ET
from pathlib import Path

HERE = Path(__file__).parent
SRC = Path(sys.argv[1]) if len(sys.argv) > 1 else HERE
OUT = HERE.parent / "app/src/main/assets/emoji.tsv"
TONES = set(range(0x1F3FB, 0x1F400))
# Tab icons for Unicode's groups; "Component" (bare skin tones, hair) is not
# something to type.
ICONS = {
    "Smileys & Emotion": "😀",
    "People & Body": "👋",
    "Animals & Nature": "🐻",
    "Food & Drink": "🍔",
    "Travel & Places": "🚗",
    "Activities": "⚽",
    "Objects": "💡",
    "Symbols": "🔣",
    "Flags": "🏁",
}


def fold(s):
    s = unicodedata.normalize("NFD", s.lower())
    return "".join(c for c in s if unicodedata.category(c) != "Mn")


def annotations(*files):
    """Per emoji, each language's name (CLDR's `tts`) and then its keywords:
    the keyboard ranks a word earlier in the line as a better match."""
    words = {}
    for f in files:
        names, keywords = {}, {}
        for a in ET.parse(SRC / f).getroot().iter("annotation"):
            key = a.get("cp").replace("\ufe0f", "")
            into = names if a.get("type") == "tts" else keywords
            into.setdefault(key, []).extend(w.strip() for w in a.text.split("|"))
        for key in names.keys() | keywords.keys():
            words.setdefault(key, []).extend(names.get(key, []) + keywords.get(key, []))
    return words


def main():
    words = annotations("es.xml", "es_d.xml", "en.xml", "en_d.xml")
    out = ["# Unicode emoji and CLDR annotations, (c) Unicode, Inc., Unicode License v3."]
    group = None
    for line in (SRC / "emoji-test.txt").read_text(encoding="utf-8").splitlines():
        if line.startswith("# group: "):
            group = line[len("# group: "):]
            if group in ICONS:
                out.append(f"#group {ICONS[group]}")
            continue
        m = re.match(r"([0-9A-F ]+?)\s*; fully-qualified\s*# (\S+) E[\d.]+ (.*)", line)
        if not m or group not in ICONS:
            continue
        cps = [int(c, 16) for c in m[1].split()]
        if TONES & set(cps):
            continue
        glyph = "".join(map(chr, cps))
        found = words.get(glyph.replace("\ufe0f", ""), []) + [m[3]]
        # Each word once, in order: Spanish, then English, then Unicode's name.
        seen = dict.fromkeys(t for w in found for t in re.split(r"[\s:,]+", fold(w)) if t)
        out.append(f"{glyph}\t{' '.join(seen)}")
    OUT.write_text("\n".join(out) + "\n", encoding="utf-8")
    print(f"{sum(1 for l in out if not l.startswith('#'))} emoji -> {OUT}", file=sys.stderr)


if __name__ == "__main__":
    main()
