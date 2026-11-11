//! The program behind the resolver of the extension. FetchBox runs it when the user pastes an
//! address that matches `match` in plugin.json:
//!
//!     __ID__ <address> <most files>
//!
//! It prints JSON on standard output, which `result` in plugin.json reads (here
//! `{"entries": [{"name": ..., "url": ...}]}`), and exits with 0. A non-zero exit code, with the
//! reason on standard error, is shown to the user as the reason the address could not be read.
//!
//! This example needs no network: it turns an address with a numbered range, such as
//! `https://example.com/img[001-020].jpg`, into one entry per number. Replace `expand` with
//! whatever your source needs (reading a page, calling an API); the engine of FetchBox downloads
//! each `url` it is given.

use serde_json::{json, Value};

/// One file to offer.
#[derive(Debug, PartialEq, Eq)]
struct Entry {
    name: String,
    url: String,
}

/// `https://host/img[001-003].jpg` becomes `img001.jpg`, `img002.jpg`, `img003.jpg`. The numbers
/// are as wide as the first one is written (`01-10` gives `01`, `02`, ..., `10`). At most `limit`
/// entries are made.
fn expand(address: &str, limit: usize) -> Result<Vec<Entry>, String> {
    let (open, close) = address
        .find('[')
        .and_then(|open| address[open..].find(']').map(|close| (open, open + close)))
        .ok_or("the address has no range like [001-020]")?;
    let range = &address[open + 1..close];
    let (from, to) = range
        .split_once('-')
        .ok_or_else(|| format!("\"{range}\" is not a range like 001-020"))?;
    let number = |text: &str| {
        (!text.is_empty() && text.bytes().all(|b| b.is_ascii_digit()))
            .then(|| text.parse::<u64>().ok())
            .flatten()
            .ok_or_else(|| format!("\"{text}\" is not a number"))
    };
    let (first, last) = (number(from)?, number(to)?);
    if last < first {
        return Err(format!("the range {range} ends before it starts"));
    }
    let width = from.len();
    let (before, after) = (&address[..open], &address[close + 1..]);

    let mut entries = Vec::new();
    for n in first..=last {
        if entries.len() >= limit {
            break;
        }
        let url = format!("{before}{n:0width$}{after}");
        let name = url
            .split(['?', '#'])
            .next()
            .and_then(|path| path.rsplit('/').next())
            .filter(|name| !name.is_empty())
            .unwrap_or("file")
            .to_owned();
        entries.push(Entry { name, url });
    }
    Ok(entries)
}

fn run(args: &[String]) -> Result<Value, String> {
    let address = args
        .first()
        .ok_or("usage: <program> <address> <most files>")?;
    let limit = args
        .get(1)
        .and_then(|limit| limit.parse::<usize>().ok())
        .unwrap_or(100);
    let entries = expand(address, limit)?;
    Ok(json!({
        "entries": entries
            .iter()
            .map(|e| json!({ "name": e.name, "url": e.url }))
            .collect::<Vec<_>>()
    }))
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match run(&args) {
        Ok(json) => println!("{json}"),
        Err(reason) => {
            eprintln!("{reason}");
            std::process::exit(1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn urls(entries: &[Entry]) -> Vec<&str> {
        entries.iter().map(|e| e.url.as_str()).collect()
    }

    #[test]
    fn a_range_is_expanded_with_the_width_of_its_start() {
        let entries = expand("https://example.com/img[001-003].jpg", 100).expect("expanded");
        assert_eq!(
            urls(&entries),
            [
                "https://example.com/img001.jpg",
                "https://example.com/img002.jpg",
                "https://example.com/img003.jpg"
            ]
        );
        assert_eq!(entries[0].name, "img001.jpg");

        let entries = expand("https://example.com/p[8-10]", 100).expect("expanded");
        assert_eq!(
            urls(&entries),
            [
                "https://example.com/p8",
                "https://example.com/p9",
                "https://example.com/p10"
            ]
        );
    }

    #[test]
    fn the_limit_cuts_a_long_range_short() {
        let entries = expand("https://example.com/[1-1000].png", 5).expect("expanded");
        assert_eq!(entries.len(), 5);
    }

    #[test]
    fn addresses_that_are_not_ranges_say_why() {
        for (address, reason) in [
            ("https://example.com/file.zip", "no range"),
            ("https://example.com/[a-b].zip", "not a number"),
            ("https://example.com/[5].zip", "not a range"),
            ("https://example.com/[9-1].zip", "ends before"),
        ] {
            let err = expand(address, 10).expect_err(address);
            assert!(err.contains(reason), "{address}: {err}");
        }
    }

    #[test]
    fn the_output_is_what_plugin_json_reads() {
        let out = run(&["https://example.com/a[1-2].txt".into(), "10".into()]).expect("json");
        assert_eq!(out["entries"][1]["url"], "https://example.com/a2.txt");
        assert_eq!(out["entries"][1]["name"], "a2.txt");
    }
}
