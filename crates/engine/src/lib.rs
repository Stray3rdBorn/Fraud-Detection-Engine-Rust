// settings for the rules, thresholds and weights.
mod config;
// the engine that runs every rule and builds the decision.
mod engine;
// the transaction and decision types.
mod event;
// city coordinates and distance.
mod geo;
// the rule contract and the individual rules.
mod rules;
// what the engine remembers about one user.
mod state;


// make these names available to other crates as engine::Name.
pub use config::Config;
pub use engine::Engine;
pub use event::{Decision, Outcome, TransactionEvent};
pub use state::UserState;