# IDE Bogotá for RustGIS

A [RustGIS](https://github.com/opengeos/RustGIS) plugin to browse the catalogue of
[IDECA](https://www.ideca.gov.co), Bogotá's spatial data infrastructure, and add its public
services to the map: an ArcGIS layer's features, or a service's map image. Adding a layer
takes the map to its extent, as any viewer does, and adding it again replaces the one added
before.

It needs RustGIS 0.6.0 or later, and the `edit` and `network` permissions.

## Use

The plugin has a ribbon tab of its own, **IDE Bogotá**:

- **Catalogue** opens the IDECA Catalogue panel: the catalogue as a tree with a search box,
  the chosen service's details (with the scales it is drawn at) and the buttons that apply to
  it, **Add Features** and **Add Map Image**.
- **Most Consulted** adds, with one click, IDECA's most consulted services as features: lots,
  schools, buildings, postcodes and, under **CADE Network**, CADE, RapiCADE and SuperCADE.
- **By Theme** holds menus of other layers most maps of Bogotá start from, each listed once:
  **Cadastre** (blocks, cadastral sectors, reference land values), **Boundaries** (localities,
  planning units, legalised neighbourhoods), **Education** (public school places, local education
  offices), **Health** (vaccination points), then the road network.
- **Basemap** puts one of IDECA's nine basemaps under the map's layers, as Mapas Bogotá offers
  them: Vector (its main one), Hybrid, Grey and Dark as buttons, and Bike, Blue, Topographic,
  Toner and Cundinamarca's municipalities under **More Basemaps**. A map away from Bogotá goes
  there.

## Install

From the [plugin registry](https://plugins.rustgis.app), in RustGIS's **Plugins › Plugin
Manager**, once it is listed. Or download the `.rgplugin` from a
[release](https://github.com/jgregoriorincon/rustgis-ide-bogota/releases) and install it from
**Plugin Manager › Settings › Install From File…**.

## How it works

The plugin carries an index of IDECA's catalogue (`ide-bogota/index.json`): reading the
catalogue live doesn't fit in a plugin command's time. A weekly workflow rebuilds the index
and, when the catalogue changed, opens a pull request with it to review and release.

Reading catalogues is shared with the other `ide-*` plugins, in
[rustgis-ide-core](https://github.com/jgregoriorincon/rustgis-ide-core).

## Develop

You need [Rust](https://rustup.rs) and Python 3.11 or later.

```sh
rustup target add wasm32-unknown-unknown
cargo test --workspace
python scripts/package.py ide-bogota   # builds dist/ide-bogota and its .rgplugin
```

Load `dist/ide-bogota` in RustGIS from **Plugin Manager › Settings › Add Plugin Folder**.

Rebuild the index by hand:

```sh
curl -s 'https://catalogopmb.catastrobogota.gov.co/PMBWeb/web/servicios?cmd=queryall' \
  | cargo run -q -p ide-bogota --example index -- "$(date -u +%F)" ide-bogota/index.json > /tmp/index.json
```

## Release

Bump `version` in `ide-bogota/plugin.json` and `ide-bogota/Cargo.toml`, then push a tag
`ide-bogota-v<version>`. The release carries the package and the registry entry to submit to
[opengeos/rustgis-plugins](https://github.com/opengeos/rustgis-plugins).

## License

[MIT](LICENSE). The data belongs to IDECA and the entities that publish it: check each
service's licence before using it.
