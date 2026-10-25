#!/usr/bin/env python3
"""Summarize a YouTube video from its link: a Lapacho plugin (desktop).

The clip holds a YouTube URL (or a bare video id). The transcript is
downloaded from this machine with `ytt`, or `yt-dlp` if there is no `ytt`,
and piped to `fabric -p summarize`, which uses the model set up with
`fabric --setup`. No paid service scrapes YouTube for you.

Standard library only. Errors go to standard error, which Lapacho shows.
"""
import glob
import os
import re
import shutil
import subprocess
import sys
import tempfile

ID = r"[A-Za-z0-9_-]{11}"
URL = re.compile(rf"(?:youtube\.com/(?:watch\?(?:\S*?&)?v=|shorts/|embed/|live/)|youtu\.be/)({ID})")


def fail(msg):
    print(msg, file=sys.stderr)
    sys.exit(1)


def video_id(text):
    m = URL.search(text)
    if m:
        return m.group(1)
    if re.fullmatch(ID, text.strip()):
        return text.strip()
    fail("No YouTube link in the clip")


def transcript(vid, tmp):
    langs = ["es", "en"]
    if shutil.which("ytt"):
        out = os.path.join(tmp, "t.txt")
        cmd = ["ytt", vid] + [a for l in langs for a in ("-l", l)] + ["-f", "text", "-o", out]
        subprocess.run(cmd, capture_output=True, text=True)
        if os.path.exists(out) and os.path.getsize(out):
            return open(out, encoding="utf-8").read()
    if shutil.which("yt-dlp"):
        subprocess.run(["yt-dlp", "--skip-download", "--write-subs", "--write-auto-subs",
                        "--sub-langs", ",".join(langs), "--sub-format", "vtt",
                        "-o", os.path.join(tmp, "s"), f"https://www.youtube.com/watch?v={vid}"],
                       capture_output=True, text=True)
        files = sorted(glob.glob(os.path.join(tmp, "s*.vtt")))
        if files:
            lines = []
            for l in open(files[0], encoding="utf-8").read().splitlines():
                l = re.sub(r"<[^>]+>", "", l).strip()
                if l and "-->" not in l and not l.startswith(("WEBVTT", "Kind:", "Language:")) and (not lines or lines[-1] != l):
                    lines.append(l)
            return "\n".join(lines)
    fail("Could not download a transcript: install ytt or yt-dlp, or the video has no captions")


def main():
    vid = video_id(sys.stdin.read())
    with tempfile.TemporaryDirectory() as tmp:
        text = transcript(vid, tmp)
    if not text.strip():
        fail("The transcript is empty")
    env = dict(os.environ, PATH=os.environ.get("PATH", "") + ":" + os.path.expanduser("~/go/bin"))
    if not shutil.which("fabric", path=env["PATH"]):
        fail("fabric not found: install it and run `fabric --setup`")
    r = subprocess.run(["fabric", "-p", "summarize"], input=text, capture_output=True, text=True, env=env)
    if r.returncode or not r.stdout.strip():
        fail(f"fabric failed: {r.stderr.strip()[:300] or 'empty answer'}")
    print(r.stdout.strip())


if __name__ == "__main__":
    main()
