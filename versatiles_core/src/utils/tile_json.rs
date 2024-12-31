use super::parse_json_str;
use crate::utils::JsonObject;
use anyhow::Result;

#[derive(Debug, PartialEq)]
pub struct TileJSON {
	pub tilejson: String,
	pub name: Option<String>,
	pub description: Option<String>,
	pub version: Option<String>,
	pub attribution: Option<String>,
	pub template: Option<String>,
	pub legend: Option<String>,
	pub scheme: Option<String>,
	pub tiles: Option<Vec<String>>,
	pub grids: Option<Vec<String>>,
	pub data: Option<Vec<String>>,
	pub minzoom: Option<u8>,
	pub maxzoom: Option<u8>,
	pub bounds: Option<[f64; 4]>,
	pub center: Option<[f64; 3]>,
}

impl TileJSON {
	pub fn parse(text: &str) -> Result<TileJSON> {
		let object = parse_json_str(text)?.to_object()?;
		let tilejson = object.get_string("tilejson")?.unwrap();

		Ok(TileJSON {
			tilejson,
			attribution: object.get_string("attribution")?,
			description: object.get_string("description")?,
			legend: object.get_string("legend")?,
			name: object.get_string("name")?,
			scheme: object.get_string("scheme")?,
			template: object.get_string("template")?,
			version: object.get_string("version")?,
			tiles: object.get_string_vec("tiles")?,
			grids: object.get_string_vec("grids")?,
			data: object.get_string_vec("data")?,
			minzoom: object.get_number("minzoom")?,
			maxzoom: object.get_number("maxzoom")?,
			bounds: object.get_number_vec("bounds")?.and_then(|a| {
				if a.len() == 4 {
					Some([a[0], a[1], a[2], a[3]])
				} else {
					None
				}
			}),
			center: object.get_number_vec("center")?.and_then(|a| {
				if a.len() == 3 {
					Some([a[0], a[1], a[2]])
				} else {
					None
				}
			}),
		})
	}
	pub fn stringify(&self) -> String {
		let mut obj = JsonObject::default();
		obj.set("tilejson", &self.tilejson);
		obj.set_optional("tiles", &self.tiles);
		obj.set_optional("name", &self.name);
		obj.set_optional("description", &self.description);
		obj.set_optional("version", &self.version);
		obj.set_optional("attribution", &self.attribution);
		obj.set_optional("template", &self.template);
		obj.set_optional("legend", &self.legend);
		obj.set_optional("scheme", &self.scheme);

		obj.set_optional("grids", &self.grids);
		obj.set_optional("data", &self.data);

		obj.set_optional("minzoom", &self.minzoom);
		obj.set_optional("maxzoom", &self.maxzoom);
		obj.set_optional("bounds", &self.bounds.map(|v| v.to_vec()));
		obj.set_optional("center", &self.center.map(|v| v.to_vec()));

		obj.stringify()
	}
}
