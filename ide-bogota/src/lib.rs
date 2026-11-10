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
//! - `add` adds an ArcGIS layer's features, with the catalogue's filter applied by the server (or
//!   a GeoJSON document). `image` adds a MapServer's or ImageServer's map image as a tile layer.
//!   Both do what any viewer does: read the layer's description, take the map to its extent, and
//!   add it. Both act on the chosen service, or on the one their `service` parameter names, so
//!   they work from scripts and `rustgis-cli` too (where the panel never opened and its tree has
//!   no nodes yet): `plugin:ide-bogota/add={"service": "7956"}`.
//!
//! - The plugin's ribbon tab, IDE Bogotá, holds what works without choosing anything: the button
//!   that opens the panel, buttons for IDECA's most consulted services and menus of layers by
//!   theme ([`REFERENCES`], added as `add` adds them) and IDECA's basemaps ([`BASEMAPS`]).
//!   `show`, `add` and `image` act on the panel's choice, so they are its buttons, not the
//!   ribbon's.
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
use ide_core::layers;
use ide_core::{Entry, Kind};
use rustgis_plugin::{Node, Panel, Request, Value, host, json, serde_json};

/// `plugin.json`, for its translations.
const MANIFEST: &str = include_str!("../plugin.json");
/// The catalogue as it was when this version was made.
const INDEX: &str = include_str!("../index.json");
/// A tree row's most characters (RustGIS clips labels at 200).
const LABEL_MAX: usize = 200;
/// The plugin's id, as `plugin.json` gives it: its features queries carry it
/// ([`layers::mark`]), so it knows its layers from the same query added by hand.
const PLUGIN: &str = "ide-bogota";

/// A ribbon button that adds one of the catalogue's layers by its id in the catalogue (a test
/// checks the index still holds it): IDECA's most consulted services and the reference layers
/// most maps of Bogotá start from, by theme.
struct Reference {
    command: &'static str,
    service: &'static str,
}

const REFERENCES: [Reference; 18] = [
    Reference { command: "ref-lote", service: "652" },
    Reference { command: "ref-manzana", service: "653" },
    Reference { command: "ref-construccion", service: "651" },
    Reference { command: "ref-sector", service: "654" },
    Reference { command: "ref-valor-referencia", service: "36699" },
    Reference { command: "ref-localidad", service: "646" },
    Reference { command: "ref-upl", service: "643" },
    Reference { command: "ref-barrio", service: "22814" },
    Reference { command: "ref-codigo-postal", service: "504" },
    Reference { command: "ref-colegios", service: "4060" },
    Reference { command: "ref-cupos", service: "41534" },
    Reference { command: "ref-dile", service: "369" },
    Reference { command: "ref-ips-vacunacion", service: "4148" },
    Reference { command: "ref-fiebre-amarilla", service: "660" },
    Reference { command: "ref-cade", service: "608" },
    Reference { command: "ref-rapicade", service: "609" },
    Reference { command: "ref-supercade", service: "610" },
    Reference { command: "ref-malla-vial", service: "5456" },
];

/// One of IDECA's basemaps, as its catalogue lists them (`PMBWeb/web/servicios?cmd=basemaps`):
/// a ribbon button each.
struct Basemap {
    command: &'static str,
    /// Its name on the map, translated by the manifest (its ribbon label is the command's).
    name: &'static str,
    /// A MapServer with cached tiles, or a VectorTileServer.
    url: &'static str,
    /// A MapServer's deepest tile level; `None` for a VectorTileServer, added as its style.
    max_zoom: Option<u8>,
}

/// In the ribbon's order: Mapas Bogotá's main basemap (Vector), then imagery and the two plain
/// ones to show data over, as buttons; the rest in a menu, in Mapas Bogotá's order.
const BASEMAPS: [Basemap; 9] = [
    Basemap {
        command: "basemap-vector",
        name: "Vector basemap",
        url: "https://tiles.arcgis.com/tiles/J5ltM0ovtzXUbp7B/arcgis/rest/services/Mapa_Base_Bogot%C3%A1/VectorTileServer",
        max_zoom: None,
    },
    Basemap {
        command: "basemap-hybrid",
        name: "Hybrid basemap",
        url: "https://serviciosgis.catastrobogota.gov.co/arcgis/rest/services/Mapa_Referencia/mapa_hibrido/MapServer",
        max_zoom: Some(23),
    },
    Basemap {
        command: "basemap-grey",
        name: "Grey basemap",
        url: "https://serviciosgis.catastrobogota.gov.co/arcgis/rest/services/Mapa_Referencia/mapa_base_gris/MapServer",
        max_zoom: Some(23),
    },
    Basemap {
        command: "basemap-dark",
        name: "Dark basemap",
        url: "https://serviciosgis.catastrobogota.gov.co/arcgis/rest/services/Mapa_Referencia/mapa_base_oscuro_3857/MapServer",
        max_zoom: Some(20),
    },
    Basemap {
        command: "basemap-bike",
        name: "Bike basemap",
        url: "https://tiles.arcgis.com/tiles/J5ltM0ovtzXUbp7B/arcgis/rest/services/mapabasebici_temp/VectorTileServer",
        max_zoom: None,
    },
    Basemap {
        command: "basemap-blue",
        name: "Blue basemap",
        url: "https://tiles.arcgis.com/tiles/J5ltM0ovtzXUbp7B/arcgis/rest/services/mapabaseazul/VectorTileServer",
        max_zoom: None,
    },
    Basemap {
        command: "basemap-topographic",
        name: "Topographic basemap",
        url: "https://serviciosgis.catastrobogota.gov.co/arcgis/rest/services/Mapa_Referencia/mapa_base_3857/MapServer",
        max_zoom: Some(23),
    },
    Basemap {
        command: "basemap-toner",
        name: "Toner basemap",
        url: "https://serviciosgis.catastrobogota.gov.co/arcgis/rest/services/Mapa_Referencia/mapa_base_toner/MapServer",
        max_zoom: Some(23),
    },
    Basemap {
        command: "basemap-cundinamarca",
        name: "Cundinamarca municipalities basemap",
        url: "https://serviciosgis.catastrobogota.gov.co/arcgis/rest/services/Mapa_Referencia/mapabasemunicipioscundinamarca/MapServer",
        max_zoom: Some(23),
    },
];

/// Bogotá's urban area, longitude/latitude: where a basemap of IDECA's takes a map that shows
/// none of Bogotá.
const BOGOTA: [f64; 4] = [-74.23, 4.47, -73.99, 4.84];
/// Bogotá D.C. whole, rural Sumapaz too: a view whose centre is in it already shows the city.
const BOGOTA_DC: [f64; 4] = [-74.46, 3.73, -73.98, 4.84];

/// What a command answered, and what it says it did (shown in the status bar and the panel).
type Done = (Value, Option<String>);

#[cfg(target_arch = "wasm32")]
rustgis_plugin::export!(handle);

fn handle(req: Request) -> Result<Value, String> {
    let (date, entries) = index::read(&serde_json::from_str(INDEX).unwrap_or(Value::Null));
    let other = req.tr(MANIFEST, "Other", &[]);
    let reference = REFERENCES.iter().find(|r| r.command == req.id);
    // A reference layer's button chooses it in the panel too.
    let service = match reference {
        Some(r) => entries.iter().find(|e| e.id == r.service),
        None => chosen(&req, &entries),
    };
    let pick = || service.ok_or_else(|| req.tr(MANIFEST, "Choose a service in the catalogue.", &[]));
    // The layer's description, for the scales on its card: read when a service is chosen.
    let mut layer = None;
    let (answer, said) = match req.id.as_str() {
        "panel-state" => (Value::Null, None),
        "show" => {
            layer = service.filter(|e| e.kind == Kind::ArcGisLayer).and_then(describe);
            (Value::Null, None)
        }
        "add" => add(&req, pick()?)?,
        "image" => image(&req, pick()?)?,
        id => match (reference, BASEMAPS.iter().find(|b| b.command == id)) {
            (Some(_), _) => {
                let e = service
                    .ok_or_else(|| req.tr(MANIFEST, "This layer is no longer in IDECA's catalogue: look for it in the catalogue.", &[]))?;
                add(&req, e)?
            }
            (None, Some(b)) => basemap(&req, b)?,
            (None, None) => return Err(req.tr(MANIFEST, "IDE Bogotá has no command '{id}'.", &[("id", id)])),
        },
    };
    if let Some(text) = &said {
        host::notify(text);
    }
    let n = entries.len().to_string();
    let source = if date.is_empty() {
        req.tr(MANIFEST, "Built-in index: {n} services.", &[("n", &n)])
    } else {
        req.tr(MANIFEST, "Index of {date}: {n} services.", &[("date", &date), ("n", &n)])
    };
    let p = panel(&req, source, catalogue(&entries, &other), service, layer.as_ref());
    // What the last command did, under the buttons; gone once another service is chosen.
    let p = match said {
        Some(text) => p.text("result", text).visible("result", true),
        None => p.visible("result", false),
    };
    Ok(p.answer(answer))
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

/// Where [`to_data`] left the map.
#[derive(Clone, Copy, PartialEq)]
enum Went {
    /// Where it was: not an ArcGIS layer, so its extent isn't known.
    Stayed,
    /// To the layer's extent.
    Moved,
    /// Nowhere: its extent couldn't be found, or it has no data.
    Lost,
    /// The web app is asking the server: the command runs again when it answers.
    Pending,
}

/// Takes the map to an ArcGIS layer's extent, as any viewer does when a layer is added: the one
/// its description declares (`layer`, `…/MapServer/3?f=json`), else (a filtered entry, an extent
/// in another system) the one its server works out for the data that pass the filter.
fn to_data(e: &Entry, layer: &Value) -> Result<Went, String> {
    if e.kind != Kind::ArcGisLayer {
        return Ok(Went::Stayed);
    }
    let extent = match arcgis::declared_extent(e, layer) {
        Some(extent) => Some(extent),
        None => match arcgis::extent_url(e).map(|u| fetch(&u)) {
            Some(Fetched::Pending) => return Ok(Went::Pending),
            Some(Fetched::Doc(answer)) => arcgis::extent_of(&answer),
            _ => None,
        },
    };
    let Some(extent) = extent else { return Ok(Went::Lost) };
    Ok(match host::run("map.zoomToExtent", json!({"extent": extent})) {
        Ok(_) => Went::Moved,
        Err(_) => Went::Lost,
    })
}

/// A layer's description (`…/MapServer/3?f=json`): the scales it draws at and the box of its
/// data. `Null` when the server doesn't give it; `None` while the web app asks.
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

/// Adds a service's features: an ArcGIS layer's, all that pass the catalogue's filter, after taking
/// the map to its extent ([`to_data`]); or a GeoJSON document, zoomed to. In the web app, where a
/// command can't download features, an ArcGIS layer's map image instead.
fn add(req: &Request, e: &Entry) -> Result<Done, String> {
    let name = [("name", e.name.as_str())];
    match e.kind {
        Kind::GeoJson => {
            let out = host::run("layer.addUrl", json!({"url": e.url, "name": e.name, "zoom": true}))?;
            Ok((out, Some(req.tr(MANIFEST, "Added {name}.", &name))))
        }
        Kind::ArcGisLayer => {
            let Some(layer) = describe(e) else { return Ok((json!({"pending": true}), None)) };
            let went = to_data(e, &layer)?;
            match went {
                Went::Pending => return Ok((json!({"pending": true}), None)),
                Went::Lost => return Err(lost(req, e)),
                _ => {}
            }
            let url = layers::features_url(e, None, PLUGIN).ok_or_else(|| not_features(req, e))?;
            // A service's features: in the app RustGIS reads them in pages, as protocol buffers
            // when the layer lists them (else GeoJSON), off the frame; `rustgis-cli` takes the
            // server's one answer. Either way RustGIS itself says when some were left out.
            match host::run("layer.addUrl", json!({"url": url, "name": e.name, "zoom": false, "service": true})) {
                Ok(out) => {
                    // One layer per service: the one an earlier Add Features left goes once the
                    // new one is in. Not before the call: the command runs again when the
                    // download lands, and that run would remove the layer it brought.
                    if let Some(new) = out.get("layer") {
                        replace(e, new)?;
                    }
                    let message = if out.get("pending").and_then(Value::as_bool) == Some(true) {
                        req.tr(MANIFEST, "Downloading {name}; it is added when it lands.", &name)
                    } else {
                        req.tr(MANIFEST, "Zoomed to {name} and added it.", &name)
                    };
                    Ok((out, Some(message)))
                }
                Err(err) if err.contains("web app") && arcgis::image_template(e).is_some() => {
                    let (out, _) = tiles(req, e, went)?;
                    let message =
                        req.tr(MANIFEST, "In the web app, {name} is shown as a map image; its features need the desktop app.", &name);
                    Ok((out, Some(message)))
                }
                Err(err) => Err(err),
            }
        }
        _ => Err(not_features(req, e)),
    }
}

/// Adds a MapServer's or ImageServer's map image as a tile layer over the basemap, after taking
/// the map to a layer's extent ([`to_data`]; a whole service has none to ask).
fn image(req: &Request, e: &Entry) -> Result<Done, String> {
    if arcgis::image_template(e).is_none() || e.kind != Kind::ArcGisLayer {
        return tiles(req, e, Went::Stayed);
    }
    let Some(layer) = describe(e) else { return Ok((json!({"pending": true}), None)) };
    match to_data(e, &layer)? {
        Went::Pending => Ok((json!({"pending": true}), None)),
        Went::Lost => Err(lost(req, e)),
        went => tiles(req, e, went),
    }
}

/// Puts the layer `new` an Add Features of the service just added in place of the ones an earlier
/// one left: they go, and `new`, which came in as `name (2)` while they were there, takes the
/// name of the one it replaces. Only the service's own layers go ([`layers_of`]): a layer of the
/// user's of the same name stays.
fn replace(e: &Entry, new: &Value) -> Result<(), String> {
    let old: Vec<(Value, String)> = layers_of(e)?.into_iter().filter(|(id, _)| id != new).collect();
    let Some((_, name)) = old.first().cloned() else { return Ok(()) };
    for (id, _) in old {
        host::run("layer.remove", json!({"layer": id}))?;
    }
    host::run("layer.rename", json!({"layer": new, "name": name}))?;
    Ok(())
}

/// The ids and names of the map's layers that an Add Features of `e` added: named as it (or as it
/// with RustGIS's ` (2)`, when a layer of that name was there), and read from its query. The name
/// alone would take a layer of the user's that happens to share it.
fn layers_of(e: &Entry) -> Result<Vec<(Value, String)>, String> {
    let layers = host::layers()?;
    Ok(layers
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|l| Some((l.get("id")?.clone(), l.get("name")?.as_str()?.to_string())))
        // By name first, so only these layers are asked their source.
        .filter(|(_, name)| layers::named(e, name))
        // A layer's source, as Copy Path gives it (no clipboard: that is the app's button).
        // A layer with none (drawn, or from a file) isn't the service's.
        .filter(|(id, _)| {
            host::run("layer.copyPath", json!({"layer": id}))
                .ok()
                .and_then(|out| out.get("text").and_then(Value::as_str).map(|source| layers::added_by(e, source, PLUGIN)))
                .unwrap_or(false)
        })
        .collect())
}

/// Why nothing was added: the layer's extent couldn't be found, or it has no data.
fn lost(req: &Request, e: &Entry) -> String {
    req.tr(MANIFEST, "{name}'s server didn't give its extent, or it has no data.", &[("name", e.name.as_str())])
}

/// The map image's tile layer; `went` says whether the map just went to the layer's extent.
fn tiles(req: &Request, e: &Entry, went: Went) -> Result<Done, String> {
    let name = [("name", e.name.as_str())];
    let template = arcgis::image_template(e)
        .ok_or_else(|| req.tr(MANIFEST, "{name} has no map image: only ArcGIS MapServer and ImageServer services draw one.", &name))?;
    let mut params = json!({"url": template, "kind": "raster", "name": e.name, "tileSize": 512, "overlay": true, "id": tile_id(e)});
    if let Some(entity) = &e.entity {
        params["attribution"] = json!(entity);
    }
    let out = host::run("map.addTiles", params)?;
    let message = match went {
        Went::Moved => req.tr(MANIFEST, "Zoomed to {name} and added it as a map image.", &name),
        _ => req.tr(MANIFEST, "Added {name} as a map image, over the basemap.", &name),
    };
    Ok((out, Some(message)))
}

/// Puts one of IDECA's basemaps under the map's layers, in place of the other tile basemaps.
fn basemap(req: &Request, b: &Basemap) -> Result<Done, String> {
    let name = req.tr(MANIFEST, b.name, &[]);
    let mut params =
        json!({"name": name, "overlay": false, "replace": true, "id": format!("ide-bogota-{}", b.command), "attribution": "IDECA - UAECD"});
    match b.max_zoom {
        Some(z) => {
            params["url"] = json!(format!("{}/tile/{{z}}/{{y}}/{{x}}", b.url));
            params["kind"] = json!("raster");
            params["maxZoom"] = json!(z);
        }
        // Its style, as Add From Service adds a VectorTileServer.
        None => {
            params["url"] = json!(format!("{}/resources/styles/root.json", b.url));
            params["kind"] = json!("style");
        }
    }
    let out = host::run("map.addTiles", params)?;
    // IDECA's basemaps draw Bogotá and its surroundings: a map elsewhere goes there.
    let view = host::view()?;
    let centre = view.get("center").and_then(Value::as_array).and_then(|c| Some([c.first()?.as_f64()?, c.get(1)?.as_f64()?]));
    let away =
        centre.is_none_or(|[lon, lat]| !(BOGOTA_DC[0]..=BOGOTA_DC[2]).contains(&lon) || !(BOGOTA_DC[1]..=BOGOTA_DC[3]).contains(&lat));
    let message = if away && host::run("map.zoomToExtent", json!({"extent": BOGOTA})).is_ok() {
        req.tr(MANIFEST, "Basemap: {name}. The map went to Bogotá, which it draws.", &[("name", &name)])
    } else {
        req.tr(MANIFEST, "Basemap: {name}.", &[("name", &name)])
    };
    Ok((out, Some(message)))
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
/// choice finds its nodes), the service's details (with the scales it draws at, from `layer`, its
/// description, when read) and the buttons that apply to it, the others hidden.
fn panel(req: &Request, source: String, nodes: Vec<Node>, service: Option<&Entry>, layer: Option<&Value>) -> Panel {
    let empty = nodes.is_empty();
    let p = Panel::new().text("source", source).nodes("catalogue", nodes);
    match service {
        Some(e) => p
            .value("catalogue", e.id.as_str())
            .text("card", card(req, e, layer))
            .visible("add", matches!(e.kind, Kind::ArcGisLayer | Kind::GeoJson))
            .visible("image", arcgis::image_template(e).is_some()),
        None => {
            let none = if empty {
                req.tr(MANIFEST, "The catalogue has no services to show.", &[])
            } else {
                req.tr(MANIFEST, "Choose a service in the catalogue.", &[])
            };
            p.text("card", none).visible("add", false).visible("image", false)
        }
    }
}

/// The scales a layer draws at, from its description, for its card: outside them its server
/// draws nothing.
fn drawn_at(req: &Request, layer: &Value) -> Option<String> {
    match arcgis::scale_range(layer) {
        (Some(min), Some(max)) => {
            Some(req.tr(MANIFEST, "Drawn between 1:{min} and 1:{max}.", &[("min", &scale_text(min)), ("max", &scale_text(max))]))
        }
        (Some(min), None) => Some(req.tr(MANIFEST, "Drawn from 1:{min} in.", &[("min", &scale_text(min))])),
        (None, Some(max)) => Some(req.tr(MANIFEST, "Drawn down to 1:{max}.", &[("max", &scale_text(max))])),
        (None, None) => None,
    }
}

/// A service's details, a line each: what it is, the scales it draws at (from `layer`, when
/// read), who publishes it, its licence, filter and metadata, and its address.
fn card(req: &Request, e: &Entry, layer: Option<&Value>) -> String {
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
    lines.extend(layer.and_then(|l| drawn_at(req, l)));
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

    /// The index and the manifest are JSON, and the manifest's id is the one the plugin marks its
    /// layers with.
    #[test]
    fn the_index_and_manifest_read() {
        let doc: Value = serde_json::from_str(INDEX).expect("index.json is JSON");
        assert!(doc.get("entries").is_some_and(Value::is_array));
        let manifest: Value = serde_json::from_str(MANIFEST).expect("plugin.json is JSON");
        assert_eq!(manifest["id"], PLUGIN);
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

    fn manifest() -> Value {
        serde_json::from_str(MANIFEST).expect("plugin.json is JSON")
    }

    /// Every command id the tab names, menus opened.
    fn tab_items(manifest: &Value) -> Vec<String> {
        fn walk(items: &[Value], out: &mut Vec<String>) {
            for i in items {
                match i {
                    Value::String(id) => out.push(id.clone()),
                    menu => walk(menu["items"].as_array().map_or(&[][..], Vec::as_slice), out),
                }
            }
        }
        let mut out = Vec::new();
        for g in manifest["tab"]["groups"].as_array().expect("a tab with groups") {
            walk(g["items"].as_array().expect("a group's items"), &mut out);
        }
        out
    }

    #[test]
    fn the_ribbon_holds_what_needs_no_choice() {
        let manifest = manifest();
        let commands: Vec<&str> = manifest["commands"].as_array().unwrap().iter().filter_map(|c| c["id"].as_str()).collect();
        let tab = tab_items(&manifest);
        // Every command is handled: the panel's, the reference layers' and the basemaps'.
        let mut handled = vec!["panel-state", "show", "add", "image"];
        handled.extend(REFERENCES.iter().map(|r| r.command));
        handled.extend(BASEMAPS.iter().map(|b| b.command));
        let (mut a, mut b) = (commands.clone(), handled.clone());
        a.sort_unstable();
        b.sort_unstable();
        assert_eq!(a, b, "the manifest's commands are the ones handled");
        // The tab: the panel, then the commands that act on nothing chosen; not the panel's.
        let mut wanted = vec!["panel".to_string()];
        wanted.extend(REFERENCES.iter().map(|r| r.command.to_string()));
        wanted.extend(BASEMAPS.iter().map(|b| b.command.to_string()));
        // Each once: a most consulted layer isn't in its theme's menu too.
        let (mut t, mut w) = (tab.clone(), wanted);
        t.sort_unstable();
        w.sort_unstable();
        assert_eq!(t, w);
        // The basemaps' buttons first, in their order, then the menu with the others.
        let group = manifest["tab"]["groups"].as_array().unwrap().iter().find(|g| g["label"] == "Basemap").expect("a Basemap group");
        let mut shown: Vec<String> = Vec::new();
        for i in group["items"].as_array().unwrap() {
            match i {
                Value::String(id) => shown.push(id.clone()),
                menu => shown.extend(menu["items"].as_array().unwrap().iter().filter_map(|c| c.as_str().map(str::to_string))),
            }
        }
        assert_eq!(shown, BASEMAPS.iter().map(|b| b.command.to_string()).collect::<Vec<_>>());
    }

    #[test]
    fn the_reference_layers_are_in_the_index() {
        let (_, entries) = index::read(&serde_json::from_str(INDEX).expect("index.json is JSON"));
        for r in &REFERENCES {
            let e =
                entries.iter().find(|e| e.id == r.service).unwrap_or_else(|| panic!("{} ({}) left the catalogue", r.command, r.service));
            assert_eq!(e.kind, Kind::ArcGisLayer, "{}: {} isn't an ArcGIS layer", r.command, e.name);
        }
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
        let labels = BASEMAPS.iter().map(|b| b.name);
        for reason in ["3D scene layers", "tables without geometry"].into_iter().chain(labels) {
            if !es.contains_key(reason) {
                missing.push(reason.to_string());
            }
        }
        assert!(missing.is_empty(), "no Spanish for {missing:?}");
    }
}
