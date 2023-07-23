# regex-chunker
Splitting output from `Read` types with regular expressions.

The chief type in this crate is the
[`ByteChunker`](https://docs.rs/regex_chunker/struct.ByteChunker.html),
which wraps a type that implements
[`Read`](https://doc.rust-lang.org/stable/std/io/trait.Read.html)
and iterates over chunks of its byte stream delimited by a supplied
regular expression. The following example reads from the standard input
and prints word counts:

```rust
use std::collections::BTreeMap;
use regex_chunker::ByteChunker;
  
fn main() -> Result<(), Box<dyn Error>> {
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    let stdin = std::io::stdin();
    
    // The regex is a stab at something matching strings of
    // "between-word" characters in general English text.
    let chunker = ByteChunker::new(stdin, r#"[ "\r\n.,!?:;/]+"#)?;
    for chunk in chunker {
        let word = String::from_utf8_lossy(&chunk?).to_lowercase();
        *counts.entry(word).or_default() += 1;
    }

    println!("{:#?}", &counts);
    Ok(())
}
```

The `async` feature enables the `stream` submodule, which contains an
asynchronous version of `ByteChunker` that wraps an
[`tokio::io::AsyncRead`](https://docs.rs/tokio/latest/tokio/io/trait.AsyncRead.html)
type and produces a
[`Stream`](https://docs.rs/futures-core/0.3.28/futures_core/stream/trait.Stream.html)
of byte chunks.

## Running The Tests

If you want to run the tests for the `async` features, you need to first
build `src/bin/slowsource.rs` with the `async` and `test` feature enabled:

```sh
$ cargo build --bin slowsource --all-features
```

Some of the [`stream`] module tests run it in a subprocess and use it as
a source of bytes.

## Unanswered Questions and Stuff To do

This is, as of yet, an essentially naive implementation. What can be done
to optimize performance?

Is there room to tighten up the `RcErr` type?

When non-overlapping blanket impls
([1672](https://github.com/rust-lang/rfcs/pull/1672),
maybe [20400](https://github.com/rust-lang/rust/issues/20400)) land, remove both the
`SimpleCustomChunker` types.