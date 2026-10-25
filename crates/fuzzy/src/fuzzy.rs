mod char_bag;
mod matcher;
mod paths;
mod strings;

pub use char_bag::CharBag;
pub use paths::PathMatch;
pub use paths::PathMatchCandidate;
pub use paths::PathMatchCandidateSet;
pub use paths::match_fixed_path_set;
pub use paths::match_path_sets;
pub use strings::StringMatch;
pub use strings::StringMatchCandidate;
pub use strings::match_strings;
