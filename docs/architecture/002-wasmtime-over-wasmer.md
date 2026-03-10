# ADR-002: wasmtime over Wasmer

## Status
Accepted

## Context
HyprDrive's extension system uses WebAssembly to run sandboxed plugins. The two major Rust WASM runtimes are **wasmtime** (Bytecode Alliance / Mozilla) and **Wasmer** (Wasmer Inc).

Spacedrive chose Wasmer. We evaluated both for HyprDrive.

## Decision
Use **wasmtime** as the sole WASM runtime.

## Consequences

### Positive
- **10× less memory per extension**: wasmtime uses ~10 MB per instance vs. Wasmer's ~100 MB. With 7 built-in extensions, this is 70 MB vs. 700 MB.
- **40× faster cold start**: wasmtime supports AOT (Ahead-of-Time) compilation. Pre-compiled modules load in ~1ms vs. ~40ms for JIT.
- **Epoch-based interruption**: wasmtime can kill runaway extensions after N instructions without polling. Wasmer requires cooperative yielding.
- **Bytecode Alliance backing**: wasmtime is maintained by Mozilla, Fastly, Intel, and others. Larger contributor base and more security audits.
- **WASI-P2 first**: wasmtime leads WASI preview 2 implementation, which is the future of portable WASM.

### Negative
- **Smaller ecosystem**: Wasmer has more third-party packages and language bindings.
- **Less flexible JIT**: wasmtime's JIT is less configurable than Wasmer's pluggable compiler backends.

### Neutral
- Both support the same core WASM spec. Extensions written for one can run on the other with minimal changes.

## References
- [wasmtime GitHub](https://github.com/bytecodealliance/wasmtime)
- [wasmtime vs Wasmer benchmarks](https://00f.net/2023/01/04/webassembly-benchmark-2023/)
