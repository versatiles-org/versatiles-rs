use napi::Error as NapiError;

/// Convert anyhow::Error to napi::Error
pub fn anyhow_to_napi(err: anyhow::Error) -> NapiError {
	NapiError::from_reason(format!("{:#}", err))
}

/// Helper macro to convert Result<T, anyhow::Error> to Result<T, napi::Error>
#[macro_export]
macro_rules! napi_result {
	($expr:expr) => {
		$expr.map_err($crate::utils::anyhow_to_napi)
	};
}
