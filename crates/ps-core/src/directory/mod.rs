mod enrichment;
mod parser;

pub use enrichment::parse_file_content;
pub use parser::{DirectoryPerson, parse_directory_html};
