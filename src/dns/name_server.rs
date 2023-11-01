use std::{
	net::{IpAddr, SocketAddr},
	time::Duration
};

use hickory_proto::{
	op::*,
	rr::{resource::RecordRef, Record},
	serialize::binary::BinDecodable,
	xfer::DnsResponse
};
use xx_core::{error::*, trace};
use xx_pulse::*;

use super::{
	lookup::{Lookup, LookupResults},
	result::DnsError
};

#[derive(Clone)]
pub struct NameServer {
	ip: IpAddr
}

impl NameServer {
	pub fn new(ip: IpAddr) -> Self {
		Self { ip }
	}
}

impl NameServer {
	#[async_fn]
	async fn get_matching_message(&self, socket: &DatagramSocket, id: u16) -> Result<Message> {
		let mut buf = [0u8; 512];

		loop {
			let len = socket.recv(&mut buf, 0).await?;
			let message = Message::from_bytes(&buf[0..len]).map_err(Error::other)?;

			if message.id() != id {
				trace!(target: self, "== Got mismatching message ids: {} =/= {}", id, message.id());

				continue;
			}

			break Ok(message);
		}
	}

	#[async_fn]
	async fn request(&self, message: Message) -> Result<Message> {
		let socket = Udp::connect(SocketAddr::new(self.ip, 53)).await?;
		let payload = message.to_vec().map_err(Error::other)?;

		socket.send(&payload, 0).await?;

		let result = select(
			self.get_matching_message(&socket, message.id()),
			sleep(Duration::from_secs(5))
		)
		.await;

		socket.close().await?;

		match result {
			Select::First(len, _) => Ok(len?),
			Select::Second(..) => Err(Error::new(ErrorKind::TimedOut, "DNS query timed Out"))
		}
	}
}

impl ToString for NameServer {
	fn to_string(&self) -> String {
		self.ip.to_string()
	}
}

#[async_trait_impl]
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
			DnsResponse::from_message(self.request(message).await?).map_err(Error::other)?;

		if response.response_code() == ResponseCode::NoError && response.contains_answer() {
			let records: Vec<Record> = response
				.answers()
				.iter()
				.filter(|record| record.record_type() == query.query_type())
				.map(|record| record.clone())
				.collect();

			if records.len() == 0 {
				Err(Error::other(DnsError::NoData))
			} else {
				Ok(LookupResults::new(query.clone(), records, None))
			}
		} else {
			Err(Error::other(DnsError::NoRecords {
				query: response.queries().iter().next().unwrap_or(query).clone(),
				soa: response.soa().as_ref().map(RecordRef::to_owned),
				response_code: response.response_code()
			}))
		}
	}
}
