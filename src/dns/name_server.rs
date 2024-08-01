use std::io::Cursor;

use xx_core::debug;
use xx_pulse::impls::TaskExt;
use xx_pulse::net::Udp;

use super::*;

#[derive(Debug, Clone)]
#[allow(missing_copy_implementations)]
pub struct NameServer {
	ip: IpAddr
}

impl NameServer {
	#[must_use]
	#[allow(clippy::missing_const_for_fn)]
	pub fn new(ip: IpAddr) -> Self {
		Self { ip }
	}
}

struct Response {
	rcode: ResponseCode,
	questions: Vec<Query<'static>>,
	answers: Vec<Record<'static>>,
	name_servers: Vec<Record<'static>>,
	additional_records: Vec<Record<'static>>
}

impl From<Packet<'_>> for Response {
	fn from(packet: Packet<'_>) -> Self {
		Self {
			rcode: packet.rcode(),
			questions: packet
				.questions
				.into_iter()
				.map(Query::into_owned)
				.collect(),
			answers: packet.answers.into_iter().map(Record::into_owned).collect(),
			name_servers: packet
				.name_servers
				.into_iter()
				.map(Record::into_owned)
				.collect(),
			additional_records: packet
				.additional_records
				.into_iter()
				.map(Record::into_owned)
				.collect()
		}
	}
}

#[asynchronous]
impl NameServer {
	async fn transact(&self, packet: Packet<'_>) -> Result<Response> {
		const MAX_PACKET_LENGTH: usize = 900;

		let mut request = Cursor::new([0u8; MAX_PACKET_LENGTH]);
		let mut response = [0u8; MAX_PACKET_LENGTH];

		packet.write_to(&mut request).map_err(DnsError::Other)?;

		let addr = SocketAddr::new(self.ip, 53);
		let mut socket = Udp::connect(addr).await?;

		#[allow(clippy::cast_possible_truncation)]
		socket
			.send(
				&request.get_ref()[0..request.position() as usize],
				Default::default()
			)
			.await?;

		loop {
			let len = socket
				.recv_from_addr(&addr, &mut response, Default::default())
				.await?;

			let response = match Packet::parse(&response[0..len]) {
				Ok(packet) => packet,
				Err(err) => {
					debug!(target: self, "== Failed to parse response: {:?}", err);

					continue;
				}
			};

			if response.id() != packet.id() {
				debug!(target: self, "== Got mismatched message ids: {} =/= {}", packet.id(), response.id());

				continue;
			}

			break Ok(response.into());
		}
	}
}

#[asynchronous]
impl Lookup for NameServer {
	async fn lookup(&self, query: &Query<'_>) -> Result<Answer> {
		let mut packet = Packet::new_query(rand::random());

		packet.set_flags(PacketFlag::RECURSION_DESIRED);
		packet.questions.push(query.clone());

		let response = self
			.transact(packet)
			.timeout(duration!(5 s))
			.await
			.ok_or(UrlError::DnsTimedOut)??;

		let mut has_answer = false;

		if response.rcode == ResponseCode::NoError {
			let all = response
				.answers
				.iter()
				.chain(&response.name_servers)
				.chain(&response.additional_records);

			let mut records = Vec::new();

			for record in all {
				if query.qname == record.name {
					has_answer = true;
				}

				if QueryClass::CLASS(record.class) != query.qclass ||
					QueryType::TYPE(record.rdata.type_code()) != query.qtype
				{
					continue;
				}

				records.push(record.clone().into_owned());
			}

			if !records.is_empty() {
				return Ok(Answer::new(query.clone().into_owned(), records, None));
			} else if has_answer {
				return Err(DnsError::NoData.into());
			}
		}

		let soa = response
			.name_servers
			.first()
			.cloned()
			.map(Record::into_owned);

		Err(DnsError::NoRecords {
			queries: response.questions,
			soa,
			response_code: response.rcode
		}
		.into())
	}
}
