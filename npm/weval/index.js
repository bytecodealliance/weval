import { endianness } from "node:os";
import { fileURLToPath } from "node:url";
import { dirname, join, parse } from "node:path";
import { platform, arch } from "node:process";
import { mkdir } from "node:fs/promises";
import { existsSync } from "node:fs";

import decompress from "decompress";
import decompressUnzip from "decompress-unzip";
import decompressTar from "decompress-tar";
import xz from "@napi-rs/lzma/xz";

const __dirname = dirname(fileURLToPath(import.meta.url));

const TAG = "v0.3.2";

/**
 * Download Weval from GitHub releases
 *
 * @param {object} [opts]
 * @param {string} [opts.downloadDir] - Directory to which the binary should be downloaded
 * @returns {string} path to the downloaded binary on disk
 */
export async function getWeval(opts) {
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

  const exeDir = join(opts && opts.downloadDir ? opts.downloadDir : __dirname, platformName);
  const exe = join(exeDir, `weval${exeSuffix}`);

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
  let buf = await data.arrayBuffer();

  if (downloadUrl.endsWith(".xz")) {
    buf = await xz.decompress(new Uint8Array(buf));
  }
  await decompress(Buffer.from(buf), exeDir, {
    // Remove the leading directory from the extracted file.
    strip: 1,
    plugins: [decompressUnzip(), decompressTar()],
    // Only extract the binary file and nothing else
    filter: (file) => parse(file.path).base === `weval${exeSuffix}`,
  });

  return exe;
}

export default getWeval;
