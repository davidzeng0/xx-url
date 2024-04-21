use std::{
	collections::HashMap,
	net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr},
	str::FromStr,
	time::{Duration, Instant}
};

use hickory_proto::{
	error::ProtoError,
	op::*,
	rr::{rdata::SOA, resource::RecordRef, *},
	serialize::binary::BinDecodable,
	xfer::DnsResponse
};
use resolv_conf::Config as ResolveConfig;
use xx_core::{
	async_std::{io::*, AsyncIteratorExt},
	macros::duration
};

use super::*;

pub mod config;
pub mod hosts;
pub mod lookup;
pub mod name_server;
pub mod resolver;

pub use config::*;
pub use hosts::*;
pub use lookup::*;
pub use name_server::*;
pub use resolver::*;

#[errors]
#[allow(clippy::large_enum_variant, variant_size_differences)]
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
