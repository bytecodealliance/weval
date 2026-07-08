import { endianness } from "node:os";
import { fileURLToPath } from "node:url";
import { dirname, join, parse } from "node:path";
import { platform, arch } from "node:process";
import { mkdir, chmod } from "node:fs/promises";
import { existsSync, createWriteStream } from "node:fs";
import { Readable } from "node:stream";
import { pipeline } from "node:stream/promises";

import * as tar from "tar";
import { unzipSync } from "fflate";
import xz from "@napi-rs/lzma/xz";

const __dirname = dirname(fileURLToPath(import.meta.url));

const TAG = "v0.4.1";

async function decompressArchive(buf, assetSuffix, exeDir, exeName) {
  if (assetSuffix === "tar.xz") {
    const tarBuf = await xz.decompress(buf);
    await pipeline(
      Readable.from(Buffer.from(tarBuf)),
      tar.extract({
        cwd: exeDir,
        strip: 1,
        filter: (filePath) => parse(filePath).base === exeName,
      })
    );
  } else {
    const entries = unzipSync(buf);
    const match = Object.entries(entries).find(
      ([name]) => parse(name).base === exeName
    );
    if (!match) {
      console.error(`Could not find ${exeName} inside the downloaded archive`);
      process.exit(1);
    }
    const [, content] = match;
    const exe = join(exeDir, exeName);
    await pipeline(Readable.from(Buffer.from(content)), createWriteStream(exe));
    await chmod(exe, 0o755);
  }
}

async function getWeval() {
  const knownPlatforms = {
    "win32 x64 LE": "x86_64-windows",
    "darwin arm64 LE": "aarch64-macos",
    "darwin x64 LE": "x86_64-macos",
    "linux x64 LE": "x86_64-linux",
    "linux arm64 LE": "aarch64-linux",
  };

  function getPlatformName() {
    let platformKey = `${platform} ${arch} ${endianness()}`;

    if (platformKey in knownPlatforms) {
      return knownPlatforms[platformKey];
    }
    throw new Error(
      `Unsupported platform: "${platformKey}". "weval does not have a precompiled binary for the platform/architecture you are using. You can open an issue on https://github.com/bytecodealliance/weval/issues to request for your platform/architecture to be included."`
    );
  }

  const platformName = getPlatformName();
  const assetSuffix = platform == "win32" ? "zip" : "tar.xz";
  const exeSuffix = platform == "win32" ? ".exe" : "";
  const exeName = `weval${exeSuffix}`;

  const exeDir = join(__dirname, platformName);
  const exe = join(exeDir, exeName);

  // If we already have the executable installed, then return it
  if (existsSync(exe)) {
    return exe;
  }

  await mkdir(exeDir, { recursive: true });
  const downloadUrl = `https://github.com/bytecodealliance/weval/releases/download/${TAG}/weval-${TAG}-${platformName}.${assetSuffix}`;
  let data = await fetch(downloadUrl);
  if (!data.ok) {
    console.error(`Error downloading ${downloadUrl}`);
    process.exit(1);
  }
  const buf = new Uint8Array(await data.arrayBuffer());

  await decompressArchive(buf, assetSuffix, exeDir, exeName);

  return exe;
}

export default getWeval;
