/*!
Example code from the README.md.
*/
use std::error::Error;

fn example() -> Result<(), Box<dyn Error>> {
    use regex_chunker::ByteChunker;
    use std::collections::BTreeMap;

    let mut counts: BTreeMap<String, usize> = BTreeMap::new();

    let stdin = std::io::stdin();
    let chunker = ByteChunker::new(stdin, r#"[ "\r\n.,!?:;/]+"#)?;

    for chunk in chunker {
        let word = String::from_utf8_lossy(&chunk?).to_lowercase();
        *counts.entry(word).or_default() += 1;
    }

    println!("{:#?}", &counts);
    Ok(())
}

fn main() -> Result<(), Box<dyn Error>> {
    example()
}
