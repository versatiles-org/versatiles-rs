// Smoke-test the published npm package, as a user would install it.
//
// Usage (in an empty directory):
//   npm install @versatiles/versatiles-rs@<version>
//   node <repo>/scripts/smoke-test-npm.mjs <version> <repo>/testdata
//
// Opens a container, reads a tile, converts it, and serves it over HTTP with
// an ETag revalidation. Loading the module at all is the first check: it is
// what fails when the platform package (the native binary) is missing or does
// not match the machine.

import { mkdtempSync, readFileSync, rmSync } from "node:fs";
import { createRequire } from "node:module";
import { tmpdir } from "node:os";
import path from "node:path";
import { pathToFileURL } from "node:url";

const [version, testdata] = process.argv.slice(2);
if (!version || !testdata) {
  console.error("usage: node smoke-test-npm.mjs <version> <testdata-dir>");
  process.exit(2);
}

function check(condition, message) {
  if (!condition) {
    console.error(`FAILED: ${message}`);
    process.exit(1);
  }
  console.log(`ok: ${message}`);
}

// The package as installed in the current directory, where the user's own code would find it.
const packageDir = path.join(
  process.cwd(),
  "node_modules/@versatiles/versatiles-rs",
);
const packageJson = JSON.parse(
  readFileSync(path.join(packageDir, "package.json"), "utf8"),
);
check(
  packageJson.version === version,
  `installed version is ${version} (got ${packageJson.version})`,
);

// Both entry points: `require` gets index.cjs, `import` gets index.js.
const require = createRequire(path.join(process.cwd(), "index.js"));
check(
  typeof require("@versatiles/versatiles-rs").TileSource === "function",
  "the CommonJS entry loads",
);

const { TileServer, TileSource, convert, __napiBindingTarget } = await import(
  pathToFileURL(path.join(packageDir, packageJson.exports["."].import)).href
);
check(
  __napiBindingTarget === "native",
  `the ESM entry loads the native binding on ${process.platform}-${process.arch}`,
);

const source = await TileSource.fromPath(path.join(testdata, "berlin.mbtiles"));
check(source.tileJson().maxzoom === 14, "berlin.mbtiles opens with maxzoom 14");

const tile = await source.getTile(14, 8800, 5374);
check(tile !== null && tile.length > 0, "tile 14/8800/5374 has data");

const work = mkdtempSync(path.join(tmpdir(), "versatiles-smoke-"));
try {
  const output = path.join(work, "out.versatiles");
  await convert(path.join(testdata, "berlin.mbtiles"), output, { maxZoom: 8 });
  const converted = await TileSource.fromPath(output);
  check(
    converted.tileJson().maxzoom === 8,
    "convert() wrote a container with maxzoom 8",
  );

  const server = new TileServer({ ip: "127.0.0.1", port: 0 });
  await server.addTileSource("osm", converted);
  await server.start();
  try {
    const url = `http://127.0.0.1:${server.port}/tiles/osm/8/137/83`;
    const first = await fetch(url);
    check(first.status === 200, `GET ${url} returns 200`);
    const etag = first.headers.get("etag");
    check(Boolean(etag), "the tile carries an ETag");
    await first.arrayBuffer();

    const again = await fetch(url, { headers: { "If-None-Match": etag } });
    check(again.status === 304, "revalidating with the ETag returns 304");
  } finally {
    await server.stop();
  }
} finally {
  rmSync(work, { recursive: true, force: true });
}

console.log(
  `All npm checks passed for ${version} on ${process.platform}-${process.arch}`,
);
