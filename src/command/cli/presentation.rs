use super::FormatArg;

/// Flags every result set shares: filtering, retrieval policy, and output.
#[derive(Clone, Debug, clap::Args)]
pub struct PresentationArgs {
    /// Filter by a field: `field=value`. Repeat flags for AND; commas mean OR.
    #[arg(long = "where")]
    pub filters: Vec<String>,
    /// Drop records whose filtered field is absent.
    #[arg(long)]
    pub strict: bool,
    /// Output shape: JSONL, readable text, or prompt-ready Markdown.
    #[arg(long, value_enum, default_value_t = FormatArg::Jsonl)]
    pub format: FormatArg,
    /// Print only these fields, in order.
    #[arg(long, value_delimiter = ',')]
    pub fields: Vec<String>,
    #[command(flatten)]
    pub retrieval: RetrievalArgs,
}

#[derive(Clone, Debug, Default, clap::Args)]
pub struct RetrievalArgs {
    /// Override the store instruction prepended to semantic queries.
    #[arg(long)]
    pub query_instruction: Option<String>,
    /// Override semantic retrieval without changing the physical index.
    #[arg(long, action = clap::ArgAction::Set)]
    pub vector_enabled: Option<bool>,
    /// Override the minimum accepted raw cosine score.
    #[arg(long)]
    pub vector_score_threshold: Option<f32>,
    /// Override source priority, for example `vector,fts`.
    #[arg(long, value_delimiter = ',', num_args = 2)]
    pub hybrid_order: Option<Vec<RetrievalSourceArg>>,
    /// Whether the second source fills unused result capacity.
    #[arg(long, action = clap::ArgAction::Set)]
    pub hybrid_fill_remaining: Option<bool>,
    /// Whether one record returned by both sources appears once.
    #[arg(long, action = clap::ArgAction::Set)]
    pub hybrid_deduplicate: Option<bool>,
}

#[derive(Clone, Copy, Debug, clap::ValueEnum)]
pub enum RetrievalSourceArg {
    Vector,
    Fts,
}

impl RetrievalArgs {
    pub fn overrides(self) -> crate::retrieval::Overrides {
        crate::retrieval::Overrides {
            query_instruction: self.query_instruction,
            vector_enabled: self.vector_enabled,
            vector_score_threshold: self.vector_score_threshold,
            hybrid_order: self
                .hybrid_order
                .map(|values| [source(values[0]), source(values[1])]),
            hybrid_fill_remaining: self.hybrid_fill_remaining,
            hybrid_deduplicate: self.hybrid_deduplicate,
        }
    }
}

fn source(value: RetrievalSourceArg) -> crate::retrieval::Source {
    match value {
        RetrievalSourceArg::Vector => crate::retrieval::Source::Vector,
        RetrievalSourceArg::Fts => crate::retrieval::Source::Fts,
    }
}
