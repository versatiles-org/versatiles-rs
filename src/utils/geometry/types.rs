pub type PointGeometry = [i64; 2];
pub type RingGeometry = Vec<PointGeometry>;
pub type PolygonGeometry = Vec<RingGeometry>;
pub type MultiPolygonGeometry = Vec<PolygonGeometry>;

pub struct Attribute {}
pub type Attributes = Vec<Attribute>;

pub struct PointFeature {
	pub id: Option<u64>,
	pub geometry: PointGeometry,
	pub attributes: Attributes,
}
