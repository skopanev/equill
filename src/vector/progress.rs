#[derive(Clone, Debug, PartialEq, Eq)]
pub enum VectorProgress {
    Connecting {
        collection: String,
    },
    LoadingModel,
    Scanned {
        collection: String,
        records: usize,
        pending: usize,
        corpus_sha256: String,
    },
    Embedded {
        completed: usize,
        total: usize,
    },
    Upserted {
        completed: usize,
        total: usize,
    },
    Ready {
        collection: String,
        corpus_sha256: String,
    },
}

pub trait VectorProgressSink {
    fn emit(&mut self, event: VectorProgress);
}

impl<F> VectorProgressSink for F
where
    F: FnMut(VectorProgress),
{
    fn emit(&mut self, event: VectorProgress) {
        self(event);
    }
}

pub(crate) fn emit(sink: &mut Option<&mut dyn VectorProgressSink>, event: VectorProgress) {
    if let Some(sink) = sink {
        sink.emit(event);
    }
}
