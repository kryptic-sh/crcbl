// Render the demo site's HTML pages from one layout and a set of partials.
//
// `web/build.sh` calls this before it copies the static half. Every page is
// `templates/layout.html` with `{{slot}}` substitutions, filled from a content
// file in `pages/` that carries its own metadata — the same shape the org site
// uses (`kryptic-sh.github.io/build.py`), for the same reason: the chrome lives
// in one file, so adding the fourth demo does not mean editing the header, the
// footer and the demo bar in four places and getting one of them wrong.
//
// Deliberately not a dependency, and deliberately Node rather than the Python
// this was until 2026-08-19. It is `node:fs`, `node:path`, `node:url` and two
// regexes; nothing here needs npm, a bundler or a `package.json`, which is the
// same policy the rest of `web/tools` keeps. Node was already required — the
// export contract check, the boot smoke test, the static server and the browser
// e2e are all `.mjs` — so one language for the whole of `web/` is one runtime
// fewer to have installed.
//
// Content format, identical to the org site's:
//
//     <!--meta { "out": "index.html", "title": "…", … } meta-->
//     <!--head--> optional per-page <style> or <link> <!--/head-->
//     <!--body--> the page <!--/body-->
//
// `out` is the path written under the site root, so a page chooses its own URL
// rather than having one derived from its filename. Paths inside a page are
// site-absolute (`/style.css`, `/demos/breakout/`), which is what lets the same
// markup work at `/` and at `/demos/breakout/` without a base-href dance.
//
// PARTIALS. A page pulls a shared block in with
//
//     <!--include demo-window-->
//
// on a line of its own, which is replaced by `templates/demo-window.html`
// indented to that line's indent. The demo window — the terminal frame, the
// canvas, the status bar and the focus note — is the reason this exists: it is
// the same markup on every demo, and a change to it has to land on all of them
// from one edit. `REQUIRED_INCLUDES` below turns that from a convention into a
// build failure.
//
// INDENTATION IS PART OF THE OUTPUT. A slot's value is indented to the column
// its placeholder sits at, and a placeholder alone on a line with an empty value
// takes the line with it. Without that the rendered pages carry the content
// file's indentation into the layout's, and view-source shows markup that looks
// broken even though it parses.
//
// THE BUILD ALSO FAILS ON A LINK OR ASSET THAT WOULD 404. Every `href` and `src`
// in the built pages is resolved against what the finished site will contain —
// the pages this run wrote plus the static half `web/build.sh` still has to copy
// — so a typo'd path or a renamed file fails the build by name instead of
// shipping a broken link. Off-site URLs and in-page fragments are exempt:
// neither is resolvable offline. (The tag-balance half of an HTML validator is
// deliberately not here; it would need a dependency, and `web/` is no-npm.)

import { mkdirSync, readFileSync, readdirSync, writeFileSync } from 'node:fs';
import { dirname, extname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

// `web/`, which is this file's grandparent: the script lives in `web/tools/`
// and every path below is relative to the site sources, not to the tool.
const ROOT = dirname(dirname(fileURLToPath(import.meta.url)));
const SITE_URL = 'https://crcbl.kryptic.sh';

// The demo bar, in the order it renders. `''` is the demo index.
//
// One list drives the bar on every page, so a new demo is one line here plus
// its content file — not an edit to each existing page.
const DEMOS = [
  ['', 'demos', '/'],
  ['breakout', 'breakout', '/demos/breakout/'],
  ['flappy', 'flappy', '/demos/flappy/'],
  ['asteroids', 'asteroids', '/demos/asteroids/'],
  ['horde', 'horde', '/demos/horde/'],
  ['hud', 'hud', '/demos/hud/'],
  ['lantern', 'lantern', '/demos/lantern/'],
  ['quarry', 'quarry', '/demos/quarry/'],
  ['viewer', 'viewer', '/demos/viewer/'],
  ['orbit', 'orbit', '/demos/orbit/'],
  ['bracket', 'bracket', '/demos/bracket/'],
  ['puppet', 'puppet', '/demos/puppet/'],
  ['sparks', 'sparks', '/demos/sparks/'],
  ['breach', 'breach', '/demos/breach/'],
  ['shard', 'shard', '/demos/shard/'],
  ['options', 'options', '/demos/options/'],
];

// Partials every demo page must pull in, so "the demo window is one template"
// is checked rather than trusted. A page that hand-rolls the frame instead of
// including it fails the build, which is the only thing that keeps the next
// demo from being a copy-paste of this one.
const REQUIRED_INCLUDES = [
  'demo-window',
  'demo-loop-keys',
  'demo-console-note',
];

const META_RE = /<!--meta\s*([\s\S]*?)\s*meta-->/;
const HEAD_RE = /<!--head-->\s*([\s\S]*?)\s*<!--\/head-->/;
const BODY_RE = /<!--body-->\s*([\s\S]*?)\s*<!--\/body-->/;
// Tested and replaced, so it needs both flavours: a `g` regex carries a
// `lastIndex` that makes a bare `.test()` answer differently on alternate calls.
const INCLUDE_SOURCE = String.raw`^([ \t]*)<!--include\s+([a-z0-9-]+)\s*-->[ \t]*$`;
const INCLUDE_RE = new RegExp(INCLUDE_SOURCE, 'm');
const INCLUDE_RE_ALL = new RegExp(INCLUDE_SOURCE, 'gm');
const DOC_COMMENT_RE = /^\s*<!--[\s\S]*?-->[ \t]*\n?/;
// A placeholder that owns its whole line, and one that sits inside other markup.
const SLOT_LINE_RE = /^([ \t]*)\{\{(\w+)\}\}[ \t]*$/gm;
const SLOT_RE = /\{\{(\w+)\}\}/g;
// Every URL a built page can name, in the two attributes that load or navigate.
const HREF_SRC_RE = /\b(?:href|src)="([^"]+)"/g;

// The static half of the site, pruned exactly as `web/build.sh`'s `find` prunes
// it, so this list and the copy that ships the files cannot disagree about what
// the site will contain. The resolution check below runs before the copy does,
// which is why it resolves against the sources rather than the site directory.
const STATIC_EXCLUDED_DIRS = new Set(['tools', 'pages', 'templates']);
const STATIC_EXCLUDED_SUFFIXES = new Set(['.sh']);
const STATIC_EXCLUDED_NAMES = new Set(['README.md']);

// How many rounds of `<!--include-->` expansion a page gets before the build
// calls it a cycle. Nothing real nests at all — a demo page includes three
// partials and none of them includes anything — so this is a runaway guard.
const MAX_INCLUDE_DEPTH = 8;

function die(message) {
  process.stderr.write(`error: ${message}\n`);
  process.exit(1);
}

function parse(path) {
  const text = readFileSync(path, 'utf8');
  const meta = META_RE.exec(text);
  if (!meta) {
    die(`${path}: missing <!--meta ... meta--> block`);
  }
  let parsed;
  try {
    parsed = JSON.parse(meta[1]);
  } catch (error) {
    die(`${path}: bad metadata JSON: ${error.message}`);
  }
  const body = BODY_RE.exec(text);
  if (!body) {
    die(`${path}: missing <!--body--> ... <!--/body--> block`);
  }
  const head = HEAD_RE.exec(text);
  return { meta: parsed, head: head ? head[1] : '', body: body[1] };
}

// Drop a partial's leading comment: it documents the file, not the page.
//
// Every demo page includes every partial, so a comment left in would ship three
// times over — and it would have its `{{slot}}`s filled on the way, which turns
// a sentence about `{{slug}}` into one about `breakout`.
function stripDocComment(text) {
  return text.replace(DOC_COMMENT_RE, '').replace(/^\n+/, '');
}

/** `text` with every line after the first shifted to `indent`. */
function indentBlock(text, indent) {
  const lines = text.split('\n');
  return lines
    .map((line, at) => {
      if (at === 0) {
        return line;
      }
      return line.trim() ? indent + line : '';
    })
    .join('\n');
}

/** Replace `<!--include name-->` lines, recursively, recording the names. */
function expandIncludes(text, partials, seen, where) {
  for (let round = 0; round <= MAX_INCLUDE_DEPTH; round += 1) {
    if (!INCLUDE_RE.test(text)) {
      return text;
    }
    text = text.replace(INCLUDE_RE_ALL, (whole, indent, name) => {
      if (!Object.hasOwn(partials, name)) {
        die(`${where}: no such partial \`templates/${name}.html\``);
      }
      seen.push(name);
      return indent + indentBlock(partials[name], indent);
    });
  }
  die(
    `${where}: <!--include--> nested more than ${MAX_INCLUDE_DEPTH} deep; ` +
      'a partial includes itself'
  );
}

/** Fill `{{slot}}`s, keeping the result's indentation honest. */
function substitute(text, subs, where) {
  text = text.replace(SLOT_LINE_RE, (whole, indent, key) => {
    if (!Object.hasOwn(subs, key)) {
      return whole;
    }
    const value = subs[key];
    // An empty slot on its own line leaves a line of trailing whitespace
    // behind. `\0` marks it for removal below — a bare `\n` here would be eaten
    // by the next line's own match.
    if (!value.trim()) {
      return '\0';
    }
    return indent + indentBlock(value, indent);
  });

  text = text.replace(SLOT_RE, (whole, key, offset, whole_text) => {
    if (!Object.hasOwn(subs, key)) {
      return whole;
    }
    const value = subs[key];
    if (!value.includes('\n')) {
      return value;
    }
    // A block of markup written into an inline slot — `<main>{{content}}` —
    // opens a line of its own and closes one, indented one step in from the
    // element that holds it. Splicing it in where it sits instead is how the
    // built pages ended up with the layout's indentation on the first line and
    // the content file's on every other.
    const start =
      offset === 0 ? 0 : whole_text.lastIndexOf('\n', offset - 1) + 1;
    const before = whole_text.slice(start, offset);
    const outer = before.slice(0, before.length - before.trimStart().length);
    const inner = `${outer}  `;
    return `\n${inner}${indentBlock(value, inner)}\n${outer}`;
  });

  text = text.replaceAll('\0\n', '');
  const leftover = [...text.matchAll(SLOT_RE)].map((match) => match[1]);
  if (leftover.length > 0) {
    const names = [...new Set(leftover)].sort();
    die(`${where}: unsubstituted template vars: ${names.join(', ')}`);
  }
  return text;
}

/**
 * Every file the static half of the build will copy, site-root-absolute.
 *
 * The same set `web/build.sh`'s `find` ships, pruned by the same rules, so the
 * resolution check can run before the copy does and still know what the finished
 * site will contain.
 */
// The demo slugs `web/build.sh` builds artifacts for, read out of its own
// `DEMOS=( ... )` array.
//
// Two lists, deliberately: that one carries a crate and a lib name per row and
// this one carries a label and a href, which are different facts about a demo
// and would not travel together well. What they share is the *set of demos*,
// and that is what `checkDemoLists` holds them to — the shape of duplication
// worth a guard rather than a merge.
function shellDemoSlugs() {
  const text = readFileSync(join(ROOT, 'build.sh'), 'utf8');
  const block = /^DEMOS=\(\n([\s\S]*?)^\)$/m.exec(text);
  if (!block) {
    die('web/build.sh: no DEMOS=( ... ) array to read the demo list out of');
  }
  const slugs = [...block[1].matchAll(/^\s*"([^:"]+):/gm)].map((row) => row[1]);
  if (slugs.length === 0) {
    die(
      'web/build.sh: the DEMOS array parsed as empty, so this check would pass on anything'
    );
  }
  return slugs;
}

// Fail the build when the demo bar and the artifact build disagree about which
// demos exist.
//
// A demo added to one and not the other does not break either script: the site
// grows a nav entry pointing at a directory nothing built (a 404 the link
// checker cannot see, because the target is written by `build.sh` after this
// runs), or an artifact ships with no way to reach it. Both are silent.
function checkDemoLists() {
  // `''` is the demo index rather than a demo, so it is not a build target.
  const bar = DEMOS.map(([slug]) => slug).filter((slug) => slug !== '');
  const shell = shellDemoSlugs();
  const missing = shell.filter((slug) => !bar.includes(slug));
  const extra = bar.filter((slug) => !shell.includes(slug));
  if (missing.length > 0 || extra.length > 0) {
    die(
      `the demo lists disagree: web/build.sh builds ${JSON.stringify(missing)} ` +
        `that the demo bar in this file does not name, and the bar names ` +
        `${JSON.stringify(extra)} that web/build.sh does not build`
    );
  }
}

function staticSiteFiles() {
  const files = [];
  const walk = (directory, prefix) => {
    for (const entry of readdirSync(directory, { withFileTypes: true })) {
      if (STATIC_EXCLUDED_DIRS.has(entry.name)) {
        continue;
      }
      const relative = prefix ? `${prefix}/${entry.name}` : entry.name;
      if (entry.isDirectory()) {
        walk(join(directory, entry.name), relative);
      } else if (entry.isFile()) {
        if (
          STATIC_EXCLUDED_SUFFIXES.has(extname(entry.name)) ||
          STATIC_EXCLUDED_NAMES.has(entry.name)
        ) {
          continue;
        }
        files.push(`/${relative}`);
      }
    }
  };
  walk(ROOT, '');
  return files;
}

/**
 * A URL the resolution check can neither resolve nor be expected to: off-site,
 * an in-page fragment, or a scheme that names no file at all.
 */
function skipLink(target) {
  const lowered = target.toLowerCase();
  const schemes = ['http://', 'https://', '//', 'mailto:', 'tel:', 'data:'];
  return (
    schemes.some((scheme) => lowered.startsWith(scheme)) ||
    target.startsWith('#')
  );
}

/**
 * Every `href` and `src` in the built pages resolves to something the site will
 * contain: a page this run wrote, a file the static half will provide, or a
 * directory whose `index.html` this run wrote. A link that names no such thing
 * is a 404 waiting for a visitor, so it fails the build by name.
 */
function checkLinks(rendered) {
  const written = new Set(rendered.map(({ out }) => `/${out}`));
  const staticFiles = new Set(staticSiteFiles());
  for (const { out, html } of rendered) {
    const base = `/${out.endsWith('index.html') ? out.slice(0, -'index.html'.length) : out}`;
    for (const [, target] of html.matchAll(HREF_SRC_RE)) {
      if (skipLink(target)) {
        continue;
      }
      // A fragment or query on a resolvable path still has to resolve.
      const path = target.split('#')[0].split('?')[0];
      const resolved = path.startsWith('/') ? path : base + path;
      const ok = resolved.endsWith('/')
        ? written.has(`${resolved}index.html`)
        : written.has(resolved) || staticFiles.has(resolved);
      if (!ok) {
        die(`${out}: ${target} names no page, static file or directory index`);
      }
    }
  }
}

/** The bar back to the org site, then across the demos. */
function siblingsHtml(slug) {
  const parts = [
    '<a href="https://www.kryptic.sh/">kryptic</a>',
    '<span class="sep">·</span>',
    '<a href="https://www.kryptic.sh/projects/crcbl/">about crcbl</a>',
  ];
  for (const [demoSlug, label, href] of DEMOS) {
    parts.push('<span class="sep">·</span>');
    parts.push(
      demoSlug === slug
        ? `<span class="current">${label}</span>`
        : `<a href="${href}">${label}</a>`
    );
  }
  return parts.join('\n');
}

function brandHtml(slug) {
  if (!slug) {
    return (
      '<span class="prompt">$</span> ' +
      '<a href="https://www.kryptic.sh/">kryptic</a>/crcbl' +
      '<span class="cursor"></span>'
    );
  }
  return (
    '<span class="prompt">$</span> ' +
    `<a href="/">crcbl</a>/${slug}<span class="cursor"></span>`
  );
}

function navHtml(links) {
  return links
    .map(({ href, label }) =>
      // No `target="_blank"`, and therefore no `rel="noopener"` — that attribute
      // exists to blunt the risk of a new tab, so without one it is noise.
      // Whether a link opens in a new tab is the reader's call; the arrow only
      // marks it as leaving the site.
      href.startsWith('http')
        ? `<a href="${href}">${label} ↗</a>`
        : `<a href="${href}">${label}</a>`
    )
    .join('\n');
}

function render(layout, partials, meta, headExtra, content) {
  const slug = meta.slug ?? '';
  const out = meta.out;
  const canonical =
    SITE_URL +
    '/' +
    (out.endsWith('index.html') ? out.slice(0, -'index.html'.length) : out);
  const subs = {
    title: meta.title,
    description: meta.description,
    canonical,
    slug,
    // What the demo is called in prose: the window's title bar and the canvas's
    // accessible name both come from it.
    name: meta.name ?? meta.title.split(' — ')[0],
    siblings: siblingsHtml(slug),
    brand: brandHtml(slug),
    nav_links: navHtml(meta.nav ?? []),
    footer_links:
      meta.footer ?? '<a href="https://github.com/kryptic-sh/crcbl">github</a>',
    head_extra: headExtra,
    body_end: meta.body_end ?? '',
  };
  const used = [];
  // Includes first and the content's own slots second, because a partial
  // carries `{{name}}` of its own and a value spliced into the layout is not
  // rescanned. Then the finished content goes into the layout.
  content = expandIncludes(content, partials, used, out);
  content = substitute(content, subs, out);
  const page = substitute(layout, { ...subs, content }, out);
  return { page, used };
}

function main() {
  if (process.argv.length !== 3) {
    die('usage: build-pages.mjs <site-dir>');
  }
  const site = resolve(process.argv[2]);

  checkDemoLists();

  const templates = join(ROOT, 'templates');
  const layout = readFileSync(join(templates, 'layout.html'), 'utf8');
  const partials = {};
  for (const name of readdirSync(templates).sort()) {
    if (!name.endsWith('.html') || name === 'layout.html') {
      continue;
    }
    const text = readFileSync(join(templates, name), 'utf8');
    partials[name.slice(0, -'.html'.length)] = stripDocComment(text).replace(
      /\n+$/,
      ''
    );
  }

  const pages = readdirSync(join(ROOT, 'pages'))
    .filter((name) => name.endsWith('.html'))
    .sort();
  if (pages.length === 0) {
    die('no pages found in web/pages/');
  }

  const written = new Set();
  const includesBySlug = new Map();
  const rendered = [];
  for (const name of pages) {
    const path = join(ROOT, 'pages', name);
    const { meta, head, body } = parse(path);
    const { page, used } = render(layout, partials, meta, head, body);
    if (written.has(meta.out)) {
      die(`${path}: two pages both write ${meta.out}`);
    }
    written.add(meta.out);
    rendered.push({ out: meta.out, html: page });
    includesBySlug.set(meta.slug ?? '', used);
    const target = join(site, meta.out);
    mkdirSync(dirname(target), { recursive: true });
    writeFileSync(target, page);
    process.stdout.write(
      `  ${meta.out}${used.length > 0 ? `  (+${used.join(', ')})` : ''}\n`
    );
  }

  // Every demo the bar links to must exist, or the bar is a set of 404s. The
  // index is a page like any other, so this covers it too.
  for (const [slug, , href] of DEMOS) {
    const expected = `${href.replace(/^\/+/, '')}index.html`;
    if (!written.has(expected)) {
      die(
        `the demo bar links to /${href.replace(/^\/+/, '')} but no page writes ${expected}`
      );
    }
    if (!slug) {
      continue;
    }
    // The point of the partials: a demo that stops including one has gone back
    // to its own copy of the window, and every later edit to the shared one will
    // silently miss it.
    const used = includesBySlug.get(slug) ?? [];
    const missing = REQUIRED_INCLUDES.filter((name) => !used.includes(name));
    if (missing.length > 0) {
      die(
        `the ${slug} demo page does not <!--include--> ${missing.join(', ')}; ` +
          'every demo renders the same window from templates/'
      );
    }
  }

  // The site's own links and assets, resolved against what the finished site
  // will contain (the pages just written plus the static half still to be
  // copied) — see `checkLinks`.
  checkLinks(rendered);

  process.stdout.write(`rendered ${written.size} page(s)\n`);
}

main();
