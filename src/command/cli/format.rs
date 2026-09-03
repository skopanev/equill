use clap::ValueEnum;

/// Output shapes for a single-record read. Prompt assembly belongs to result
/// sets, so `get` keeps the two formats it already supported.
#[derive(Clone, Copy, Debug, ValueEnum)]
pub enum RecordFormatArg {
    Jsonl,
    Text,
}
