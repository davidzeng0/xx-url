use xx_core::debug;
use xx_pulse::impls::TaskExtensionsExt;

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

#[asynchronous]
impl NameServer {
	async fn transact(&self, message: Message) -> Result<DnsResponse> {
		let addr = SocketAddr::new(self.ip, 53);
		let payload = message.to_vec().map_err(DnsError::Proto)?;

		let mut buf = [0u8; 512];
		let mut socket = Udp::connect(addr).await?;

		socket.send(&payload, Default::default()).await?;

		loop {
			let len = socket
				.recv_from_addr(&addr, &mut buf, Default::default())
				.await?;

			let response = match Message::from_bytes(&buf[0..len]) {
				Ok(message) => message,
				Err(err) => {
					debug!(target: self, "== Failed to parse response: {:?}", err);

					continue;
				}
			};

			if response.id() != message.id() {
				debug!(target: self, "== Got mismatched message ids: {} =/= {}", message.id(), response.id());

				continue;
			}

			break DnsResponse::from_message(response).map_err(Error::map);
		}
	}

	async fn request(&self, message: Message) -> Result<DnsResponse> {
		self.transact(message)
			.timeout(duration!(5 s))
			.await
			.ok_or(UrlError::DnsTimedOut)?
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

		let response = self.request(message).await?;

		if response.response_code() == ResponseCode::NoError && response.contains_answer() {
			let records: Vec<Record> = response
				.answers()
				.iter()
				.filter(|record| record.record_type() == query.query_type())
				.cloned()
				.collect();

			if !records.is_empty() {
				Ok(LookupResults::new(query.clone(), records, None))
			} else {
				Err(DnsError::NoData.into())
			}
		} else {
			Err(DnsError::NoRecords {
				query: response.queries().iter().next().unwrap_or(query).clone(),
				soa: response.soa().as_ref().map(RecordRef::to_owned),
				response_code: response.response_code()
			}
			.into())
		}
	}
}
