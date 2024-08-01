use xx_core::warn;
use xx_pulse::fs::File;

use super::*;

#[derive(Default)]
struct Results {
	a: Vec<Record<'static>>,
	aaaa: Vec<Record<'static>>
}

pub struct Hosts {
	name: HashMap<Name<'static>, Results>
}

#[asynchronous]
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
			let Ok(ip) = ip.parse() else {
				warn!(target: &hosts, "== Failed to parse ip '{}', skipping", ip);

				continue;
			};

			for host in tokens {
				let host = host.to_lowercase();
				let name = match Name::new(&host) {
					Ok(name) => name.into_owned(),
					Err(err) => {
						warn!(target: &hosts, "== Failed to parse hostname '{}': {:?}, skipping", host, err);

						continue;
					}
				};

				let results = hosts.name.entry(name.clone()).or_default();

				let rdata = match ip {
					IpAddr::V4(addr) => RData::A(addr.into()),
					IpAddr::V6(addr) => RData::AAAA(addr.into())
				};

				let record = Record::new(name, DnsClass::IN, 0, rdata);

				match &record.rdata {
					RData::A(_) => results.a.push(record),
					RData::AAAA(_) => results.aaaa.push(record),
					_ => unreachable!()
				}
			}
		}

		Ok(hosts)
	}
}

#[asynchronous]
impl Lookup for Hosts {
	async fn lookup(&self, query: &Query<'_>) -> Result<Answer> {
		let results = match self.name.get(&query.qname) {
			None => return Err(DnsError::NoData.into()),
			Some(results) => results
		};

		let results = match query.qtype {
			QueryType::TYPE(RecordType::A) => Some(results.a.clone()),
			QueryType::TYPE(RecordType::AAAA) => Some(results.aaaa.clone()),
			_ => None
		};

		if let Some(records) = results {
			Ok(Answer::new(query.clone().into_owned(), records, None))
		} else {
			Err(DnsError::NoData.into())
		}
	}
}
