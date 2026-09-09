use super::{cli::RecordArgs, output};
use crate::kernel::{error::Error, identity};
use crate::record;

pub(crate) fn run(json: bool, args: RecordArgs) -> Result<String, Error> {
    let actor = identity::actor_from_env()?;
    if record::is_batch(&args.input)? {
        if args.idempotency_key.is_some() {
            return Err(Error::InvalidRecord(
                "JSONL entries must carry individual idempotency keys".into(),
            ));
        }
        let report = record::append_batch(&args.store, &args.input, &actor)?;
        let text = output::batch(&report);
        let rendered = output::render(json, &report, text)?;
        return if report.ok && report.stored > 0 {
            Ok(rendered)
        } else {
            Err(Error::CommandRejected(rendered))
        };
    }
    let value: serde_json::Value = serde_json::from_slice(&std::fs::read(args.input)?)?;
    let mut request: record::AppendRequest = if value.get("draft").is_some() {
        serde_json::from_value(value)?
    } else {
        record::AppendRequest {
            draft: serde_json::from_value(value)?,
            idempotency_key: None,
        }
    };
    if args.idempotency_key.is_some() {
        if request.idempotency_key.is_some() {
            return Err(Error::InvalidRecord(
                "idempotency key supplied twice".into(),
            ));
        }
        request.idempotency_key = args.idempotency_key;
    }
    let report = record::append_request(&args.store, request, &actor)?;
    output::render(json, &report, output::record(&report))
}
