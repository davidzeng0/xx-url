use super::*;

#[derive(Clone)]
pub struct LookupResults {
	query: Query,
	records: Vec<Record>,
	valid_until: Option<Instant>
}

impl LookupResults {
	#[must_use]
	pub const fn new(query: Query, records: Vec<Record>, valid_until: Option<Instant>) -> Self {
		Self { query, records, valid_until }
	}

	#[must_use]
	pub const fn records(&self) -> &Vec<Record> {
		&self.records
	}

	#[must_use]
	pub fn records_mut(&mut self) -> &mut Vec<Record> {
		&mut self.records
	}

	#[must_use]
	pub const fn query(&self) -> &Query {
		&self.query
	}

	#[must_use]
	pub const fn valid_until(&self) -> Option<Instant> {
		self.valid_until
	}
}

#[asynchronous]
pub trait Lookup {
	async fn lookup(&self, query: &Query) -> Result<LookupResults>;
}
