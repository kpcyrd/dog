//! dog, the command-line DNS client.

#![warn(deprecated_in_future)]
#![warn(future_incompatible)]
#![warn(missing_copy_implementations)]
#![warn(missing_docs)]
#![warn(nonstandard_style)]
#![warn(rust_2018_compatibility)]
#![warn(rust_2018_idioms)]
#![warn(single_use_lifetimes)]
#![warn(trivial_casts, trivial_numeric_casts)]
#![warn(unused)]

#![warn(clippy::all, clippy::pedantic)]
#![allow(clippy::enum_glob_use)]
#![allow(clippy::module_name_repetitions)]
#![allow(clippy::option_if_let_else)]
#![allow(clippy::too_many_lines)]
#![allow(clippy::unit_arg)]
#![allow(clippy::unused_self)]
#![allow(clippy::useless_let_if_seq)]
#![allow(clippy::wildcard_imports)]

#![deny(unsafe_code)]

use log::*;

mod colours;
mod connect;
mod logger;
mod output;
mod requests;
mod resolve;
mod table;
mod txid;

mod options;
use self::options::*;


/// Configures logging, parses the command-line options, and handles any
/// errors before passing control over to the Dog type.
fn main() {
    use std::env;
    use std::process::exit;

    logger::configure(env::var_os("DOG_DEBUG"));

    #[cfg(windows)]
    if let Err(e) = ansi_term::enable_ansi_support() {
        warn!("Failed to enable ANSI support: {}", e);
    }

    match Options::getopts(env::args_os().skip(1)) {
        OptionsResult::Ok(options) => {
            info!("Running with options -> {:#?}", options);
            exit(run(options));
        }

        OptionsResult::Help(help_reason, use_colours) => {
            if use_colours.should_use_colours() {
                print!("{}", include_str!(concat!(env!("OUT_DIR"), "/usage.pretty.txt")));
            }
            else {
                print!("{}", include_str!(concat!(env!("OUT_DIR"), "/usage.bland.txt")));
            }

            if help_reason == HelpReason::NoDomains {
                exit(exits::OPTIONS_ERROR);
            }
            else {
                exit(exits::SUCCESS);
            }
        }

        OptionsResult::Version(use_colours) => {
            if use_colours.should_use_colours() {
                print!("{}", include_str!(concat!(env!("OUT_DIR"), "/version.pretty.txt")));
            }
            else {
                print!("{}", include_str!(concat!(env!("OUT_DIR"), "/version.bland.txt")));
            }

            exit(exits::SUCCESS);
        }

        OptionsResult::InvalidOptionsFormat(oe) => {
            eprintln!("Invalid options: {}", oe);
            exit(exits::OPTIONS_ERROR);
        }

        OptionsResult::InvalidOptions(why) => {
            eprintln!("Invalid options: {}", why);
            exit(exits::OPTIONS_ERROR);
        }
    }
}


/// Runs dog with some options, returning the status to exit with.
fn run(Options { requests, format, measure_time }: Options) -> i32 {
    use std::time::Instant;

    let should_show_opt = requests.edns.should_show();

    let mut responses = Vec::new();
    let timer = if measure_time { Some(Instant::now()) } else { None };

    let mut errored = false;

    let requests = match requests.generate() {
        Ok(requests) => requests,
        Err(e) => {
            format.print_error(e);
            return exits::OPTIONS_ERROR;
        },
    };

    for (request, transport) in requests {
        let result = transport.send(&request);

        match result {
            Ok(mut response) => {
                if ! should_show_opt {
                    response.answers.retain(dns::Answer::is_standard);
                    response.authorities.retain(dns::Answer::is_standard);
                    response.additionals.retain(dns::Answer::is_standard);
                }

                responses.push(response);
            }
            Err(e) => {
                format.print_error(e);
                errored = true;
            }
        }
    }

    let duration = timer.map(|t| t.elapsed());
    if format.print(responses, duration) {
        if errored {
            exits::NETWORK_ERROR
        }
        else {
            exits::SUCCESS
        }
    }
    else {
        exits::NO_SHORT_RESULTS
    }
}


/// The possible status numbers dog can exit with.
mod exits {

    /// Exit code for when everything turns out OK.
    pub const SUCCESS: i32 = 0;

    /// Exit code for when there was at least one network error during execution.
    pub const NETWORK_ERROR: i32 = 1;

    /// Exit code for when there is no result from the server when running in
    /// short mode. This can be any received server error, not just `NXDOMAIN`.
    pub const NO_SHORT_RESULTS: i32 = 2;

    /// Exit code for when the command-line options are invalid.
    pub const OPTIONS_ERROR: i32 = 3;
}
