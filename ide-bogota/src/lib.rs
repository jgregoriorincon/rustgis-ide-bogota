//! IDE Bogotá: IDECA's catalogue in a panel beside the map.
//!
//! The panel shows the index this version carries (`index.json`, written by `examples/index.rs`
//! with the same reader as the live catalogue) as a tree. A plugin keeps nothing between calls,
//! so every click reads the list again: the index costs little, while reading the 665 KB
//! catalogue on each click ran out of a command's fuel, and so did reading it once to compare it
//! with the index. The repository's CI does that instead: it rebuilds the index each week and
//! proposes it when the catalogue changed (`.github/workflows/refresh-index.yml`).
//!
//! - `panel-state` runs when the panel opens; `show` when a node of the tree is chosen. The tree
//!   holds categories, their subcategories, then the services, each branch with how many
//!   services it holds; RustGIS's search box above it finds them by name. A category whose
//!   services have no subcategory holds them itself.
//! - `add` adds an ArcGIS layer's features in the map view, with the catalogue's filter applied
//!   by the server (or a GeoJSON document). `image` adds a MapServer's or ImageServer's map image
//!   as a tile layer. Both first take the map to where a layer shows: to the scales it draws at
//!   (ArcGIS's `minScale` and `maxScale`) when the map is outside them, then to the box that
//!   holds its data (with its filter) when the view holds none of it; otherwise the map stays,
//!   so a big layer still comes a piece at a time. Both act on the chosen service, or on the one their `service` parameter
//!   names, so they work from scripts and `rustgis-cli` too (where the panel never opened and
//!   its tree has no nodes yet): `plugin:ide-bogota/add={"service": "7956"}`.
//!
//! In the web app nothing may wait: a fetch not kept yet answers `pending`, the command returns
//! at once, and RustGIS runs it again when the answer lands.

// The entry point exists only in the WebAssembly module. On this computer (the tests) nothing
// calls the handler, so RustGIS's host imports are never linked.
#![cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]

pub mod ideca;

use ide_core::arcgis;
use ide_core::browse::{self, clip};
use ide_core::index;
use ide_core::{Entry, Kind};
use rustgis_plugin::{Node, Panel, Request, Value, host, json, serde_json};

/// `plugin.json`, for its translations.
const MANIFEST: &str = include_str!("../plugin.json");
/// The catalogue as it was when this version was made.
const INDEX: &str = include_str!("../index.json");
/// A tree row's most characters (RustGIS clips labels at 200).
const LABEL_MAX: usize = 200;

#[cfg(target_arch = "wasm32")]
rustgis_plugin::export!(handle);

fn handle(req: Request) -> Result<Value, String> {
    let (date, entries) = index::read(&serde_json::from_str(INDEX).unwrap_or(Value::Null));
    let other = req.tr(MANIFEST, "Other", &[]);
    let service = chosen(&req, &entries);
    let pick = || service.ok_or_else(|| req.tr(MANIFEST, "Choose a service in the catalogue.", &[]));
    let answer = match req.id.as_str() {
        "panel-state" | "show" => Value::Null,
        "add" => add(&req, pick()?)?,
        "image" => image(&req, pick()?)?,
        other => return Err(req.tr(MANIFEST, "IDE Bogotá has no command '{id}'.", &[("id", other)])),
    };
    let n = entries.len().to_string();
    let source = if date.is_empty() {
        req.tr(MANIFEST, "Built-in index: {n} services.", &[("n", &n)])
    } else {
        req.tr(MANIFEST, "Index of {date}: {n} services.", &[("date", &date), ("n", &n)])
    };
    Ok(panel(&req, source, catalogue(&entries, &other), service).answer(answer))
}

/// The service the command acts on: the one its `service` parameter names (scripts), else the
/// tree's chosen node when it is a service (not a branch). IDECA's ids are numbers, so a script
/// may give one as a number.
fn chosen<'a>(req: &Request, entries: &'a [Entry]) -> Option<&'a Entry> {
    let named = req.params.get("service").and_then(|v| v.as_str().map(str::to_string).or_else(|| v.as_u64().map(|n| n.to_string())));
    let id = named.or_else(|| req.panel_str("catalogue").map(str::to_string))?;
    entries.iter().find(|e| e.id == id)
}

/// The catalogue as the tree's nodes: categories, their subcategories, then the services by
/// name. A service's node id is its own id, what `add={"service": …}` takes; a branch's is its
/// place (`c3`, `c3.1`), so a long name never makes one too long for RustGIS.
fn catalogue(entries: &[Entry], other: &str) -> Vec<Node> {
    let services = |category: &str, subcategory: &str| -> Vec<Node> {
        browse::services(entries, category, subcategory, other)
            .into_iter()
            .map(|e| Node::new(e.id.clone(), clip(&e.name, LABEL_MAX)))
            .collect()
    };
    let branch = |id: String, name: &str, n: usize| Node::new(id, clip(name, LABEL_MAX)).count(n as u64);
    browse::categories(entries, other)
        .iter()
        .enumerate()
        .map(|(c, (category, n))| {
            let node = branch(format!("c{c}"), category, *n);
            match browse::subcategories(entries, category, other).as_slice() {
                // Nothing to tell apart: the services under the category itself.
                [(only, _)] if only == other => node.children(services(category, other)),
                subs => node.children(
                    subs.iter().enumerate().map(|(s, (sub, m))| branch(format!("c{c}.{s}"), sub, *m).children(services(category, sub))),
                ),
            }
        })
        .collect()
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

/// How many of a layer's features the map view holds, as its server counts them.
enum InView {
    Count(u64),
    /// The server didn't say (an error, a layer that doesn't count).
    Unknown,
    /// The web app is asking the server: the command runs again when it answers.
    Pending,
}

fn in_view(e: &Entry) -> Result<InView, String> {
    let Some(url) = arcgis::view_of(&host::view()?).and_then(|view| arcgis::count_url(e, view)) else { return Ok(InView::Unknown) };
    Ok(match fetch(&url) {
        Fetched::Pending => InView::Pending,
        Fetched::Doc(answer) => arcgis::count_of(&answer).map_or(InView::Unknown, InView::Count),
        Fetched::Failed => InView::Unknown,
    })
}

/// Where the map goes first when the view holds none of a layer drawn only close in: Bogotá's
/// centre, when the layer's box holds it. Most of IDECA's data is in the city, while the box's
/// own centre can be in rural Sumapaz (Catastro's lots).
const HOME: [f64; 2] = [-74.08, 4.65];

/// Where [`to_data`] left the map.
#[derive(Clone, Copy, PartialEq)]
enum Went {
    Stayed,
    /// In or out to this scale (RustGIS's), the nearest the layer draws at.
    Scaled(f64),
    /// To the data: the view held none of it.
    Moved,
    /// Nowhere: the view holds none of it, and where its data is couldn't be found.
    Lost,
    /// The web app is asking the server: the command runs again when it answers.
    Pending,
}

/// Takes the map to where an ArcGIS layer shows (`layer` is its description, `…/MapServer/3?f=json`):
///
/// 1. to the scale it draws at, round the view's centre, when the map is outside its range (a
///    server draws no map image there, and a layer drawn from 1:10,000 in is too big to download
///    over the whole city);
/// 2. to its data when the view then holds none of it. The box that holds the data is the one the
///    description declares, else (a filtered entry, a box in another system) the one the server
///    works out, which can take it most of a minute on a big layer. A layer drawn at any scale is
///    shown whole (the box); one drawn only close in, at its scale, at [`HOME`] when the box holds
///    it, else at the box's centre: the first of them where the view holds some of the data.
///
/// When the view holds some data, or the server doesn't count, the map stays where it is, so a
/// big layer still comes a piece at a time. When the view holds none and its data can't be
/// found, [`Went::Lost`]: there is nothing to add there.
fn to_data(e: &Entry, layer: &Value) -> Result<Went, String> {
    if e.kind != Kind::ArcGisLayer {
        return Ok(Went::Stayed);
    }
    let range = arcgis::scale_range(layer);
    let went = fit_scale(range)?.map_or(Went::Stayed, Went::Scaled);
    match in_view(e)? {
        InView::Pending => return Ok(Went::Pending),
        InView::Count(0) => {}
        _ => return Ok(went),
    }
    let bbox = match arcgis::declared_extent(e, layer) {
        Some(bbox) => Some(bbox),
        None => match arcgis::extent_url(e).map(|u| fetch(&u)) {
            Some(Fetched::Pending) => return Ok(Went::Pending),
            Some(Fetched::Doc(answer)) => arcgis::extent_of(&answer),
            _ => None,
        },
    };
    // No data at all (the filter leaves none), or a server that didn't answer in time.
    let Some(bbox) = bbox else { return Ok(Went::Lost) };
    if range.0.is_none() {
        return Ok(match host::run("map.zoomToExtent", json!({"extent": bbox})) {
            Ok(_) => {
                fit_scale(range)?;
                Went::Moved
            }
            // A box RustGIS can't go to.
            Err(_) => Went::Lost,
        });
    }
    let [w, s, east, north] = bbox;
    let home = (w..=east).contains(&HOME[0]) && (s..=north).contains(&HOME[1]);
    let centre = [(w + east) / 2.0, (s + north) / 2.0];
    for [lon, lat] in home.then_some(HOME).into_iter().chain([centre]) {
        if host::run("map.goToXY", json!({"lon": lon, "lat": lat})).is_err() {
            continue;
        }
        fit_scale(range)?;
        match in_view(e)? {
            InView::Pending => return Ok(Went::Pending),
            InView::Count(0) => {}
            _ => return Ok(Went::Moved),
        }
    }
    Ok(Went::Lost)
}

/// Zooms round the view's centre to the nearest scale in `range` when the map is outside it:
/// the scale gone to.
fn fit_scale(range: (Option<f64>, Option<f64>)) -> Result<Option<f64>, String> {
    let view = host::view()?;
    let n = |v: Option<&Value>| v.and_then(Value::as_f64);
    let center = view.get("center");
    let (Some(lon), Some(lat), Some(scale)) = (n(center.and_then(|c| c.get(0))), n(center.and_then(|c| c.get(1))), n(view.get("scale")))
    else {
        return Ok(None);
    };
    let Some(target) = arcgis::fit_scale(scale, lat, range) else { return Ok(None) };
    Ok(host::run("map.goToXY", json!({"lon": lon, "lat": lat, "scale": target})).ok().map(|_| target))
}

/// A layer's description (`…/MapServer/3?f=json`): how many features it gives at once and the
/// scales it draws at. `Null` when the server doesn't give it; `None` while the web app asks.
fn describe(e: &Entry) -> Option<Value> {
    match fetch(&format!("{}?f=json", e.url)) {
        Fetched::Doc(layer) => Some(layer),
        Fetched::Pending => None,
        Fetched::Failed => Some(Value::Null),
    }
}

/// The scale gone to, for a message: RustGIS's, rounded as its scale bar reads.
fn scale_text(scale: f64) -> String {
    format!("{:.0}", scale)
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
            let Some(layer) = describe(e) else { return Ok(json!({"pending": true})) };
            let limit = arcgis::max_records(&layer);
            let went = to_data(e, &layer)?;
            match went {
                Went::Pending => return Ok(json!({"pending": true})),
                Went::Lost => return Err(lost(req, e)),
                _ => {}
            }
            let view = arcgis::view_of(&host::view()?);
            let url = arcgis::features_url(e, view, limit).ok_or_else(|| not_features(req, e))?;
            // One layer per service: the one an earlier Add Features left goes, so the map shows
            // the view asked for last (a failed add puts it back with the rest).
            replace(&e.name)?;
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
                        match went {
                            Went::Moved => req.tr(MANIFEST, "The map view held none of {name}: zoomed to its data and added it.", &name),
                            Went::Scaled(s) => req.tr(
                                MANIFEST,
                                "Zoomed to 1:{scale}, where {name} is drawn, and added it.",
                                &[("scale", &scale_text(s)), ("name", &e.name)],
                            ),
                            _ => req.tr(MANIFEST, "Added {name} in the map view.", &name),
                        }
                    };
                    host::notify(&message);
                    Ok(out)
                }
                Err(err) if err.contains("web app") && arcgis::image_template(e).is_some() => {
                    let out = tiles(req, e, went)?;
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

/// Adds a MapServer's or ImageServer's map image as a tile layer over the basemap, after taking
/// the map to where a layer shows ([`to_data`]; a whole service can't be asked).
fn image(req: &Request, e: &Entry) -> Result<Value, String> {
    if arcgis::image_template(e).is_none() || e.kind != Kind::ArcGisLayer {
        return tiles(req, e, Went::Stayed);
    }
    let Some(layer) = describe(e) else { return Ok(json!({"pending": true})) };
    match to_data(e, &layer)? {
        Went::Pending => Ok(json!({"pending": true})),
        Went::Lost => Err(lost(req, e)),
        went => tiles(req, e, went),
    }
}

/// Removes the layers named `name`: those an earlier Add Features of the service added.
fn replace(name: &str) -> Result<(), String> {
    let layers = host::layers()?;
    let ids: Vec<Value> = layers
        .as_array()
        .into_iter()
        .flatten()
        .filter(|l| l.get("name").and_then(Value::as_str) == Some(name))
        .filter_map(|l| l.get("id").cloned())
        .collect();
    for id in ids {
        host::run("layer.remove", json!({"layer": id}))?;
    }
    Ok(())
}

/// Why nothing was added: the view holds none of the layer, and where its data is couldn't be
/// found.
fn lost(req: &Request, e: &Entry) -> String {
    req.tr(
        MANIFEST,
        "The map view holds none of {name}, and its server didn't say where its data is: move the map to it and add it again.",
        &[("name", e.name.as_str())],
    )
}

/// The map image's tile layer; `went` says whether the map just went to the data.
fn tiles(req: &Request, e: &Entry, went: Went) -> Result<Value, String> {
    let name = [("name", e.name.as_str())];
    let template = arcgis::image_template(e)
        .ok_or_else(|| req.tr(MANIFEST, "{name} has no map image: only ArcGIS MapServer and ImageServer services draw one.", &name))?;
    let mut params = json!({"url": template, "kind": "raster", "name": e.name, "tileSize": 512, "overlay": true, "id": tile_id(e)});
    if let Some(entity) = &e.entity {
        params["attribution"] = json!(entity);
    }
    let out = host::run("map.addTiles", params)?;
    let message = match went {
        Went::Moved => req.tr(MANIFEST, "The map view held none of {name}: zoomed to its data and added it as a map image.", &name),
        Went::Scaled(s) => req.tr(
            MANIFEST,
            "Zoomed to 1:{scale}, where {name} is drawn, and added it as a map image.",
            &[("scale", &scale_text(s)), ("name", &e.name)],
        ),
        _ => req.tr(MANIFEST, "Added {name} as a map image, over the basemap.", &name),
    };
    host::notify(&message);
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

/// The panel's state: the index it shows, the tree (sent with every answer, so that a script's
/// choice finds its nodes), the service's details and which buttons apply to it.
fn panel(req: &Request, source: String, nodes: Vec<Node>, service: Option<&Entry>) -> Panel {
    let empty = nodes.is_empty();
    let p = Panel::new().text("source", source).nodes("catalogue", nodes);
    match service {
        Some(e) => p
            .value("catalogue", e.id.as_str())
            .text("card", card(req, e))
            .enabled("add", matches!(e.kind, Kind::ArcGisLayer | Kind::GeoJson))
            .enabled("image", arcgis::image_template(e).is_some()),
        None => {
            let none = if empty {
                req.tr(MANIFEST, "The catalogue has no services to show.", &[])
            } else {
                req.tr(MANIFEST, "Choose a service in the catalogue.", &[])
            };
            p.text("card", none).enabled("add", false).enabled("image", false)
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

    fn themed(id: &str, name: &str, category: Option<&str>, subcategory: Option<&str>) -> Entry {
        let url = format!("https://x/rest/services/a/MapServer/{id}");
        Entry::new(&Raw { id: Some(id), name: Some(name), url: Some(&url), category, subcategory, ..Raw::default() })
            .expect("a valid entry")
    }

    /// A node as `id label (count)`, its children indented below it.
    fn rows(nodes: &[Node], depth: usize, out: &mut Vec<String>) {
        for n in nodes {
            let count = n.count.map_or(String::new(), |c| format!(" ({c})"));
            out.push(format!("{}{} {}{count}", "  ".repeat(depth), n.id, n.label));
            rows(&n.children, depth + 1, out);
        }
    }

    fn request(params: Value, chosen: Option<&str>) -> Request {
        let mut panel = rustgis_plugin::Map::new();
        panel.insert("catalogue".into(), chosen.map_or(Value::Null, Value::from));
        Request {
            kind: rustgis_plugin::Kind::Command,
            id: "add".into(),
            params,
            layers: Default::default(),
            files: Default::default(),
            lang: "en".into(),
            panel,
            panel_changed: Vec::new(),
        }
    }

    #[test]
    fn the_tree_holds_categories_subcategories_and_services() {
        let all = [
            themed("1", "Lote", Some("Catastro"), Some("Predios")),
            themed("2", "Árboles", Some("Ambiente"), Some("Arborización")),
            themed("3", "Ruido", Some("Ambiente"), None),
            themed("4", "Malla vial", Some("Movilidad"), None),
            themed("5", "Sin tema", None, None),
        ];
        let mut out = Vec::new();
        rows(&catalogue(&all, "Otros"), 0, &mut out);
        assert_eq!(
            out,
            [
                "c0 Ambiente (2)",
                "  c0.0 Arborización (1)",
                "    2 Árboles",
                "  c0.1 Otros (1)",
                "    3 Ruido",
                "c1 Catastro (1)",
                "  c1.0 Predios (1)",
                "    1 Lote",
                // Nothing to tell apart below these: the services right under the category.
                "c2 Movilidad (1)",
                "  4 Malla vial",
                "c3 Otros (1)",
                "  5 Sin tema",
            ]
        );
        assert!(catalogue(&[], "Otros").is_empty());
    }

    #[test]
    fn the_index_fits_in_a_tree() {
        let (_, entries) = index::read(&serde_json::from_str(INDEX).expect("index.json is JSON"));
        let nodes = catalogue(&entries, "Otros");
        fn walk<'a>(nodes: &'a [Node], depth: usize, ids: &mut Vec<&'a str>, leaves: &mut usize, deepest: &mut usize) {
            *deepest = (*deepest).max(depth);
            for n in nodes {
                ids.push(&n.id);
                assert!(n.label.chars().count() <= LABEL_MAX && n.id.chars().count() <= 200, "{}", n.id);
                if n.children.is_empty() {
                    *leaves += 1;
                }
                walk(&n.children, depth + 1, ids, leaves, deepest);
            }
        }
        let (mut ids, mut leaves, mut deepest) = (Vec::new(), 0, 0);
        walk(&nodes, 1, &mut ids, &mut leaves, &mut deepest);
        // Every service once, within RustGIS's 5,000 nodes and 8 levels; no id twice.
        assert_eq!(leaves, entries.len());
        assert!(ids.len() <= 5_000 && deepest <= 4, "{} nodes, {deepest} deep", ids.len());
        let n = ids.len();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), n, "node ids are unique");
        // Each branch counts the services below it.
        let sum: u64 = nodes.iter().filter_map(|n| n.count).sum();
        assert_eq!(sum, entries.len() as u64);
    }

    #[test]
    fn commands_act_on_the_named_or_the_chosen_service() {
        let all = [themed("7956", "Lote", Some("Catastro"), None), themed("12", "Ruido", None, None)];
        let id = |r: &Request| chosen(r, &all).map(|e| e.id.clone());
        assert_eq!(id(&request(json!({}), Some("7956"))).as_deref(), Some("7956"));
        // A script names it, whatever the panel holds.
        assert_eq!(id(&request(json!({"service": "12"}), Some("7956"))).as_deref(), Some("12"));
        assert_eq!(id(&request(json!({"service": "12"}), None)).as_deref(), Some("12"));
        // A branch, nothing, or a service the index doesn't hold: none.
        for r in [request(json!({}), Some("c0")), request(json!({}), None), request(json!({"service": "999"}), Some("7956"))] {
            assert_eq!(id(&r), None);
        }
        assert_eq!(id(&request(json!({"service": 12}), Some("7956"))).as_deref(), Some("12"), "an id given as a number");
        assert_eq!(id(&request(json!({"service": true}), Some("7956"))).as_deref(), Some("7956"), "junk: the panel's");
        assert_eq!(id(&request(Value::Null, Some("12"))).as_deref(), Some("12"));
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
