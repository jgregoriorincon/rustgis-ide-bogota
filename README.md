# IDE Bogotá for RustGIS

A [RustGIS](https://github.com/opengeos/RustGIS) plugin to browse the catalogue of
[IDECA](https://www.ideca.gov.co), Bogotá's spatial data infrastructure, and add its public
services to the map: an ArcGIS layer's features, or a service's map image.

> **Unofficial.** This plugin is not made, endorsed or supported by IDECA, the Unidad
> Administrativa Especial de Catastro Distrital, or any other office of Bogotá's district
> government. It reads the catalogue and the services they publish openly. The data belongs to
> them and to the entities that publish it: check each service's licence before using it.
> Report problems with the plugin in this repository's
> [issues](https://github.com/jgregoriorincon/rustgis-ide-bogota/issues), not to IDECA.

It needs RustGIS 0.6.0 or later, and the `edit` and `network` permissions.

## What you'll find

IDECA's catalogue lists the public geographic services of Bogotá's district offices. The plugin
carries it as of the date its panel shows (559 services on 2026-11-09), from 51 publishers: the
cadastre (UAECD), the water company (EAAB), and the secretariats of education, environment,
housing, economic development, social integration and planning, among others.

The services are grouped by IDECA's themes, and within them by subtheme:

| Theme | Services | For example |
|---|---|---|
| Vivienda, Ciudad y Territorio | 72 | neighbourhoods being legalised, public space, population density |
| Ambiente y Desarrollo Sostenible | 67 | urban trees, landslide hazard, land cover |
| Servicios Públicos | 54 | the water and sewer networks, free public internet |
| Economía y Finanzas | 47 | economic clusters, income, labour informality |
| Educación | 45 | schools, kindergartens, enrolment |
| Inclusión Social y Reconciliación | 39 | social integration centres, food vouchers |
| Cultura | 35 | libraries, heritage buildings, cultural venues |
| Imágenes y fotografías aéreas | 34 | satellite images, terrain and surface models, slope and hillshade maps |
| Ordenamiento Territorial | 34 | lots, buildings, localities, postcodes, contour lines |
| Transporte | 29 | road network, cycle paths, TransMilenio and Metro stations |
| Others (10 themes) | 103 | health, security, sport, tourism, justice… |

What a service is decides what you can do with it:

- **ArcGIS layers** (514): points, lines or polygons with their attributes. Add them as features
  to query, style, select, analyse and export them like any layer; or as a map image.
- **ArcGIS map and image services** (39): whole map services, and imagery (orthophotos, satellite
  images, terrain models). Add them as a map image: the server draws them, over the basemap.
- **WMS, WMTS and GeoJSON** (a few): listed with their details; adding them comes in a later
  version.

Most services are licensed CC BY-SA 4.0 (245) or CC BY 4.0 (207); 67 have a non-commercial
licence (CC BY-NC-ND, CC BY-NC-SA or CC BY-NC 4.0), and 40 state none. The licence is on each
service's details.

## Use

The plugin has a ribbon tab of its own, **IDE Bogotá**.

### The catalogue

1. **Catalogue** opens the IDECA Catalogue panel: the themes as a tree, each with how many
   services it holds, and a search box above it that finds services by name.
2. Choose a service. Its details show what it is, the scales its server draws it at, its theme,
   who publishes it, its licence, its description, a link to its metadata and its address.
3. **Add Features** reads the layer's description, takes the map to its extent and adds all its
   features, as any viewer does. RustGIS reads them in pages, and says if a server left some
   out. **Add Map Image** adds the server's own drawing of the service, over the basemap: for
   imagery, whole map services, or a quick look at a big layer.
4. Adding a service again replaces the layer the plugin added before, never one you added
   yourself (even the same service under the same name).

### The ribbon

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

### In scripts and the web app

The commands work from RustGIS scripts and `rustgis-cli`, naming a service by its id in the
catalogue (on its details): `plugin:ide-bogota/add={"service": "7956"}`.

In the web app a command can't download features, so **Add Features** adds the layer as a map
image there; the desktop app adds its features.

## Install

From the [plugin registry](https://plugins.rustgis.app), in RustGIS's **Plugins › Plugin
Manager**, once it is listed. Or download the `.rgplugin` from a
[release](https://github.com/jgregoriorincon/rustgis-ide-bogota/releases) and install it from
**Plugin Manager › Settings › Install From File…**.

## How it works

The plugin carries an index of IDECA's catalogue (`ide-bogota/index.json`): reading the
catalogue live doesn't fit in a plugin command's time. A workflow checks the catalogue once a
month (or when run by hand) and, when it changed, opens a pull request with the new index to
review and release; the panel shows the date of the index it carries. The services themselves
are always read live, from their servers.

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

The plugin's code is [MIT](LICENSE). The data belongs to IDECA and the entities that publish
it, under each service's own licence.
