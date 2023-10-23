use std::{collections::HashMap, net::IpAddr, str::FromStr};

use hickory_proto::{
	op::Query,
	rr::{Name, RData, Record, RecordType}
};
use xx_core::{
	async_std::{io::*, AsyncIteratorExt},
	coroutines::{async_trait_fn, AsyncContext},
	error::*,
	warn
};
use xx_pulse::*;

use super::{
	lookup::{Lookup, LookupResults},
	result::DnsError
};
use crate::env::hosts_path;

struct Results {
	a: Option<LookupResults>,
	aaaa: Option<LookupResults>
}

pub struct Hosts {
	name: HashMap<Name, Results>
}

#[async_fn]
impl Hosts {
	pub async fn new() -> Result<Self> {
		let mut hosts = Self { name: HashMap::new() };

		let mut lines = BufReader::new(File::open(hosts_path()).await?).lines();

		while let Some(line) = lines.next().await {
			let line = line?;
			let line = line.split('#').next().unwrap().trim();

			if line.is_empty() {
				continue;
			}

			let mut tokens = line.split_whitespace();

			let ip = tokens.next().unwrap();

			let ip = match ip.parse() {
				Ok(ip) => ip,
				Err(_) => {
					warn!(target: &hosts, "Failed to parse ip {}", ip);

					continue;
				}
			};

			for host in tokens {
				let host = host.to_lowercase();
				let name = match Name::from_str(&host) {
					Err(err) => {
						warn!(target: &hosts, "Failed to parse hostname '{}': {}", host, err);

						continue;
					}

					Ok(name) => name
				};

				let (query, rdata) = match ip {
					IpAddr::V4(addr) => (
						Query::query(name.clone(), RecordType::A),
						RData::A(addr.into())
					),

					IpAddr::V6(addr) => (
						Query::query(name.clone(), RecordType::AAAA),
						RData::AAAA(addr.into())
					)
				};

				let record = Record::from_rdata(name.clone(), 0, rdata.clone());

				let results = hosts
					.name
					.entry(name)
					.or_insert_with(|| Results { a: None, aaaa: None });

				match query.query_type() {
					RecordType::A => results
						.a
						.get_or_insert_with(|| LookupResults::new(query, vec![], None))
						.records_mut()
						.push(record),

					RecordType::AAAA => results
						.aaaa
						.get_or_insert_with(|| LookupResults::new(query, vec![], None))
						.records_mut()
						.push(record),

					_ => unreachable!()
				}
			}
		}

		Ok(hosts)
	}
}

impl<Context: AsyncContext> Lookup<Context> for Hosts {
	#[async_trait_fn]
	async fn lookup(&self, query: &Query) -> Result<LookupResults> {
		let results = match self.name.get(query.name()) {
			None => return Err(Error::other(DnsError::NoData)),
			Some(results) => results
		};

		let results = match query.query_type() {
			RecordType::A => results.a.clone(),
			RecordType::AAAA => results.aaaa.clone(),
			_ => None
		};

		match results {
			None => Err(Error::other(DnsError::NoData)),
			Some(results) => Ok(results)
		}
	}
}
