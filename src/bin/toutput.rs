#![allow(dead_code)]
/**!
Experimenting with code and generating output for tests, doc tests.
*/
use std::error::Error;

use regex_chunker::RcErr;

fn example() -> Result<(), Box<dyn Error>> {
    use regex_chunker::{ByteChunker, MatchDisposition};
    use std::io::Cursor;

    let text = b"One, two, three, four. Can I have a little more?";
    let c = Cursor::new(text);

    let chunks: Vec<String> = ByteChunker::new(c, "[ .,?]+")?
        .with_match(MatchDisposition::Append)
        .map(|res| {
            let v = res.unwrap();
            String::from_utf8(v).unwrap()
        })
        .collect();

    println!("{:?}", &chunks);

    assert_eq!(
        &chunks,
        &["One, ", "two, ", "three, ", "four. ", "Can ", "I ", "have ", "a ", "little ", "more?"]
            .clone()
    );

    Ok(())
}

fn adapter_example() -> Result<(), Box<dyn Error>> {
    use regex_chunker::{Adapter, ByteChunker};
    use std::io::Cursor;

    struct LineCounter {
        lines: usize,
    }

    impl Adapter for LineCounter {
        type Item = Result<Vec<u8>, RcErr>;

        fn adapt(&mut self, v: Option<Result<Vec<u8>, RcErr>>) -> Option<Self::Item> {
            match v {
                Some(Ok(v)) => {
                    self.lines += 1;
                    Some(Ok(v))
                }
                x => x,
            }
        }
    }

    let text = br#"What's he that wishes so?
My cousin Westmoreland? No, my fair cousin:
If we are mark'd to die, we are enow
To do our country loss; and if to live,
The fewer men, the greater share of honour."#;

    let c = Cursor::new(text);

    let mut chunker = ByteChunker::new(c, r#"\r?\n"#)?.with_adapter(LineCounter { lines: 0 });

    let _: Vec<String> = (&mut chunker)
        .map(|res| {
            let v: Vec<u8> = res.unwrap();
            String::from_utf8(v).unwrap()
        })
        .collect();

    println!("{}", &chunker.get_adapter().lines);

    Ok(())
}

fn string_adapter() -> Result<(), Box<dyn Error>> {
    use regex_chunker::{ByteChunker, StringAdapter};
    use std::io::Cursor;

    let text = b"One, two, three four. Can I have a little more?";
    let c = Cursor::new(text);

    let chunks: Vec<_> = ByteChunker::new(c, "[ .,?]+")?
        .with_adapter(StringAdapter::default())
        .map(|res| res.unwrap())
        .collect();

    assert_eq!(
        &chunks,
        &["One", "two", "three", "four", "Can", "I", "have", "a", "little", "more"].clone()
    );
    Ok(())
}

fn simple_string() -> Result<(), Box<dyn Error>> {
    use regex_chunker::{ByteChunker, SimpleAdapter};
    use std::io::Cursor;

    struct LossyStringAdapter {}

    impl SimpleAdapter for LossyStringAdapter {
        type Item = String;

        fn adapt(&mut self, v: Vec<u8>) -> Self::Item {
            String::from_utf8_lossy(&v).into()
        }
    }

    let text = b"One, two, three four. Can I have a little more?";
    let c = Cursor::new(text);

    let chunks: Vec<_> = ByteChunker::new(c, "[ .,?]+")?
        .with_simple_adapter(LossyStringAdapter{})
        .map(|res| res.unwrap())
        .collect();

    assert_eq!(
        &chunks,
        &["One", "two", "three", "four", "Can", "I", "have", "a", "little", "more"].clone()
    );
    Ok(())
}

fn main() -> Result<(), Box<dyn Error>> {
    example()?;
    adapter_example()?;
    simple_string()?;

    Ok(())
}
