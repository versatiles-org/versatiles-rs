pub type PointGeometry = [i64; 2];
pub type RingGeometry = Vec<PointGeometry>;
pub type PolygonGeometry = Vec<RingGeometry>;
pub type MultiPolygonGeometry = Vec<PolygonGeometry>;

pub struct PointFeature {
	pub id: Option<u64>,
	pub tags: Vec<u32>,
	pub geom_type: Option<GeomType>,
	pub geometry: Vec<u32>,
}
