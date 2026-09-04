#!/usr/bin/env node
// The static server the demo site is served from, and the two headers that
// decide whether the browser will give it shared memory.
//
//   node web/tools/serve.mjs <root> [--port 8000] [--no-isolation]
//
// `web/build.sh --serve` runs it as a command; `web/tools/browser-e2e.mjs`
// imports `serve` and drives the same code on an ephemeral port. One
// implementation on purpose: the e2e asserts `crossOriginIsolated === true`
// against whatever this file sends, and a second server for humans would be a
// second set of headers nothing checks — so the demo that works under the gate
// could stop working under `--serve` with nothing going red.
//
// `--no-isolation`, and the `isolated: false` option behind it, withhold the
// pair. **The origin without them is not a broken configuration — it is the
// published one.** GitHub Pages sends neither header, so every visitor to
// crcbl.kryptic.sh gets a document where `SharedArrayBuffer` does not exist and
// `crcbl_jobs`'s browser backend degrades onto `Inline`'s behaviour. That is a
// supported configuration rather than a gap, which means it is one the gate owes
// coverage of: `web/run-jobs-e2e.sh` drives its page twice, once each way, and
// `docs/plan/21-jobs.md`'s first rung is the reasoning.
//
// Plain Node, no dependencies: `web/` has a no-npm policy and `build.sh`
// already needs `node` for `check-exports.mjs` and `smoke.mjs`.
//
// WHY THE ISOLATION HEADERS ARE HERE AT ALL
//   `SharedArrayBuffer` — and therefore `new WebAssembly.Memory({ shared: true })`,
//   and therefore any wasm build with `+atomics` — is refused unless the
//   document is *cross-origin isolated*, which is the browser's name for
//   "nothing in this agent cluster can be reached by an origin that did not opt
//   in". Two response headers earn it, and both are required:
//
//     Cross-Origin-Opener-Policy: same-origin    severs the window from any
//                                                cross-origin opener, so a
//                                                document that could script
//                                                this one cannot exist
//     Cross-Origin-Embedder-Policy: require-corp every subresource must opt in
//                                                (CORP or CORS); a plain
//                                                cross-origin image is blocked
//
//   `docs/plan/21-jobs.md` finding 3 records the consequence: GitHub Pages
//   cannot set either, so the published demos are *not* isolated and the worker
//   backend has to degrade there. Locally they are set here, which is what
//   makes the threaded path testable at all.
//
// WHAT `require-corp` COSTS
//   Nothing today — the site loads only same-origin assets, and same-origin
//   subresources need no CORP header. It would cost the moment a demo pulled in
//   a font, a script or an image from another origin: that request is blocked
//   outright, not warned about. A new cross-origin asset therefore has to
//   arrive with `Cross-Origin-Resource-Policy: cross-origin` on it, or the page
//   loses isolation to keep the asset.

import { createReadStream, statSync } from 'node:fs';
import { createServer } from 'node:http';
import { join, normalize, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

/**
 * The pair that buys `SharedArrayBuffer`. Removing either one makes
 * `crossOriginIsolated` false, which the browser e2e asserts against.
 */
export const ISOLATION_HEADERS = {
  'cross-origin-opener-policy': 'same-origin',
  'cross-origin-embedder-policy': 'require-corp',
};

/** Content types the site actually ships. */
export const MIME = {
  '.css': 'text/css; charset=utf-8',
  '.html': 'text/html; charset=utf-8',
  '.js': 'text/javascript; charset=utf-8',
  '.json': 'application/json; charset=utf-8',
  '.mjs': 'text/javascript; charset=utf-8',
  '.png': 'image/png',
  '.svg': 'image/svg+xml',
  '.wasm': 'application/wasm',
};

/**
 * Serves `root` over HTTP on localhost, cross-origin isolated.
 *
 * `routes` maps an exact path to `{ contentType, body }` for pages that are
 * generated rather than read from disk — the e2e's readback control page is the
 * only one. It is served through here rather than from a `data:` URL because an
 * opaque origin is not a secure context, and WebGPU insists on one.
 *
 * `isolated` is the one thing a caller can turn off. `false` withholds the
 * COOP/COEP pair, which is what GitHub Pages does and therefore what a visitor
 * to the published site gets; see the file header.
 *
 * @param {string} root Directory to serve. Escapes out of it are refused.
 * @param {{ port?: number, host?: string, isolated?: boolean,
 *           routes?: Record<string, { contentType: string, body: string }> }} [options]
 * @returns {Promise<{ origin: string, port: number, misses: string[],
 *                     isolated: boolean, close: () => Promise<void> }>}
 */
export function serve(root, options = {}) {
  const {
    port = 0,
    host = '127.0.0.1',
    routes = {},
    isolated = true,
  } = options;
  const base = resolve(root);
  /** What every response carries. Empty when this origin is not isolated. */
  const isolation = isolated ? ISOLATION_HEADERS : {};
  /** Requests that 404'd. A missing asset is a shim bug, not a warning. */
  const misses = [];
  const server = createServer((request, response) => {
    // Only the path; a query string is not part of a file name.
    const path = decodeURIComponent(
      new URL(request.url, 'http://localhost').pathname
    );
    const route = routes[path] ?? routes[path.replace(/\/$/, '')];
    if (route) {
      response.writeHead(200, {
        ...isolation,
        'content-type': route.contentType,
        'cache-control': 'no-store',
      });
      response.end(route.body);
      return;
    }
    // `normalize` collapses `..` before the prefix test, so a request for
    // `/../../etc/passwd` cannot escape the site directory.
    const target = normalize(
      join(base, path.endsWith('/') ? `${path}index.html` : path)
    );
    if (!target.startsWith(base)) {
      response.writeHead(403).end('outside the site');
      return;
    }
    let info;
    try {
      info = statSync(target);
    } catch {
      misses.push(path);
      response.writeHead(404).end('not found');
      return;
    }
    if (info.isDirectory()) {
      response.writeHead(301, { location: `${path}/` }).end();
      return;
    }
    const extension = target.slice(target.lastIndexOf('.'));
    response.writeHead(200, {
      // Isolation is a property of the *document*, so it is the HTML response
      // that has to carry these. Sending them on everything costs two headers
      // per asset and removes the question of which responses are documents.
      ...isolation,
      'content-type': MIME[extension] ?? 'application/octet-stream',
      'content-length': info.size,
      // A stale artifact served to a fresh browser is a debugging session
      // nobody enjoys, and this server lives for one run.
      'cache-control': 'no-store',
    });
    createReadStream(target).pipe(response);
  });
  return new Promise((ok, no) => {
    server.on('error', no);
    // Port 0 by default: the OS picks a free one. A hard-coded port turns two
    // harnesses on one machine into a flake, and this repository treats a flake
    // as a bug. `--serve` asks for 8000 because a human needs to type it.
    server.listen(port, host, () => {
      const address = /** @type {import('node:net').AddressInfo} */ (
        server.address()
      );
      ok({
        origin: `http://localhost:${address.port}`,
        port: address.port,
        misses,
        isolated,
        // **`close()` alone can hang forever, and did.** `server.close(cb)`
        // stops the server accepting new connections and then waits for every
        // existing one to end before calling back — so a browser holding a
        // keep-alive socket to this server keeps the promise pending, and a
        // caller that awaits it before killing the browser deadlocks. That is
        // exactly what stalled the Windows leg of the render-harness gate for
        // three hours a run: all eleven scenes had rendered, and the process
        // simply never exited. `closeAllConnections()` ends the sockets first,
        // so this resolves whatever the caller's teardown order is.
        close: () =>
          new Promise((done) => {
            server.close(() => done(undefined));
            server.closeAllConnections();
          }),
      });
    });
  });
}

// Run as a command: `node web/tools/serve.mjs <root> [--port N] [--no-isolation]`.
if (
  process.argv[1] &&
  resolve(process.argv[1]) === fileURLToPath(import.meta.url)
) {
  const argv = process.argv.slice(2);
  const root = argv.find((a) => !a.startsWith('--'));
  if (!root) {
    console.error('usage: serve.mjs <root> [--port 8000] [--no-isolation]');
    process.exit(2);
  }
  const flag = argv.indexOf('--port');
  const port = flag === -1 ? 8000 : Number(argv[flag + 1]);
  const isolated = !argv.includes('--no-isolation');
  // Loopback only. `python3 -m http.server`, which this replaced, binds every
  // interface — but `http://<lan-ip>:8000` is not a secure context, so the one
  // thing this server exists to provide would be silently absent there while
  // the page still loaded.
  const site = await serve(root, { port, host: '127.0.0.1', isolated });
  console.log(`serving ${resolve(root)} at ${site.origin}/`);
  console.log(
    site.isolated
      ? `cross-origin isolated: ${Object.entries(ISOLATION_HEADERS)
          .map(([name, value]) => `${name}: ${value}`)
          .join(', ')}`
      : 'not cross-origin isolated: neither COOP nor COEP is sent, which is ' +
          'the shape of the published origin'
  );
}
