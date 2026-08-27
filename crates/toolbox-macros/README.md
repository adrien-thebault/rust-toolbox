# toolbox-macros

`#[derive(Entity)]`.

Proc macros. A separate crate because proc macros must be.

| Module | What it holds |
|---|---|
| `entity` | the `Entity` derive: parsing its attribute, and generating the methods |

Each macro gets its own directory under `src/`, with `parse.rs` and `expand.rs`
inside it, so adding a second macro is a new directory rather than a rename.

The error messages are the product. Every misuse has a `trybuild` case with a
committed `.stderr`, and the reason this is a proc macro rather than a
`macro_rules!` is that it can point the caret at the offending token.
