#!/usr/bin/env python3
"""Fact check a clip with a model you run: a Lapacho plugin (desktop).

Sends the clip with Fabric's `analyze_claims` pattern to the same
OpenAI-compatible server as terms-assistant, and keeps the model's report:
each claim, the evidence for and against it, and a rating. The model
answers from what it already knows; nothing is looked up on the web, and
the report says so.

Needs terms-assistant.py and its terms-assistant.local.json next to it (the
server is configured once, for both), and the pattern at
~/.config/fabric/patterns/analyze_claims/system.md (`fabric --setup`, or
`yt2action sync-patterns`).

Standard library only. Errors go to standard error, which Lapacho shows.
"""
import importlib.util
import os
import sys

here = os.path.dirname(os.path.abspath(__file__))
spec = importlib.util.spec_from_file_location("terms_assistant", os.path.join(here, "terms-assistant.py"))
if spec is None or not os.path.exists(spec.origin):
    print("fact-check needs terms-assistant.py next to it", file=sys.stderr)
    sys.exit(1)
ta = importlib.util.module_from_spec(spec)
spec.loader.exec_module(ta)

# The pattern's headings are English, and models follow them unless told twice.
LANGUAGE = "\n\nIMPORTANT: write the entire report, headings and labels included, in the language of the INPUT text (Spanish input, Spanish report)."
PATTERN = os.path.expanduser("~/.config/fabric/patterns/analyze_claims/system.md")


def main():
    endpoint, model, token = ta.settings()
    text = sys.stdin.read()
    if not text.strip():
        ta.fail("Nothing to check")
    if not endpoint.startswith(("http://", "https://")) or endpoint.rstrip("/").endswith("/api/terms/analyze"):
        ta.fail("Set an OpenAI-compatible server (its base URL) in terms-assistant.local.json")
    try:
        with open(PATTERN, encoding="utf-8") as f:
            pattern = f.read()
    except FileNotFoundError:
        ta.fail(f"No Fabric pattern at {PATTERN}: install Fabric's patterns first")
    base = endpoint.rstrip("/")
    v1 = base if base.endswith("/v1") else base + "/v1"
    model = ta.resolve_model(v1, model, token)
    body = {
        "messages": [
            {"role": "system", "content": pattern + LANGUAGE},
            {"role": "user", "content": text + "\n\n" + LANGUAGE.strip()},
        ],
        "temperature": 0,
        "chat_template_kwargs": {"enable_thinking": False},
    }
    if model:
        body["model"] = model
    reply = ta.post(v1 + "/chat/completions", body, token)
    try:
        content = reply["choices"][0]["message"]["content"]
    except (KeyError, IndexError, TypeError):
        ta.fail(f"Unexpected answer from {v1}: {str(reply)[:300]}")
    used = reply.get("model") or model or "model"
    print(f"{content.strip()}\n\n({used}, at {base}: from the model's own knowledge, nothing was looked up)")


if __name__ == "__main__":
    main()
