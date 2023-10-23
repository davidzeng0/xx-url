use std::time::Instant;

use hickory_proto::{op::Query, rr::Record};
use xx_core::{
	coroutines::{async_fn, async_trait_fn, get_context, AsyncContext},
	error::Result
};

#[derive(Clone)]
pub struct LookupResults {
	query: Query,
	records: Vec<Record>,
	valid_until: Option<Instant>
}

impl LookupResults {
	pub fn new(query: Query, records: Vec<Record>, valid_until: Option<Instant>) -> Self {
		Self { query, records, valid_until }
	}

	pub fn records(&self) -> &Vec<Record> {
		&self.records
	}

	pub fn records_mut(&mut self) -> &mut Vec<Record> {
		&mut self.records
	}

	pub fn query(&self) -> &Query {
		&self.query
	}

	pub fn valid_until(&self) -> Option<Instant> {
		self.valid_until.clone()
	}
}

pub trait Lookup<Context: AsyncContext> {
	#[async_trait_fn]
	async fn lookup(&self, query: &Query) -> Result<LookupResults>;
}

pub struct LookupService<Context: AsyncContext> {
	service: Box<dyn Lookup<Context>>
}

impl<Context: AsyncContext> LookupService<Context> {
	pub fn new(service: impl Lookup<Context> + 'static) -> Self {
		Self { service: Box::new(service) }
	}

	#[async_fn]
	pub async fn lookup(&self, query: &Query) -> Result<LookupResults> {
		self.service.lookup(query, get_context().await)
	}
}
