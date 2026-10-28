//! Writes the index the plugin carries, from a `queryall` answer, with the plugin's own reader:
//!
//!     curl -s 'https://catalogopmb.catastrobogota.gov.co/PMBWeb/web/servicios?cmd=queryall' \
//!       | cargo run -q -p ide-bogota --example index -- 2026-10-25 > ide-bogota/index.json
//!
//! The first argument is the date the index is of; the panel shows it. Given the index it
//! replaces as a second argument, it also tells on its last line how they differ: `unchanged`,
//! `changed: N new, M withdrawn` or `incomplete` (the CI reads it, and proposes only a change).

use std::io::Read;

use ide_core::index::{self, Freshness};
use rustgis_plugin::{Value, serde_json};

fn main() -> Result<(), String> {
    let mut args = std::env::args().skip(1);
    let date = args.next().ok_or("usage: index <YYYY-MM-DD> [previous index.json] < queryall.json")?;
    let previous = args.next();
    let mut text = String::new();
    std::io::stdin().read_to_string(&mut text).map_err(|e| e.to_string())?;
    let doc: Value = serde_json::from_str(&text).map_err(|e| format!("not JSON: {e}"))?;
    let entries = ide_bogota::ideca::parse(&doc);
    if entries.is_empty() {
        return Err("no services: is it a queryall answer?".to_string());
    }
    println!("{}", index::write(&date, &entries));
    eprintln!("{} services", entries.len());
    if let Some(path) = previous {
        let old = std::fs::read_to_string(&path).map_err(|e| format!("{path}: {e}"))?;
        let (_, old) = index::read(&serde_json::from_str(&old).unwrap_or(Value::Null));
        // The same services can differ in what is said of them (a new licence, a new name).
        let summary = match index::freshness(&old, &entries) {
            Freshness::UpToDate if old == entries => "unchanged".to_string(),
            Freshness::UpToDate => "changed: details only".to_string(),
            Freshness::Changed { added, removed } => format!("changed: {added} new, {removed} withdrawn"),
            Freshness::Incomplete => "incomplete".to_string(),
        };
        eprintln!("{summary}");
    }
    Ok(())
}
