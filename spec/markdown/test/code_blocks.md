# Code blocks

- Hover a block: copy button at the top right; copying puts the code (without
  fences) on the clipboard.
- Highlighting uses the language after the fence.

```rust
#[derive(Debug)]
struct Point { x: f32, y: f32 }

fn main() {
    let p = Point { x: 1.0, y: 2.0 };
    println!("{p:?}");
}
```

```python
def greet(name: str) -> str:
    return f"hello {name}"
```

```json
{ "theme": "One Dark", "buffer_font_size": 14 }
```

```
untagged block (no language)
```

    indented code block

A very long line to check horizontal scrolling:

```text
0123456789 0123456789 0123456789 0123456789 0123456789 0123456789 0123456789 0123456789 0123456789 0123456789 0123456789 0123456789 end
```

Hover popovers (e.g. hover a symbol in a Rust file) should **not** show a copy
button on their code blocks.
