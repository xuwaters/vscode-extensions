//! Decode a `cli-completions` packed blob and print its content as JSON
//! for human inspection or diffing.

use std::fs;
use std::io::{self, Write};
use std::path::PathBuf;
use std::process::ExitCode;

use clap::Parser;
use cli_completions::{CompletionsDb, DumpCommand, EntryFlags};
use serde::Serialize;

#[derive(Debug, Parser)]
#[command(
    about = "Dump a cli-completions packed blob (.bin) as JSON for debugging.",
    version
)]
struct Args {
    /// Path to the packed blob (e.g. `data/embed/completions.bin`).
    input: PathBuf,
    /// Filter the output to specific commands; may be repeated.
    #[arg(long = "command", short = 'c')]
    commands: Vec<String>,
    /// Emit compact JSON instead of pretty-printed.
    #[arg(long)]
    compact: bool,
    /// Skip the entries list and only print summary counts.
    #[arg(long)]
    summary: bool,
}

#[derive(Serialize)]
struct DumpRoot<'a> {
    file: String,
    bytes: usize,
    command_count: usize,
    entry_count: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    commands: Option<Vec<Command<'a>>>,
}

#[derive(Serialize)]
struct Command<'a> {
    name: &'a str,
    entries: Vec<Entry<'a>>,
}

#[derive(Serialize)]
struct Entry<'a> {
    #[serde(skip_serializing_if = "Option::is_none")]
    short: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    long: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    description: Option<&'a str>,
    flags: Flags,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    subcommand_path: Vec<&'a str>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    arg_values: Vec<&'a str>,
}

#[derive(Serialize)]
struct Flags {
    bits: u8,
    requires_arg: bool,
    no_files: bool,
    force_files: bool,
    has_arg_values: bool,
    top_level_only: bool,
}

impl From<EntryFlags> for Flags {
    fn from(f: EntryFlags) -> Self {
        Self {
            bits: f.0,
            requires_arg: f.contains(EntryFlags::REQUIRES_ARG),
            no_files: f.contains(EntryFlags::NO_FILES),
            force_files: f.contains(EntryFlags::FORCE_FILES),
            has_arg_values: f.contains(EntryFlags::HAS_ARG_VALUES),
            top_level_only: f.contains(EntryFlags::TOP_LEVEL_ONLY),
        }
    }
}

fn convert<'a>(dump: &'a [DumpCommand<'a>], filter: &[String]) -> Vec<Command<'a>> {
    dump.iter()
        .filter(|c| filter.is_empty() || filter.iter().any(|n| n == c.name))
        .map(|c| Command {
            name: c.name,
            entries: c
                .entries
                .iter()
                .map(|e| Entry {
                    short: e.short.map(|c| c.to_string()),
                    long: e.long,
                    description: e.description,
                    flags: e.flags.into(),
                    subcommand_path: e.subcommand_path.clone(),
                    arg_values: e.arg_values.clone(),
                })
                .collect(),
        })
        .collect()
}

fn run(args: Args) -> io::Result<()> {
    let bytes = fs::read(&args.input)?;
    let db = CompletionsDb::from_bytes(&bytes)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, format!("invalid blob: {e}")))?;

    let dump = if args.summary { Vec::new() } else { db.dump_all() };
    let commands = if args.summary {
        None
    } else {
        Some(convert(&dump, &args.commands))
    };

    let root = DumpRoot {
        file: args.input.display().to_string(),
        bytes: bytes.len(),
        command_count: db.command_count(),
        entry_count: db.entry_count(),
        commands,
    };

    let stdout = io::stdout();
    let mut out = stdout.lock();
    if args.compact {
        serde_json::to_writer(&mut out, &root)?;
    } else {
        serde_json::to_writer_pretty(&mut out, &root)?;
    }
    out.write_all(b"\n")?;
    Ok(())
}

fn main() -> ExitCode {
    let args = Args::parse();
    match run(args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            let _ = writeln!(io::stderr(), "cli-completions-dump: {e}");
            ExitCode::FAILURE
        }
    }
}
