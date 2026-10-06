#!/usr/bin/env python3
"""Builds the official dictionaries from hermitdave/FrequencyWords (MIT).

    python3 build.py <lang> <frequency list>   # e.g. build.py fr fr_50k.txt > fr.txt

Each language below is a recipe: the header, which entries are words, which
one-letter words to keep, and how to mend the subtitles' missing accents (see
docs/DICTIONARIES.md). A file this writes must be committed byte for byte and
its SHA-256 added to OFFICIAL_DICTIONARIES in Predict.kt.
"""
import re
import sys
import unicodedata


def strip_marks(word):
    """The word without its accents: what a missing accent turns it into."""
    return "".join(c for c in unicodedata.normalize("NFD", word) if unicodedata.category(c) != "Mn")


def merge_final(entries):
    """Italian: a word whose last vowel lost its accent (perche) and is less
    than twice as common as the accented one gives it its count. Real pairs
    (e/è 2.9x, si/sì 3.2x, la/là 89x) have the plain word far more common."""
    count = dict(entries)
    best = {}
    for a, _ in entries:
        if not re.search("[àèéìíòóùú]$", a):
            continue
        p = a[:-1] + strip_marks(a[-1])
        if p in count and count[p] < 2 * count[a] and (p not in best or count[a] > count[best[p]]):
            best[p] = a
    for p, a in best.items():
        count[a] += count.pop(p)
    return [(w, count[w]) for w, _ in entries if w in count]


def merge_dominated(entries):
    """Portuguese, French, German: a word that lost an accent anywhere (voce,
    ca, fur) and is under a tenth as common as the accented spelling gives it
    its count. Real pairs are near even (e/é 0.85, a/à 0.89, wurde/würde
    0.81), so the cut is far below them."""
    count = dict(entries)
    best = {}
    for a, c in entries:
        p = strip_marks(a)
        if p != a and p in count and count[p] < 0.1 * c and (p not in best or c > count[best[p]]):
            best[p] = a
    for p, a in best.items():
        count[a] += count.pop(p)
    return [(w, count[w]) for w, _ in entries if w in count]


LANGS = {
    "en": dict(header="#lang en\n#name English", letters="a-z", singles="ai"),
    "it": dict(
        header="#lang it\n#name Italiano\n#alternates e:èé a:à i:ì o:ò u:ù",
        letters="a-zàèéìíòóùú", singles="aeioè", mend=merge_final,
    ),
    "he": dict(header="#lang he\n#name עברית\n#rows קראטוןםפ שדגכעיחלךף זסבהנמצתץ", letters="א-ת", singles=""),
    "pt-br": dict(
        header="#lang pt-br\n#name Português (Brasil)\n#alternates c:ç a:ãáâà o:õóô e:éê i:í u:ú",
        letters="a-zàáâãçéêíóôõú", singles="aeoàé", mend=merge_dominated,
    ),
    "fr": dict(
        header="#lang fr\n#name Français\n#rows azertyuiop qsdfghjklm wxcvbn\n"
        "#alternates e:éèêë a:àâæ c:ç i:îï o:ôœ u:ùûü y:ÿ",
        letters="a-zàâæçéèêëîïôœùûüÿ", singles="aày", mend=merge_dominated,
    ),
    "de": dict(
        header="#lang de\n#name Deutsch\n#rows qwertzuiopü asdfghjklöä yxcvbnm\n#alternates s:ß",
        letters="a-zäöüß", singles="", mend=merge_dominated,
    ),
    "ru": dict(
        header="#lang ru\n#name Русский\n#rows йцукенгшщзхъ фывапролджэ ячсмитьбю\n#alternates е:ё",
        letters="а-яё", singles="авикосуя",
    ),
}


def build(lang, lines):
    r = LANGS[lang]
    word = re.compile(f"[{r['letters']}]+")
    entries = []
    for line in lines:
        f = line.split()
        if len(f) == 2 and word.fullmatch(f[0]) and (len(f[0]) > 1 or f[0] in r["singles"]):
            entries.append((f[0], int(f[1])))
    entries = r.get("mend", lambda e: e)(entries)
    return f"#lapacho-dict 1\n{r['header']}\n" + "".join(f"{w} {c}\n" for w, c in entries)


if __name__ == "__main__":
    lang, path = sys.argv[1:]
    with open(path, encoding="utf-8") as f:
        sys.stdout.write(build(lang, f))
