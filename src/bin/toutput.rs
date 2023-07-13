/**!
Generating output for tests and documentation.
*/
use std::{
    error::Error,
    io::Cursor,
};

use regex_chunker::{ByteChunker, MatchDisposition};

fn example() -> Result<(), Box<dyn Error>> {
    let text = b"One, two, three, four. Can I have a little more?";
    let c = Cursor::new(text);

    let chunks: Vec<String> = ByteChunker::new(c, "[ .,?]+")?
        .with_match(MatchDisposition::Append)
        .map(|res| {
            let v = res.unwrap();
            String::from_utf8(v).unwrap()
        }).collect();
    
    println!("{:?}", &chunks);

    assert_eq!(
        &chunks,
        &["One, ", "two, ", "three, ", "four. ",
        "Can ", "I ", "have ", "a ", "little ", "more?"].clone()
    );

    Ok(())
}

fn main() -> Result<(), Box<dyn Error>> {
    example()?;

    Ok(())
}