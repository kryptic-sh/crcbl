#!/usr/bin/env python3
"""Render the demo site's HTML pages from one layout and a set of partials.

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

PARTIALS. A page pulls a shared block in with

    <!--include demo-window-->

on a line of its own, which is replaced by `templates/demo-window.html`
indented to that line's indent. The demo window — the terminal frame, the
canvas, the status bar and the focus note — is the reason this exists: it is
the same markup on every demo, and a change to it has to land on all of them
from one edit. `REQUIRED_INCLUDES` below turns that from a convention into a
build failure.

INDENTATION IS PART OF THE OUTPUT. A slot's value is indented to the column
its placeholder sits at, and a placeholder alone on a line with an empty value
takes the line with it. Without that the rendered pages carry the content
file's indentation into the layout's, and view-source shows markup that looks
broken even though it parses.

THE BUILD ALSO FAILS ON A LINK OR ASSET THAT WOULD 404. Every `href` and `src`
in the built pages is resolved against what the finished site will contain —
the pages this run wrote plus the static half `web/build.sh` still has to copy —
so a typo'd path or a renamed file fails the build by name instead of shipping
a broken link. Off-site URLs and in-page fragments are exempt: neither is
resolvable offline. (The tag-balance half of an HTML validator is deliberately
not here; it would need a dependency, and `web/` is no-npm.)
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
    ("asteroids", "asteroids", "/demos/asteroids/"),
    ("horde", "horde", "/demos/horde/"),
]

# Partials every demo page must pull in, so "the demo window is one template"
# is checked rather than trusted. A page that hand-rolls the frame instead of
# including it fails the build, which is the only thing that keeps the next
# demo from being a copy-paste of this one.
REQUIRED_INCLUDES = ("demo-window", "demo-loop-keys", "demo-console-note")

META_RE = re.compile(r"<!--meta\s*(.*?)\s*meta-->", re.S)
HEAD_RE = re.compile(r"<!--head-->\s*(.*?)\s*<!--/head-->", re.S)
BODY_RE = re.compile(r"<!--body-->\s*(.*?)\s*<!--/body-->", re.S)
INCLUDE_RE = re.compile(r"^([ \t]*)<!--include\s+([a-z0-9-]+)\s*-->[ \t]*$", re.M)
DOC_COMMENT_RE = re.compile(r"\A\s*<!--.*?-->[ \t]*\n?", re.S)
# A placeholder that owns its whole line, and one that sits inside other markup.
SLOT_LINE_RE = re.compile(r"^([ \t]*)\{\{(\w+)\}\}[ \t]*$", re.M)
SLOT_RE = re.compile(r"\{\{(\w+)\}\}")
# Every URL a built page can name, in the two attributes that load or navigate.
HREF_SRC_RE = re.compile(r'\b(?:href|src)="([^"]+)"')

# The static half of the site, pruned exactly as `web/build.sh`'s `find` prunes
# it, so this list and the copy that ships the files cannot disagree about what
# the site will contain. The resolution check below runs before the copy does,
# which is why it resolves against the sources rather than the site directory.
STATIC_EXCLUDED_DIRS = {"tools", "pages", "templates"}
STATIC_EXCLUDED_SUFFIXES = {".sh", ".py"}
STATIC_EXCLUDED_NAMES = {"README.md"}


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


def strip_doc_comment(text: str) -> str:
    """Drop a partial's leading comment: it documents the file, not the page.

    Every demo page includes every partial, so a comment left in would ship
    three times over — and it would have its `{{slot}}`s filled on the way,
    which turns a sentence about `{{slug}}` into one about `breakout`.
    """
    return DOC_COMMENT_RE.sub("", text, count=1).lstrip("\n")


def indent_block(text: str, indent: str) -> str:
    """`text` with every line after the first shifted to `indent`."""
    lines = text.split("\n")
    return "\n".join(
        [lines[0]] + [(indent + line if line.strip() else "") for line in lines[1:]]
    )


def expand_includes(text: str, partials: dict[str, str], seen: list[str], where: str) -> str:
    """Replace `<!--include name-->` lines, recursively, recording the names."""
    for _ in range(8):
        if not INCLUDE_RE.search(text):
            return text

        def swap(match: re.Match[str]) -> str:
            indent, name = match.group(1), match.group(2)
            if name not in partials:
                die(f"{where}: no such partial `templates/{name}.html`")
            seen.append(name)
            return indent + indent_block(partials[name], indent)

        text = INCLUDE_RE.sub(swap, text)
    die(f"{where}: <!--include--> nested more than 8 deep; a partial includes itself")


def substitute(text: str, subs: dict[str, str], where: str) -> str:
    """Fill `{{slot}}`s, keeping the result's indentation honest."""

    def line_slot(match: re.Match[str]) -> str:
        indent, key = match.group(1), match.group(2)
        if key not in subs:
            return match.group(0)
        value = subs[key]
        # An empty slot on its own line leaves a line of trailing whitespace
        # behind. `\x00` marks it for removal below — a bare `\n` here would be
        # eaten by the next line's own match.
        if not value.strip():
            return "\x00"
        return indent + indent_block(value, indent)

    def inline_slot(match: re.Match[str]) -> str:
        key = match.group(1)
        if key not in subs:
            return match.group(0)
        value = subs[key]
        if "\n" not in value:
            return value
        # A block of markup written into an inline slot — `<main>{{content}}` —
        # opens a line of its own and closes one, indented one step in from the
        # element that holds it. Splicing it in where it sits instead is how
        # the built pages ended up with the layout's indentation on the first
        # line and the content file's on every other.
        line = text.rfind("\n", 0, match.start()) + 1
        outer = text[line : match.start()]
        outer = outer[: len(outer) - len(outer.lstrip())]
        inner = outer + "  "
        return "\n" + inner + indent_block(value, inner) + "\n" + outer

    text = SLOT_LINE_RE.sub(line_slot, text)
    text = SLOT_RE.sub(inline_slot, text)
    text = re.sub(r"\x00\n", "", text)
    leftover = SLOT_RE.findall(text)
    if leftover:
        die(f"{where}: unsubstituted template vars: {sorted(set(leftover))}")
    return text


def static_site_files() -> list[str]:
    """Every file the static half of the build will copy, site-root-absolute.

    The same set `web/build.sh`'s `find` ships, pruned by the same rules, so
    the resolution check can run before the copy does and still know what the
    finished site will contain.
    """
    files = []
    for path in sorted(ROOT.rglob("*")):
        if not path.is_file():
            continue
        relative = path.relative_to(ROOT)
        if any(part in STATIC_EXCLUDED_DIRS for part in relative.parts):
            continue
        if path.suffix in STATIC_EXCLUDED_SUFFIXES or path.name in STATIC_EXCLUDED_NAMES:
            continue
        files.append("/" + relative.as_posix())
    return files


def _skip_link(target: str) -> bool:
    """A URL the resolution check can neither resolve nor be expected to: off-site,
    an in-page fragment, or a scheme that names no file at all."""
    lowered = target.lower()
    return (
        lowered.startswith(("http://", "https://", "//", "mailto:", "tel:", "data:"))
        or target.startswith("#")
    )


def check_links(rendered: list[tuple[str, str]]) -> None:
    """Every `href` and `src` in the built pages resolves to something the site
    will contain: a page this run wrote, a file the static half will provide,
    or a directory whose `index.html` this run wrote. A link that names no such
    thing is a 404 waiting for a visitor, so it fails the build by name."""
    written = {"/" + out for out, _ in rendered}
    static = set(static_site_files())
    for out, html in rendered:
        base = "/" + out.removesuffix("index.html")
        for target in HREF_SRC_RE.findall(html):
            if _skip_link(target):
                continue
            # A fragment or query on a resolvable path still has to resolve.
            path = target.split("#", 1)[0].split("?", 1)[0]
            resolved = path if path.startswith("/") else base + path
            if resolved.endswith("/"):
                ok = resolved + "index.html" in written
            else:
                ok = resolved in written or resolved in static
            if not ok:
                die(f"{out}: {target} names no page, static file or directory index")


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
    return "\n".join(parts)


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
    return "\n".join(parts)


def render(
    layout: str,
    partials: dict[str, str],
    meta: dict,
    head_extra: str,
    content: str,
) -> tuple[str, list[str]]:
    slug = meta.get("slug", "")
    out = meta["out"]
    canonical = SITE_URL + "/" + (out[: -len("index.html")] if out.endswith("index.html") else out)
    subs = {
        "title": meta["title"],
        "description": meta["description"],
        "canonical": canonical,
        "slug": slug,
        # What the demo is called in prose: the window's title bar and the
        # canvas's accessible name both come from it.
        "name": meta.get("name", meta["title"].split(" — ")[0]),
        "siblings": siblings_html(slug),
        "brand": brand_html(slug),
        "nav_links": nav_html(meta.get("nav", [])),
        "footer_links": meta.get(
            "footer", '<a href="https://github.com/kryptic-sh/crcbl">github</a>'
        ),
        "head_extra": head_extra,
        "body_end": meta.get("body_end", ""),
    }
    used: list[str] = []
    # Includes first and the content's own slots second, because a partial
    # carries `{{name}}` of its own and a value spliced into the layout is not
    # rescanned. Then the finished content goes into the layout.
    content = expand_includes(content, partials, used, out)
    content = substitute(content, subs, out)
    page = substitute(layout, {**subs, "content": content}, out)
    return page, used


def main() -> None:
    if len(sys.argv) != 2:
        die("usage: build-pages.py <site-dir>")
    site = Path(sys.argv[1]).resolve()

    templates = ROOT / "templates"
    layout = (templates / "layout.html").read_text()
    partials = {
        path.stem: strip_doc_comment(path.read_text()).rstrip("\n")
        for path in sorted(templates.glob("*.html"))
        if path.name != "layout.html"
    }
    pages = sorted((ROOT / "pages").glob("*.html"))
    if not pages:
        die("no pages found in web/pages/")

    written = set()
    includes_by_slug: dict[str, list[str]] = {}
    rendered: list[tuple[str, str]] = []
    for page in pages:
        meta, head_extra, body = parse(page)
        html, used = render(layout, partials, meta, head_extra, body)
        out = site / meta["out"]
        if meta["out"] in written:
            die(f"{page}: two pages both write {meta['out']}")
        written.add(meta["out"])
        rendered.append((meta["out"], html))
        includes_by_slug[meta.get("slug", "")] = used
        out.parent.mkdir(parents=True, exist_ok=True)
        out.write_text(html)
        print(f"  {meta['out']}" + (f"  (+{', '.join(used)})" if used else ""))

    # Every demo the bar links to must exist, or the bar is a set of 404s. The
    # index is a page like any other, so this covers it too.
    for slug, _label, href in DEMOS:
        expected = href.lstrip("/") + "index.html"
        if expected not in written:
            die(f"the demo bar links to /{href.lstrip('/')} but no page writes {expected}")
        if not slug:
            continue
        # The point of the partials: a demo that stops including one has gone
        # back to its own copy of the window, and every later edit to the shared
        # one will silently miss it.
        missing = [n for n in REQUIRED_INCLUDES if n not in includes_by_slug.get(slug, [])]
        if missing:
            die(
                f"the {slug} demo page does not <!--include--> {', '.join(missing)}; "
                "every demo renders the same window from templates/"
            )

    # The site's own links and assets, resolved against what the finished site
    # will contain (the pages just written plus the static half still to be
    # copied) — see [`check_links`].
    check_links(rendered)

    print(f"rendered {len(written)} page(s)")


if __name__ == "__main__":
    main()
