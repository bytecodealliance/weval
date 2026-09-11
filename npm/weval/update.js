#!/usr/bin/env node

import { fileURLToPath } from 'node:url';
import { dirname, join } from 'node:path';
import { readFile, writeFile } from 'node:fs/promises';

const __dirname = dirname(fileURLToPath(import.meta.url));
const tag = process.argv.slice(2).at(0)?.trim() ?? 'dev';
const version = tag.startsWith('v') ? tag.slice(1) : tag;

const pjsonPath = join(__dirname, 'package.json');
const pjson = JSON.parse(await readFile(pjsonPath, 'utf8'));
pjson.version = version;
await writeFile(pjsonPath, JSON.stringify(pjson, null, 2) + '\n');

const indexPath = join(__dirname, 'index.js');
let index = await readFile(indexPath, 'utf8');
index = index.replace(/^const TAG = ".*";$/m, `const TAG = "${tag}";`);
await writeFile(indexPath, index);
