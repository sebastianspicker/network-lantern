//! Report reading, paging, listing, comparison and export for native and legacy records.
mod csv_file;
mod exchange;
mod listing;
mod normalize;
mod paging;

pub use exchange::{compare, export_file};
pub use listing::list_runs;
pub use normalize::{NormalizedReport, read};
pub use paging::read_page;
