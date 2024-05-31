use xx_core::{debug, trace};

use super::*;

pub struct Resolver {
	services: Vec<Box<dyn Lookup + Send + Sync>>
}

#[derive(Debug, Default)]
pub struct LookupIp {
	v4: Vec<Ipv4Addr>,
	v6: Vec<Ipv6Addr>
}

impl LookupIp {
	fn from_ip(ip: IpAddr) -> Self {
		let mut this = Self::default();

		match ip {
			IpAddr::V4(addr) => this.v4.push(addr),
			IpAddr::V6(addr) => this.v6.push(addr)
		}

		this
	}

	fn push_records(&mut self, records: &[Record<'_>]) {
		for record in records {
			match &record.rdata {
				RData::A(ip) => self.v4.push(ip.address.into()),
				RData::AAAA(ip) => self.v6.push(ip.address.into()),
				_ => ()
			}
		}
	}

	#[must_use]
	pub const fn v4(&self) -> &Vec<Ipv4Addr> {
		&self.v4
	}

	#[must_use]
	pub const fn v6(&self) -> &Vec<Ipv6Addr> {
		&self.v6
	}

	#[must_use]
	pub fn is_empty(&self) -> bool {
		self.v4.is_empty() && self.v6.is_empty()
	}
}

#[asynchronous]
impl Resolver {
	pub async fn new() -> Result<Self> {
		let Join(config, hosts) = join(Config::new(), Hosts::new()).await.flatten()?;

		let mut this = Self { services: Vec::new() };

		this.services.push(Box::new(hosts));

		for nameserver in config.name_servers {
			trace!(target: &this, "++ {:?}", nameserver);

			this.services.push(Box::new(nameserver));
		}

		Ok(this)
	}

	async fn resolve_ips_lookup(&self, name: &Name<'_>) -> Result<LookupIp> {
		let mut error = None;
		let mut result = LookupIp::default();

		let a = Query::new(
			name.clone(),
			QueryType::TYPE(RecordType::A),
			QueryClass::CLASS(DnsClass::IN),
			false
		);

		let aaaa = Query::new(
			name.clone(),
			QueryType::TYPE(RecordType::AAAA),
			QueryClass::CLASS(DnsClass::IN),
			false
		);

		for _ in 0..3 {
			for service in &self.services {
				let Join(a, aaaa) = join(service.lookup(&a), service.lookup(&aaaa)).await;
				let mut success = false;

				match a {
					Ok(results) => {
						result.push_records(&results.records);
						success = true;
					}

					Err(err) => error = Some(err)
				}

				match aaaa {
					Ok(results) => {
						result.push_records(&results.records);
						success = true;
					}

					Err(err) => error = Some(err)
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
				debug!(target: self, "== Ip: {}", addr);

				return Ok(LookupIp::from_ip(addr));
			}
		}

		let name = name.to_lowercase();
		let name = Name::new(&name).map_err(DnsError::Other)?;
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
