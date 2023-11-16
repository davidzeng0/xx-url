use thiserror::Error;

use super::*;

#[derive(Debug, Error)]
pub enum DnsError {
	#[error("No data")]
	NoData,

	#[error("No records for {:?}", query)]
	NoRecords {
		query: Query,
		soa: Option<Record<SOA>>,
		response_code: ResponseCode
	},

	#[error("Proto error")]
	Proto(#[from] ProtoError)
}
