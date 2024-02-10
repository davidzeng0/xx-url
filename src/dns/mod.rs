use thiserror::Error;

mod config;
pub use config::*;
mod hosts;
pub use hosts::*;
mod lookup;
pub use lookup::*;
mod name_server;
pub use name_server::*;
mod resolver;
use hickory_proto::{
	error::ProtoError,
	op::*,
	rr::{rdata::SOA, resource::RecordRef, *},
	serialize::binary::BinDecodable,
	xfer::DnsResponse
};
pub use resolver::*;
use xx_core::{
	async_std::{io::*, AsyncIteratorExt},
	error::*
};
use xx_pulse::*;

use crate::env::*;

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
