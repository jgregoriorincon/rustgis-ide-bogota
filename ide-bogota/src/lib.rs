//! IDE Bogotá: IDECA's catalogue in a panel beside the map.
//!
//! The panel walks the index this version carries (`index.json`, written by `examples/index.rs`
//! with the same reader as the live catalogue), a category at a time. A plugin keeps nothing
//! between calls, so every click reads the list again: the index costs little, while reading
//! the 665 KB catalogue on each click ran out of a command's fuel, and so did reading it once to
//! compare it with the index. The repository's CI does that instead: it rebuilds the index each
//! week and proposes it when the catalogue changed (`.github/workflows/refresh-index.yml`).
//!
//! - `panel-state` runs when the panel opens; `show` when a slider is let go or stepped. The
//!   sliders walk categories, then the category's subcategories, then the subcategory's
//!   services; each label counts the services it holds.
//! - `add` adds an ArcGIS layer's features in the map view, with the catalogue's filter applied
//!   by the server (or a GeoJSON document). `image` adds a MapServer's or ImageServer's map image
//!   as a tile layer. Both act on the sliders' service, so they work from scripts and
//!   `rustgis-cli` too: `plugin:ide-bogota/add={"panel": {"category": 2, "service": 5}}`.
//!
//! In the web app nothing may wait: a fetch not kept yet answers `pending`, the command returns
//! at once, and RustGIS runs it again when the answer lands.

// The entry point exists only in the WebAssembly module. On this computer (the tests) nothing
// calls the handler, so RustGIS's host imports are never linked.
#![cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]

pub mod ideca;

use ide_core::arcgis;
use ide_core::browse::{self, Group, clip};
use ide_core::index;
use ide_core::{Entry, Kind};
use rustgis_plugin::{Panel, Request, Value, host, json, serde_json};

/// `plugin.json`, for its translations.
const MANIFEST: &str = include_str!("../plugin.json");
/// The catalogue as it was when this version was made.
const INDEX: &str = include_str!("../index.json");
/// A slider label's most characters (RustGIS clips them at 80).
const LABEL_MAX: usize = 80;

#[cfg(target_arch = "wasm32")]
rustgis_plugin::export!(handle);

fn handle(req: Request) -> Result<Value, String> {
    let (date, entries) = index::read(&serde_json::from_str(INDEX).unwrap_or(Value::Null));
    let other = req.tr(MANIFEST, "Other", &[]);
    let walk = walk(&req, &entries, &other);
    let chosen = || walk.services.get(walk.at).copied().ok_or_else(|| req.tr(MANIFEST, "The catalogue has no services to show.", &[]));
    let answer = match req.id.as_str() {
        "panel-state" | "show" => Value::Null,
        "add" => add(&req, chosen()?)?,
        "image" => image(&req, chosen()?)?,
        other => return Err(req.tr(MANIFEST, "IDE Bogotá has no command '{id}'.", &[("id", other)])),
    };
    let n = entries.len().to_string();
    let source = if date.is_empty() {
        req.tr(MANIFEST, "Built-in index: {n} services.", &[("n", &n)])
    } else {
        req.tr(MANIFEST, "Index of {date}: {n} services.", &[("date", &date), ("n", &n)])
    };
    Ok(panel(&req, source, &walk).answer(answer))
}

/// Where the sliders stand: each level's groups, and the one chosen at each.
struct Walk<'a> {
    cats: Vec<Group>,
    cat_at: usize,
    subs: Vec<Group>,
    sub_at: usize,
    services: Vec<&'a Entry>,
    at: usize,
}

/// The sliders' positions, kept within their lists. Moving a level starts the levels below it
/// at their first.
fn walk<'a>(req: &Request, entries: &'a [Entry], other: &str) -> Walk<'a> {
    let name = |groups: &[Group], at: usize| groups.get(at).map_or(String::new(), |(n, _)| n.clone());
    let cats = browse::categories(entries, other);
    let cat_at = browse::clamp(req.panel_i64("category"), cats.len());
    let category = name(&cats, cat_at);
    let subs = browse::subcategories(entries, &category, other);
    let sub_at = if req.changed("category") { 0 } else { browse::clamp(req.panel_i64("subcategory"), subs.len()) };
    let services = browse::services(entries, &category, &name(&subs, sub_at), other);
    let at =
        if req.changed("category") || req.changed("subcategory") { 0 } else { browse::clamp(req.panel_i64("service"), services.len()) };
    Walk { cats, cat_at, subs, sub_at, services, at }
}

/// What `host.fetch` gave.
enum Fetched {
    Doc(Value),
    /// The web app is reading it: the command runs again when it lands.
    Pending,
    /// No JSON came: an error, or another kind of document.
    Failed,
}

fn fetch(url: &str) -> Fetched {
    match host::call("host.fetch", json!({"url": url})) {
        Ok(v) if v.get("pending").and_then(Value::as_bool) == Some(true) => Fetched::Pending,
        // Taken, not copied: the module has a fuel budget.
        Ok(mut v) => v.get_mut("json").map(Value::take).map_or(Fetched::Failed, Fetched::Doc),
        Err(_) => Fetched::Failed,
    }
}

/// Adds a service's features: an ArcGIS layer's in the map view, filtered by the server, or a
/// GeoJSON document. In the web app, where a command can't download features, an ArcGIS layer's
/// map image instead.
fn add(req: &Request, e: &Entry) -> Result<Value, String> {
    let name = [("name", e.name.as_str())];
    match e.kind {
        Kind::GeoJson => {
            let out = host::run("layer.addUrl", json!({"url": e.url, "name": e.name}))?;
            host::notify(&req.tr(MANIFEST, "Added {name}.", &name));
            Ok(out)
        }
        Kind::ArcGisLayer => {
            let limit = match fetch(&format!("{}?f=json", e.url)) {
                Fetched::Doc(layer) => arcgis::max_records(&layer),
                Fetched::Pending => return Ok(json!({"pending": true})),
                Fetched::Failed => arcgis::DEFAULT_MAX_RECORDS,
            };
            let view = arcgis::view_of(&host::view()?);
            let url = arcgis::features_url(e, view, limit).ok_or_else(|| not_features(req, e))?;
            match host::run("layer.addUrl", json!({"url": url, "name": e.name, "zoom": false})) {
                Ok(out) => {
                    let features = out.get("features").and_then(Value::as_u64);
                    let n = limit.to_string();
                    let message = if out.get("pending").and_then(Value::as_bool) == Some(true) {
                        req.tr(MANIFEST, "Downloading {name}; it is added when it lands.", &name)
                    } else if features.is_some_and(|f| f >= u64::from(limit)) {
                        req.tr(
                            MANIFEST,
                            "{name}: the map view holds more than the {n} features the service gives at once, so the layer is incomplete. Zoom in and add it again.",
                            &[("name", &e.name), ("n", &n)],
                        )
                    } else {
                        req.tr(MANIFEST, "Added {name} in the map view.", &name)
                    };
                    host::notify(&message);
                    Ok(out)
                }
                Err(err) if err.contains("web app") && arcgis::image_template(e).is_some() => {
                    let out = image(req, e)?;
                    host::notify(&req.tr(
                        MANIFEST,
                        "In the web app, {name} is shown as a map image; its features need the desktop app.",
                        &name,
                    ));
                    Ok(out)
                }
                Err(err) => Err(err),
            }
        }
        _ => Err(not_features(req, e)),
    }
}

/// Adds a MapServer's or ImageServer's map image as a tile layer over the basemap.
fn image(req: &Request, e: &Entry) -> Result<Value, String> {
    let name = [("name", e.name.as_str())];
    let template = arcgis::image_template(e)
        .ok_or_else(|| req.tr(MANIFEST, "{name} has no map image: only ArcGIS MapServer and ImageServer services draw one.", &name))?;
    let mut params = json!({"url": template, "kind": "raster", "name": e.name, "tileSize": 512, "overlay": true, "id": tile_id(e)});
    if let Some(entity) = &e.entity {
        params["attribution"] = json!(entity);
    }
    let out = host::run("map.addTiles", params)?;
    host::notify(&req.tr(MANIFEST, "Added {name} as a map image, over the basemap.", &name));
    Ok(out)
}

/// Why a service can't be added as features.
fn not_features(req: &Request, e: &Entry) -> String {
    match e.kind {
        Kind::Unsupported(reason) => {
            let reason = req.tr(MANIFEST, reason, &[]);
            req.tr(MANIFEST, "Not shown in RustGIS: {reason}", &[("reason", &reason)])
        }
        _ => {
            req.tr(MANIFEST, "{name} can't be added as features: WMS, WFS and WMTS services come in a later version.", &[("name", &e.name)])
        }
    }
}

/// The id of a service's tile layer: RustGIS keeps letters, digits and hyphens of it.
fn tile_id(e: &Entry) -> String {
    let id: String = e.id.chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '-' }).collect();
    format!("ide-bogota-{id}")
}

/// The panel's state: the index it walks, the three sliders, the service's details and which
/// buttons apply to it.
fn panel(req: &Request, source: String, walk: &Walk) -> Panel {
    let labels = |names: Vec<String>| if names.is_empty() { vec!["—".to_string()] } else { names };
    let cat_labels = labels(walk.cats.iter().map(|g| browse::label(g, LABEL_MAX)).collect());
    let sub_labels = labels(walk.subs.iter().map(|g| browse::label(g, LABEL_MAX)).collect());
    let names = labels(walk.services.iter().map(|e| clip(&e.name, LABEL_MAX)).collect());
    let p = Panel::new()
        .text("source", source)
        .value("category", walk.cat_at as i64)
        .slider("category", 0, cat_labels.len() as i64 - 1, &cat_labels)
        .value("subcategory", walk.sub_at as i64)
        .slider("subcategory", 0, sub_labels.len() as i64 - 1, &sub_labels)
        .value("service", walk.at as i64)
        .slider("service", 0, names.len() as i64 - 1, &names);
    match walk.services.get(walk.at) {
        Some(e) => p
            .text("card", card(req, e))
            .enabled("add", matches!(e.kind, Kind::ArcGisLayer | Kind::GeoJson))
            .enabled("image", arcgis::image_template(e).is_some()),
        None => {
            p.text("card", req.tr(MANIFEST, "The catalogue has no services to show.", &[])).enabled("add", false).enabled("image", false)
        }
    }
}

/// A service's details, a line each: what it is, who publishes it, its licence, filter and
/// metadata, and its address.
fn card(req: &Request, e: &Entry) -> String {
    let kind = match e.kind {
        Kind::ArcGisLayer => req.tr(MANIFEST, "ArcGIS layer", &[]),
        Kind::ArcGisService => req.tr(MANIFEST, "ArcGIS map service", &[]),
        Kind::ArcGisImage => req.tr(MANIFEST, "ArcGIS image service", &[]),
        Kind::Wms => "WMS".to_string(),
        Kind::Wfs => "WFS".to_string(),
        Kind::Wmts => "WMTS".to_string(),
        Kind::GeoJson => "GeoJSON".to_string(),
        Kind::Unsupported(_) => not_features(req, e),
    };
    let mut lines = vec![e.name.clone(), kind];
    let theme: Vec<&str> = [e.category.as_deref(), e.subcategory.as_deref()].into_iter().flatten().collect();
    if !theme.is_empty() {
        lines.push(theme.join(" › "));
    }
    lines.extend(e.entity.clone());
    lines.push(match &e.licence {
        Some(l) => req.tr(MANIFEST, "Licence: {licence}", &[("licence", l)]),
        None => req.tr(MANIFEST, "No licence stated: check it before using the data.", &[]),
    });
    if let Some(f) = &e.filter {
        lines.push(req.tr(MANIFEST, "Filter: {filter}", &[("filter", f)]));
    }
    lines.extend(e.description.clone());
    if let Some(m) = &e.metadata_url {
        lines.push(req.tr(MANIFEST, "Metadata: {url}", &[("url", m)]));
    }
    lines.push(e.url.clone());
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use ide_core::entry::Raw;

    fn entry(id: &str, url: &str) -> Entry {
        Entry::new(&Raw { id: Some(id), name: Some("Lote"), url: Some(url), ..Raw::default() }).expect("a valid entry")
    }

    #[test]
    fn the_index_and_manifest_read() {
        let doc: Value = serde_json::from_str(INDEX).expect("index.json is JSON");
        assert!(doc.get("entries").is_some_and(Value::is_array));
        let manifest: Value = serde_json::from_str(MANIFEST).expect("plugin.json is JSON");
        assert_eq!(manifest["id"], "ide-bogota");
    }

    #[test]
    fn tile_ids_keep_letters_digits_and_hyphens() {
        assert_eq!(tile_id(&entry("7956", "https://x/rest/services/a/MapServer/0")), "ide-bogota-7956");
        assert_eq!(tile_id(&entry("{A8/B}", "https://x/rest/services/a/MapServer/0")), "ide-bogota--A8-B-");
    }

    #[test]
    fn every_message_has_its_spanish() {
        let manifest: Value = serde_json::from_str(MANIFEST).expect("plugin.json is JSON");
        let es = manifest["i18n"]["es"].as_object().expect("an es table");
        let source = include_str!("lib.rs");
        // Every literal handed to req.tr in this file.
        let mut missing = Vec::new();
        for part in source.split("req.tr(").skip(1) {
            let Some(rest) = part.trim_start().strip_prefix("MANIFEST,") else { continue };
            let rest = rest.trim_start();
            let Some(text) = rest.strip_prefix('"').and_then(|r| r.split_once('"')).map(|(t, _)| t) else { continue };
            if !es.contains_key(text) {
                missing.push(text.to_string());
            }
        }
        for reason in ["3D scene layers", "tables without geometry"] {
            if !es.contains_key(reason) {
                missing.push(reason.to_string());
            }
        }
        assert!(missing.is_empty(), "no Spanish for {missing:?}");
    }
}
