#!/usr/bin/env python3
"""Render the demo site's HTML pages from one layout.

`web/build.sh` calls this before it copies the static half. Every page is
`templates/layout.html` with `{{slot}}` substitutions, filled from a content
file in `pages/` that carries its own metadata — the same shape the org site
uses (`kryptic-sh.github.io/build.py`), for the same reason: the chrome lives
in one file, so adding the fourth demo does not mean editing the header,
the footer and the demo bar in four places and getting one of them wrong.

Deliberately not a dependency. This is stdlib Python with a regex and a string
replace, matching `web/`'s no-npm, no-bundler policy; a static site with three
pages does not need a generator, it needs the chrome factored out once.

Content format, identical to the org site's:

    <!--meta { "out": "index.html", "title": "…", … } meta-->
    <!--head--> optional per-page <style> or <link> <!--/head-->
    <!--body--> the page <!--/body-->

`out` is the path written under the site root, so a page chooses its own URL
rather than having one derived from its filename. Paths inside a page are
site-absolute (`/style.css`, `/demos/breakout/`), which is what lets the same
markup work at `/` and at `/demos/breakout/` without a base-href dance.
"""

from __future__ import annotations

import json
import re
import sys
from pathlib import Path
from typing import NoReturn

ROOT = Path(__file__).resolve().parent
SITE_URL = "https://crcbl.kryptic.sh"

# The demo bar, in the order it renders. `""` is the demo index.
#
# One list drives the bar on every page, so a new demo is one line here plus
# its content file — not an edit to each existing page.
DEMOS: list[tuple[str, str, str]] = [
    ("", "demos", "/"),
    ("breakout", "breakout", "/demos/breakout/"),
    ("flappy", "flappy", "/demos/flappy/"),
]

META_RE = re.compile(r"<!--meta\s*(.*?)\s*meta-->", re.S)
HEAD_RE = re.compile(r"<!--head-->\s*(.*?)\s*<!--/head-->", re.S)
BODY_RE = re.compile(r"<!--body-->\s*(.*?)\s*<!--/body-->", re.S)


def die(msg: str) -> NoReturn:
    print(f"error: {msg}", file=sys.stderr)
    sys.exit(1)


def parse(path: Path) -> tuple[dict, str, str]:
    text = path.read_text()
    meta_match = META_RE.search(text)
    if not meta_match:
        die(f"{path}: missing <!--meta ... meta--> block")
    try:
        meta = json.loads(meta_match.group(1))
    except json.JSONDecodeError as exc:
        die(f"{path}: bad metadata JSON: {exc}")
    body = BODY_RE.search(text)
    if not body:
        die(f"{path}: missing <!--body--> ... <!--/body--> block")
    head = HEAD_RE.search(text)
    return meta, (head.group(1) if head else ""), body.group(1)


def siblings_html(slug: str) -> str:
    """The bar back to the org site, then across the demos."""
    parts = [
        '<a href="https://www.kryptic.sh/">kryptic</a>',
        '<span class="sep">·</span>',
        '<a href="https://www.kryptic.sh/projects/crcbl/">about crcbl</a>',
    ]
    for demo_slug, label, href in DEMOS:
        parts.append('<span class="sep">·</span>')
        if demo_slug == slug:
            parts.append(f'<span class="current">{label}</span>')
        else:
            parts.append(f'<a href="{href}">{label}</a>')
    return "\n        ".join(parts)


def brand_html(slug: str) -> str:
    if not slug:
        return (
            '<span class="prompt">$</span> '
            '<a href="https://www.kryptic.sh/">kryptic</a>/crcbl'
            '<span class="cursor"></span>'
        )
    return (
        '<span class="prompt">$</span> '
        f'<a href="/">crcbl</a>/{slug}<span class="cursor"></span>'
    )


def nav_html(links: list[dict]) -> str:
    parts = []
    for link in links:
        href, label = link["href"], link["label"]
        if href.startswith("http"):
            # No `target="_blank"`, and therefore no `rel="noopener"` — that
            # attribute exists to blunt the risk of a new tab, so without one it
            # is noise. Whether a link opens in a new tab is the reader's call;
            # the arrow only marks it as leaving the site.
            parts.append(f'<a href="{href}">{label} ↗</a>')
        else:
            parts.append(f'<a href="{href}">{label}</a>')
    return "\n        ".join(parts)


def render(layout: str, meta: dict, head_extra: str, content: str) -> str:
    slug = meta.get("slug", "")
    out = meta["out"]
    canonical = SITE_URL + "/" + (out[: -len("index.html")] if out.endswith("index.html") else out)
    subs = {
        "title": meta["title"],
        "description": meta["description"],
        "canonical": canonical,
        "siblings": siblings_html(slug),
        "brand": brand_html(slug),
        "nav_links": nav_html(meta.get("nav", [])),
        "footer_links": meta.get(
            "footer", '<a href="https://github.com/kryptic-sh/crcbl">github</a>'
        ),
        "head_extra": head_extra,
        "body_end": meta.get("body_end", ""),
        "content": content,
    }
    page = layout
    for key, value in subs.items():
        page = page.replace("{{" + key + "}}", value)
    leftover = re.findall(r"\{\{\w+\}\}", page)
    if leftover:
        die(f"{out}: unsubstituted template vars: {leftover}")
    return page


def main() -> None:
    if len(sys.argv) != 2:
        die("usage: build-pages.py <site-dir>")
    site = Path(sys.argv[1]).resolve()

    layout = (ROOT / "templates" / "layout.html").read_text()
    pages = sorted((ROOT / "pages").glob("*.html"))
    if not pages:
        die("no pages found in web/pages/")

    written = set()
    for page in pages:
        meta, head_extra, body = parse(page)
        html = render(layout, meta, head_extra, body)
        out = site / meta["out"]
        if meta["out"] in written:
            die(f"{page}: two pages both write {meta['out']}")
        written.add(meta["out"])
        out.parent.mkdir(parents=True, exist_ok=True)
        out.write_text(html)
        print(f"  {meta['out']}")

    # Every demo the bar links to must exist, or the bar is a set of 404s. The
    # index is a page like any other, so this covers it too.
    for _, _label, href in DEMOS:
        expected = href.lstrip("/") + "index.html"
        if expected not in written:
            die(f"the demo bar links to /{href.lstrip('/')} but no page writes {expected}")

    print(f"rendered {len(written)} page(s)")


if __name__ == "__main__":
    main()
