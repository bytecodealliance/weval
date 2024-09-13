# weval

> Prebuilt weval binaries available via npm

See the [weval repository](https://github.com/bytecodealliance/weval) for more details.

## API

```
$ npm install --save @bytecodealliance/weval
```

```js
const execFile = require('child_process').execFile;
const weval = require('@bytecodealliance/weval');

execFile(weval, ['-w', '-i', 'snapshot.wasm', '-o', 'wevaled.wasm'], (err, stdout) => {
	console.log(stdout);
});
```
