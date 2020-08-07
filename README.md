# Cold Clear JS
JavaScript bindings for the WebAssembly build of the [Cold Clear Tetris bot](https://github.com/MinusKelvin/cold-clear).

## Compiling
Compilation requires [`wasm-pack`](https://rustwasm.github.io/wasm-pack).<br>
Running `wasm-pack build --release -- --features release` will build the project.

## Usage
Cold Clear JS depends on a `worker.js` file served at the root to spawn its web workers. This file must call the module's `_web_worker_entry_point` function with `self` as the only argument.
