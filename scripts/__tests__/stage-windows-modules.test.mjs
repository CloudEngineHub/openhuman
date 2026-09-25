import assert from "node:assert/strict";
import { test } from "node:test";

import { readRegistrySource } from "../ci/self-hosted/test-module-assets.mjs";
import { parseAllList } from "../lib/module-pins.mjs";
import { bundledAssets } from "../release/stage-windows-modules.mjs";

test("every compiled module has one pinned Windows installer asset", () => {
  const source = readRegistrySource();
  const assets = bundledAssets(source);
  assert.equal(assets.length, parseAllList(source).length);
  assert.equal(new Set(assets.map((asset) => asset.id)).size, assets.length);
  for (const asset of assets) {
    assert.equal(asset.hostKey, "windows-2022-x86_64");
    assert.match(asset.url, /^https:\/\/github\.com\/tinyhumansai\//);
    assert.match(asset.archive, /-windows-2022-x86_64\.zip$/);
    assert.match(asset.sha256, /^[a-f0-9]{64}$/);
  }
});

test("a missing Windows pin fails staging before any download", () => {
  const source = readRegistrySource().replace(
    'host_key: "windows-2022-x86_64"',
    'host_key: "removed-windows-asset"',
  );
  assert.throws(() => bundledAssets(source), /has no windows-2022-x86_64 asset/);
});
