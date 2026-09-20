//! Parse arbitrary text as VPL.
//!
//! Both parsers are recursive descent, and the depth bound added after the
//! audit is the only thing between `[[[[…]]]]` and a stack overflow — which
//! aborts rather than unwinding, so it is not something a caller can catch.
//! `parseVpl` is documented as something an editor calls on every keystroke,
//! which makes arbitrary text a realistic input rather than a contrived one.

#![no_main]

use libfuzzer_sys::fuzz_target;
use versatiles_pipeline::vpl::{parse_cst, parse_vpl_detailed};

fuzz_target!(|data: &[u8]| {
	let Ok(text) = std::str::from_utf8(data) else {
		return;
	};

	let _ = parse_vpl_detailed(text);

	// The lossless parser is a separate descent over the same grammar, so it
	// has its own recursion and its own way to be wrong about it.
	if let Ok(cst) = parse_cst(text) {
		// Printing must reproduce the input byte for byte — that is the point
		// of a lossless tree, and a cheap invariant to assert while here.
		assert_eq!(cst.to_string(), text, "lossless parse did not round-trip");
	}
});
