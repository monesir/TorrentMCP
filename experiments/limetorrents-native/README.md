# Frozen LimeTorrents comparison build

This snapshot contains only the search path used to compare the Python and native
Rust LimeTorrents adapters. It excludes concurrently developed analysis modules.
The adapter is also integrated into the main project. Do not use this snapshot as
the starting point for new product features.

From the repository root:

```powershell
cargo test --manifest-path experiments/limetorrents-native/Cargo.toml --target-dir .cache/limetorrents-experiment-target --locked
cargo build --manifest-path experiments/limetorrents-native/Cargo.toml --target-dir .cache/limetorrents-experiment-target --release --locked
```

The measured executable is `.cache/limetorrents-experiment-target/release/torrent-search.exe`.
Use `--assets experiments/limetorrents-native/resources` and
`--python .cache/python-3.14.8-embed-amd64/python.exe` when running it from the root.
Select `--limetorrents-backend rust` or `python` explicitly. The Python selection
needs that runtime; the native adapter does not.

Measurements, limitations, and reproduction commands are in
[`verification/limetorrents-benchmark/README.md`](../../verification/limetorrents-benchmark/README.md).
