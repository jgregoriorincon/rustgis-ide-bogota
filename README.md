# IDE Bogotá for RustGIS

A [RustGIS](https://github.com/opengeos/RustGIS) plugin to browse the catalogue of
[IDECA](https://www.ideca.gov.co), Bogotá's spatial data infrastructure, and add its public
services to the map: an ArcGIS layer's features in the map view, or a service's map image.
Adding a layer's features again replaces the ones added before with the new view's. When the
view holds none of a layer, the map goes to its data first (to Bogotá's centre at the scale it is
drawn at, for a layer drawn only close in).

It needs RustGIS 0.4.0 or later, and the `edit` and `network` permissions.

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
