# PhotoSite

PhotoManager for real life. Based on ideas of Zoner Photo Studio v15 (not its newer versions) in mixup with modern features like AI image reading and describing or ML face recognition.

Rust, egui over wgpu, Windows / macOS / Linux from one source. The
application lives in [v2/](v2/README.md), which is also where the design is
written down.

```bash
cd v2
cargo run --release -p photosite-ui              # the application
cargo test --workspace                           # no window, no GPU
```

Browsing, culling, organising, filtering, writing metadata into the files,
comparing, file operations, RAW, faces and people, expressions, batch
conversion, and descriptions from a vision model on the same machine. The
editor is what is left to build.

An earlier implementation in C# and WPF was retired once this one had
caught up with it outside the editor; its last state is the tag
`v1-final`, and nothing in the tree depends on it any more.

## Shared

[`CONTEXT.md`](CONTEXT.md) is the vocabulary the code speaks: the terms,
the synonyms deliberately not used, and the invariants.

[`spikes/`](spikes/README.md) holds throwaway code kept only for the numbers
it produced — including the thumbnail-grid benchmark that decided the
application would be drawn over the GPU rather than out of widgets.
