use std::time::Instant;

use super::*;

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

#[asynchronous]
pub trait Lookup {
	async fn lookup(&self, query: &Query) -> Result<LookupResults>;
}
