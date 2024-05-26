#[macro_export]
macro_rules! assert_wildcard {
	($expression:expr, $wildcard:expr) => {
		let expression = format!("{}", $expression);
		if !wildmatch::WildMatch::new($wildcard).matches(&expression) {
			panic!(
				"assertion failed: expression \"{expression:?}\" does not match wildcard \"{}\"",
				$wildcard
			)
		}
	};
}
