//! Log reader for the example app: Copper's export CLI, built for this app's messages.
//!
//! The app keeps its effective config, and so the camera URL, out of the log (see
//! `src/main.rs`). Still read a log's output before sharing it.

use cu29::prelude::*;
use cu29_export::run_cli;

gen_cumsgs!("copperconfig.ron");

fn main() {
    run_cli::<CuMsgs>().expect("Failed to run the export CLI");
}
