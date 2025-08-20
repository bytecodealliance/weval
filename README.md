<div align="center">
  <h1>weval</h1>

  <p>
    <strong>weval Wasm partial evaluator</strong>
  </p>

  <strong>A <a href="https://bytecodealliance.org/">Bytecode Alliance</a> project</strong>

  <p>
    <a href="https://github.com/bytecodealliance/weval/actions?query=workflow%3ACI"><img src="https://github.com/bytecodealliance/weval/workflows/CI/badge.svg" alt="build status" /></a>
    <a href="https://bytecodealliance.zulipchat.com/#narrow/stream/223391-wasm"><img src="https://img.shields.io/badge/zulip-join_chat-brightgreen.svg" alt="zulip chat" /></a>
    <a href="https://docs.rs/weval"><img src="https://docs.rs/weval/badge.svg" alt="Documentation Status" /></a>
  </p>

  <h3>
    <a href="https://github.com/bytecodealliance/weval/blob/main/CONTRIBUTING.md">Contributing</a>
    <span> | </span>
    <a href="https://bytecodealliance.zulipchat.com/#narrow/stream/223391-wasm">Chat</a>
  </h3>
</div>

`weval` partially evaluates WebAssembly snapshots to turn interpreters into
compilers (see [Futamura
projection](https://en.wikipedia.org/wiki/Partial_evaluation#Futamura_projections)
for more).

`weval` binaries are available via releases on this repo or via an [npm
package](https://www.npmjs.com/package/@cfallin/weval).

Usage of weval is like:

```
$ weval weval -w -i program.wasm -o wevaled.wasm
```

which runs Wizer on `program.wasm` to obtain a snapshot, then processes any
weval requests (function specialization requests) in the resulting heap image,
appending the specialized functions and filling in function pointers in
`wevaled.wasm`.

See the API in `include/weval.h` for more.

### Releasing Checklist

- Bump the version in `Cargo.toml` and `cargo check` to ensure `Cargo.lock` is
  updated as well.
- Bump the tag version (`TAG` constant) in `npm/weval/index.js`.
- Bump the npm package version in `npm/weval/package.json`.
- Run `npm i` in `npm/weval/` to ensure the `package-lock.json` file is
  updated.

- Commit all of this as a "version bump" PR.
- Push it to `main` and ensure CI completes successfully.
- Tag as `v0.x.y` and push that tag.
- `cargo publish` from the root.
- `npm publish` from `npm/weval/`.

### Further Details

The theory behind weval is described in the author's blog post
[here](https://cfallin.org/blog/2024/08/28/weval/), covering partial evaluation
and Futamura projections as well as how weval's main transform works.

### Uses

weval is in use to provide ahead-of-time compilation of JavaScript by wevaling
a build of the [SpiderMonkey](https://spidermonkey.dev) interpreter, providing
3-5x speedups over the generic interpreter. Please let us know if you use it
elsewhere!
