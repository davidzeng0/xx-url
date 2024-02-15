use xx_core::trace;
use xx_pulse::impls::TaskExtensionsExt;

use super::*;
use crate::error::UrlError;

#[derive(Debug, Clone)]
pub struct NameServer {
	ip: IpAddr
}

impl NameServer {
	pub fn new(ip: IpAddr) -> Self {
		Self { ip }
	}
}

#[asynchronous]
impl NameServer {
	async fn get_matching_message(&self, socket: &DatagramSocket, id: u16) -> Result<Message> {
		let mut buf = [0u8; 512];

		loop {
			let len = socket.recv(&mut buf, 0).await?;
			let message = match Message::from_bytes(&buf[0..len]) {
				Ok(message) => message,
				_ => continue
			};

			if message.id() != id {
				trace!(target: self, "== Got mismatching message ids: {} =/= {}", id, message.id());

				continue;
			}

			break Ok(message);
		}
	}

	async fn request(&self, message: Message) -> Result<Message> {
		let socket = Udp::connect(SocketAddr::new(self.ip, 53)).await?;
		let payload = message.to_vec().map_err(Error::map_as_other)?;

		socket.send(&payload, 0).await?;

		self.get_matching_message(&socket, message.id())
			.timeout(duration!(5 s))
			.await
			.ok_or_else(|| UrlError::DnsTimedOut.new())?
	}
}

#[asynchronous]
impl Lookup for NameServer {
	async fn lookup(&self, query: &Query) -> Result<LookupResults> {
		let mut message = Message::new();

		message
			.add_query(query.clone())
			.set_id(rand::random())
			.set_message_type(MessageType::Query)
			.set_op_code(OpCode::Query)
			.set_recursion_desired(true);

		let response =
			DnsResponse::from_message(self.request(message).await?).map_err(Error::map_as_other)?;

		if response.response_code() == ResponseCode::NoError && response.contains_answer() {
			let records: Vec<Record> = response
				.answers()
				.iter()
				.filter(|record| record.record_type() == query.query_type())
				.map(|record| record.clone())
				.collect();

			if records.len() == 0 {
				Err(Error::map_as_other(DnsError::NoData))
			} else {
				Ok(LookupResults::new(query.clone(), records, None))
			}
		} else {
			Err(Error::map_as_other(DnsError::NoRecords {
				query: response.queries().iter().next().unwrap_or(query).clone(),
				soa: response.soa().as_ref().map(RecordRef::to_owned),
				response_code: response.response_code()
			}))
		}
	}
}
