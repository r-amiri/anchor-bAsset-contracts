mod tax_querier;

pub use tax_querier::{deduct_tax, compute_lido_fee};

#[cfg(test)]
mod mock_querier;

#[cfg(test)]
mod testing;
