use std::{
	net::{IpAddr, Ipv4Addr, Ipv6Addr},
	str::FromStr,
	time::Instant
};

use hickory_proto::{op::Query, rr::*};
use xx_core::{debug, error::*, trace};
use xx_pulse::*;

use super::{
	config::Config,
	hosts::Hosts,
	lookup::{Lookup, LookupExt}
};

pub struct Resolver {
	services: Vec<Box<dyn Lookup>>
}

#[derive(Debug, Default)]
pub struct LookupIp {
	v4: Vec<Ipv4Addr>,
	v6: Vec<Ipv6Addr>
}

impl LookupIp {
	pub fn new() -> Self {
		Self::default()
	}

	fn from_ip(ip: IpAddr) -> Self {
		let mut this = Self::new();

		match ip {
			IpAddr::V4(addr) => this.v4.push(addr),
			IpAddr::V6(addr) => this.v6.push(addr)
		}

		this
	}

	fn push_records(&mut self, records: &Vec<Record>) {
		for record in records {
			match record.data() {
				Some(RData::A(ip)) => self.v4.push(ip.clone().into()),
				Some(RData::AAAA(ip)) => self.v6.push(ip.clone().into()),
				_ => ()
			}
		}
	}

	pub fn v4(&self) -> &Vec<Ipv4Addr> {
		&self.v4
	}

	pub fn v6(&self) -> &Vec<Ipv6Addr> {
		&self.v6
	}

	pub fn is_empty(&self) -> bool {
		self.v4.is_empty() && self.v6.is_empty()
	}
}

#[async_fn]
impl Resolver {
	pub async fn new() -> Result<Self> {
		let Join(config, hosts) = join(Config::new(), Hosts::new()).await.flatten()?;

		let mut this = Self { services: Vec::new() };

		this.services.push(Box::new(hosts));

		for nameserver in config.name_servers {
			trace!(target: &this, "++ Name Server {}", nameserver.to_string());

			this.services.push(Box::new(nameserver));
		}

		Ok(this)
	}

	async fn resolve_ips_lookup(&self, name: &Name) -> Result<LookupIp> {
		let a = Query::query(name.clone(), RecordType::A);
		let aaaa = Query::query(name.clone(), RecordType::AAAA);

		let mut error = None;
		let mut result = LookupIp::new();

		for _ in 0..3 {
			for service in &self.services {
				let Join(a, aaaa) = join(service.lookup(&a), service.lookup(&aaaa)).await;
				let mut success = false;

				match a {
					Err(err) => error = Some(err),
					Ok(results) => {
						result.push_records(results.records());
						success = true;
					}
				}

				match aaaa {
					Err(err) => error = Some(err),
					Ok(results) => {
						result.push_records(results.records());
						success = true;
					}
				}

				if success {
					return Ok(result);
				}
			}
		}

		Err(error.unwrap())
	}

	pub async fn resolve_ips(&self, name: &str) -> Result<LookupIp> {
		match name.parse() {
			Err(_) => (),
			Ok(addr) => {
				debug!(target: self, "== Addr {}", addr);

				return Ok(LookupIp::from_ip(addr));
			}
		}

		let name = Name::from_str(&name.to_lowercase()).map_err(Error::other)?;
		let now = Instant::now();

		debug!(target: self, "<< Lookup {}", name);

		let addrs = self.resolve_ips_lookup(&name).await;

		debug!(target: self, ">> Lookup {} ({:.3} ms)", name, now.elapsed().as_secs_f32() * 1000.0);

		let addrs = addrs?;

		for a in addrs.v4() {
			debug!(target: self, ">>     A    {}", a);
		}

		for aaaa in addrs.v6() {
			debug!(target: self, ">>     AAAA {}", aaaa);
		}

		Ok(addrs)
	}
}
