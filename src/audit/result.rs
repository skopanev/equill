//! Request-local structured summaries survive human and LLM presentation.
use super::Output;
use crate::kernel::digest::sha256_hex;
use std::cell::RefCell;

#[derive(Default)]
struct Summary {
    ids: Vec<uuid::Uuid>,
    count: Option<u64>,
    receipt: Option<String>,
    durable: Option<bool>,
}

thread_local! { static CURRENT: RefCell<Option<Summary>> = const { RefCell::new(None) }; }

pub(crate) struct Capture(Option<Summary>);

impl Capture {
    pub(crate) fn begin() -> Self {
        Self(CURRENT.with(|slot| slot.replace(Some(Summary::default()))))
    }
}

impl Drop for Capture {
    fn drop(&mut self) {
        CURRENT.with(|slot| {
            slot.replace(self.0.take());
        });
    }
}

pub(crate) fn remember(
    ids: impl IntoIterator<Item = uuid::Uuid>,
    count: usize,
    receipt: Option<&str>,
    durable: Option<bool>,
) {
    CURRENT.with(|slot| {
        if let Some(summary) = slot.borrow_mut().as_mut() {
            summary.ids = ids.into_iter().take(16).collect();
            summary.count = Some(count as u64);
            if let Some(receipt) = receipt {
                summary.receipt = Some(sha256_hex(receipt.as_bytes()));
            }
            summary.durable = durable.or(summary.durable);
        }
    });
}

pub(super) fn apply(output: &mut Output) {
    CURRENT.with(|slot| {
        if let Some(summary) = slot.borrow().as_ref()
            && summary.count.is_some()
        {
            output.ids = summary.ids.clone();
            output.count = summary.count;
            output.receipt_sha256 = summary.receipt.clone().or(output.receipt_sha256.take());
            output.durable = summary.durable.or(output.durable);
        }
    });
}
