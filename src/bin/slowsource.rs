/*!
Program that produces chunks of bytes slowly in order to test the
async chunkers.
*/

#[cfg(feature = "test")]
use std::{
    error::Error,
    fs::File,
    io::{Read, Write},
    time::Duration,
};
#[cfg(feature = "test")]
use regex_chunker::{ByteChunker, MatchDisposition};
#[cfg(feature = "test")]
const DEFAULT_LO: f64 = 0.0;
#[cfg(feature = "test")]
const DEFAULT_HI: f64 = 1.0;
#[cfg(feature = "test")]
const RE: &str = r#"[ .,;:?!/"()\n\r\t]+"#;

// Generates random durations in a range.
#[cfg(feature = "test")]
struct RanDur {
    low: f64,
    width: f64,
}
#[cfg(feature = "test")]
impl RanDur {
    fn new(lo: f64, hi: f64) -> Self {
        if hi < lo {
            panic!("low end of Duration range must be less than high end");
        }

        Self {
            low: lo,
            width: hi - lo,
        }
    }

    fn get(&self) -> Duration {
        let t = self.low + (fastrand::f64() * self.width);
        Duration::from_secs_f64(t)
    }
}
#[cfg(feature = "test")]
fn getopts() -> Result<(Box<dyn Read>, RanDur), Box<dyn Error>> {
    let args: Vec<String> = std::env::args().collect();
    let src: Box<dyn Read> = match args.get(1).map(|x| x.as_str()) {
        None | Some("-") => Box::new(std::io::stdin()),
        Some(fname) => Box::new(File::open(fname)?),
    };

    if let Some(hi_s) = args.get(3) {
        let hi: f64 = hi_s.parse()?;
        let lo: f64 = args.get(2).unwrap().parse()?;

        return Ok((src, RanDur::new(lo, hi)));
    }

    if let Some(hi_s) = args.get(2) {
        let hi: f64 = hi_s.parse()?;

        return Ok((src, RanDur::new(0.0, hi)));
    }

    Ok((src, RanDur::new(DEFAULT_LO, DEFAULT_HI)))
}

#[cfg(feature = "test")]
fn main() -> Result<(), Box<dyn Error>> {
    let (src, durs) = getopts()?;

    let chunker = ByteChunker::new(src, RE)?.with_match(MatchDisposition::Append);

    for chunk in chunker {
        let chunk = chunk?;
        let mut stdout = std::io::stdout();
        stdout.write_all(&chunk)?;
        stdout.flush()?;
        std::thread::sleep(durs.get());
    }

    Ok(())
}

#[cfg(not(feature = "test"))]
fn main() {
    const MSG: &str =
r#"
The slowsource binary must be built with feature \"test\" enabled.
Try re-running with `cargo build --features test`.
"#;
    println!("{}", MSG);
}