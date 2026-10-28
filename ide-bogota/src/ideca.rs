//! IDECA's catalogue: `PMBWeb/web/servicios?cmd=queryall`, a flat list of services.
//!
//! `{"servicios": [{"ID_SERVICIO", "NOMBRE", "URL", "TIPO", "URL2", "TIPO2", "CATEGORIA", …}]}`.
//! `URL2` is the same data as a WFS or WMS. `URL3` (a dashboard) and `URL4` (an open data page)
//! are no services, so they are left out.

use ide_core::entry::Raw;
use ide_core::{Entry, clean};
use rustgis_plugin::Value;

/// The catalogue's address. It answers with `Access-Control-Allow-Origin: *`, so the web app
/// reads it too.
pub const CATALOGUE: &str = "https://catalogopmb.catastrobogota.gov.co/PMBWeb/web/servicios?cmd=queryall";

/// The services of a `queryall` answer: the public ones, without a login, that RustGIS knows.
pub fn parse(doc: &Value) -> Vec<Entry> {
    let list = doc.get("servicios").and_then(Value::as_array).map(Vec::as_slice).unwrap_or_default();
    clean(list.iter().filter(|s| public(s)).filter_map(|s| {
        let f = |key: &str| s.get(key).and_then(Value::as_str);
        Entry::new(&Raw {
            id: f("ID_SERVICIO"),
            name: f("NOMBRE"),
            url: f("URL"),
            label: f("TIPO"),
            category: f("CATEGORIA"),
            entity: f("ENTIDAD"),
            licence: f("LICENCIA"),
            subcategory: f("SUBCATEGORIA"),
            keywords: f("KEYWORDS"),
            metadata_url: f("ID_METADATO"),
            alternates: f("URL2").map(|u| vec![(f("TIPO2"), u)]).unwrap_or_default(),
            ..Raw::default()
        })
    }))
}

/// Public and without a login: every service today, but the catalogue says so per service.
fn public(s: &Value) -> bool {
    s.get("ES_SERVICIO_PUBLICO").and_then(Value::as_bool).unwrap_or(false)
        && !s.get("ES_SERVICIO_SEGURO").and_then(Value::as_bool).unwrap_or(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ide_core::Kind;
    use rustgis_plugin::json;

    /// Two services as `queryall` gives them (2026-10-24), the second with its metadata page.
    fn answer() -> Value {
        json!({"servicios": [
            {
                "ID_SERVICIO": "501",
                "URL": "https://serviciosgis.catastrobogota.gov.co/arcgis/rest/services/desarrolloeconomico/productividadrural/MapServer/0",
                "ID_METADATO": "{A8AB6E7A-55FB-40DA-85E1-0AA6B4FE6CE3}",
                "TIPO": "FeatureLayer",
                "SUBCATEGORIA": "Desarrollo económico rural ",
                "NOMBRE": "Productividad Rural",
                "ENTIDAD": "Secretaría Distrital de Desarrollo Económico - SDDE",
                "URL4": "", "URL3": "",
                "URL2": "https://serviciosgis.catastrobogota.gov.co/arcgis/services/desarrolloeconomico/productividadrural/MapServer/WFSServer?request=GetCapabilities&service=WFS",
                "LICENCIA": "",
                "ES_SERVICIO_PUBLICO": true,
                "ES_SERVICIO_SEGURO": false,
                "TIPO2": "WFSLayer",
                "KEYWORDS": "Economía, Actividad productiva, Finca",
                "CATEGORIA": "Agricultura y Desarrollo Rural"
            },
            {
                "ID_SERVICIO": "7956",
                "URL": "https://serviciosgis.catastrobogota.gov.co/arcgis/rest/services/emergencias/gestionriesgos/MapServer/0",
                "ID_METADATO": "https://catalogopmb.catastrobogota.gov.co/PMBWeb/web/metadata?id=7095e30b-da33-426c-8e55-b1181b91f33d",
                "TIPO": "FeatureLayer",
                "NOMBRE": "Amenaza movimientos en masa en centros poblados.",
                "ENTIDAD": "Instituto Distrital de Gestión del Riesgo y Cambio Climático - IDIGER",
                "URL4": "https://datosabiertos.bogota.gov.co/dataset/amenaza-movimientos-en-masa-en-centros-poblados",
                "URL2": "",
                "ES_SERVICIO_PUBLICO": true,
                "ES_SERVICIO_SEGURO": false,
                "CATEGORIA": "Ambiente y Desarrollo Sostenible"
            }
        ]})
    }

    #[test]
    fn reads_the_catalogue() {
        let entries = parse(&answer());
        assert_eq!(entries.len(), 2);
        let rural = &entries[0];
        assert_eq!((rural.id.as_str(), rural.name.as_str(), rural.kind), ("501", "Productividad Rural", Kind::ArcGisLayer));
        assert_eq!(rural.category.as_deref(), Some("Agricultura y Desarrollo Rural"));
        assert_eq!(rural.subcategory.as_deref(), Some("Desarrollo económico rural"));
        assert_eq!(rural.description, None);
        assert_eq!(rural.keywords, ["Economía", "Actividad productiva", "Finca"]);
        assert_eq!((rural.licence.as_deref(), rural.metadata_url.as_deref()), (None, None));
        assert_eq!(rural.alternates.first().map(|(k, _)| *k), Some(Kind::Wfs));
        let risk = &entries[1];
        assert_eq!(
            risk.metadata_url.as_deref(),
            Some("https://catalogopmb.catastrobogota.gov.co/PMBWeb/web/metadata?id=7095e30b-da33-426c-8e55-b1181b91f33d")
        );
        assert!(risk.alternates.is_empty());
    }

    #[test]
    fn leaves_out_what_needs_a_login() {
        let mut doc = answer();
        doc["servicios"][0]["ES_SERVICIO_SEGURO"] = json!(true);
        doc["servicios"][1]["ES_SERVICIO_PUBLICO"] = json!(false);
        assert!(parse(&doc).is_empty());
        doc["servicios"][0]["ES_SERVICIO_SEGURO"] = json!("no");
        assert!(parse(&doc).is_empty());
    }

    #[test]
    fn a_broken_answer_gives_no_services() {
        assert!(parse(&json!(null)).is_empty());
        assert!(parse(&json!({"servicios": {}})).is_empty());
        assert!(parse(&json!({"servicios": [1, "x", null, {"ES_SERVICIO_PUBLICO": true, "ES_SERVICIO_SEGURO": false}]})).is_empty());
    }
}
