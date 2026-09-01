# PhotoSite

A photo manager and editor built around one idea: nothing is written to your
photographs until you say so. Browsing, rating, organising and editing all
happen against a non-destructive recipe; the original file is only touched by
an explicit save, export or batch run.

The repository holds two implementations of it.

## [v2/](v2/README.md) — where the work is

Rust, egui over wgpu, Windows / macOS / Linux from one source.

```bash
cd v2
cargo run --release -p photosite-ui              # the application
cargo test --workspace                           # no window, no GPU
```

Everything v1 did **outside the editor** it does too: browsing, culling,
organising, filtering, writing metadata into the files, comparing, file
operations, RAW, faces and people, expressions, batch conversion, and
descriptions from a vision model on the same machine. The editor is what is
left.

## [v1/](v1/README.md) — frozen, kept for reference

C# and WPF, Windows only. Feature-complete: manager, editor, batch, import,
face recognition, AI descriptions. It is **no longer maintained** and no
longer released; the tag `v1-final` marks the last state.

It stays in the tree on purpose. Porting means answering "what did this
actually do, and why was it done that way" a few hundred times, and the
answer is usually in the source rather than in any document.

```bash
git show v1-final                                # the archive point
```

## Shared

[`CONTEXT.md`](CONTEXT.md) is the vocabulary both versions speak. The type
names in it are still v1's; the concepts are not, and each one gets its v2
name as that part is ported.

[`spikes/`](spikes/README.md) holds throwaway code kept only for the numbers
it produced — including the thumbnail-grid benchmark that decided v2 would be
drawn over the GPU rather than out of widgets.
