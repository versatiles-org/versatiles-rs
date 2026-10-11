//! Parameters operations used to have, and what to do in their place.
//!
//! One table for the two places that meet such a parameter: building the
//! operation, and checking a pipeline without building it (`convert
//! --dry-run`, an editor). Either one answering with the generic "has no
//! parameter" would leave a pipeline that worked yesterday with a list of
//! names and no way forward.

use anyhow::{Result, bail};

use crate::vpl::VPLNode;

/// Operation, parameter, advice.
const REMOVED_PARAMETERS: [(&str, &str, &str); 4] = [
	// Before it filled gaps (#284).
	(
		"raster_overscale",
		"level_base",
		"tiles are created from the nearest lower zoom level the source has; to ignore its higher levels, put `filter level_max=…` in front",
	),
	(
		"raster_overscale",
		"enable_climbing",
		"tiles are always created from the nearest lower zoom level the source has; remove the parameter",
	),
	(
		"vector_overzoom",
		"level_base",
		"tiles are created from the source's highest zoom level; to use a lower one, put `filter level_max=…` in front",
	),
	(
		"vector_overzoom",
		"enable_climbing",
		"a tile the source lacks at its highest zoom level is an empty one, and nothing is created from further down; remove the parameter",
	),
];

/// What to say about `parameter` of `operation`, if it is one that was removed.
pub(crate) fn removed_parameter_message(operation: &str, parameter: &str) -> Option<String> {
	REMOVED_PARAMETERS
		.iter()
		.find(|(op, name, _)| *op == operation && *name == parameter)
		.map(|(_, _, advice)| format!("'{operation}' no longer has the parameter '{parameter}': {advice}"))
}

/// Refuses a node that uses a parameter its operation no longer has.
pub(crate) fn refuse_removed_parameters(node: &VPLNode) -> Result<()> {
	for name in node.properties.keys() {
		if let Some(message) = removed_parameter_message(&node.name, name) {
			bail!(message);
		}
	}
	Ok(())
}
