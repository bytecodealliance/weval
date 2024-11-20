import assert from "node:assert";
import { test } from "node:test";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { mkdtemp, access } from 'node:fs/promises';

import { getWeval } from "../index.js";

export default async function tests() {
  test("downloading works", async () => {
    const downloadDir = await mkdtemp(join(tmpdir(), "weval-dl-"));
    const wevalPath = await getWeval({ downloadDir });
    assert(wevalPath);
    await access(wevalPath);
    console.log(`weval path: ${wevalPath}`);
  });
}
