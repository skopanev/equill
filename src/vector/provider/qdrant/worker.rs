use crate::kernel::error::Error;
use crate::vector::model::vector_error;
use qdrant_client::{Qdrant, QdrantBuilder};
use std::future::Future;
use std::sync::mpsc::{self, Sender};
use std::thread::{self, JoinHandle};
use tokio::runtime::{Builder, Runtime};

type Task = Box<dyn FnOnce(&Runtime, &Qdrant) + Send>;

enum Command {
    Run(Task),
    Shutdown,
}

pub(super) struct RuntimeWorker {
    commands: Sender<Command>,
    thread: Option<JoinHandle<()>>,
}

impl RuntimeWorker {
    pub(super) fn start(builder: QdrantBuilder) -> Result<Self, Error> {
        let (commands, receiver) = mpsc::channel();
        let (ready_sender, ready_receiver) = mpsc::sync_channel(1);
        let worker = thread::Builder::new()
            .name("equill-qdrant".into())
            .spawn(move || {
                let runtime = match Builder::new_current_thread().enable_all().build() {
                    Ok(runtime) => runtime,
                    Err(_) => {
                        let _ = ready_sender.send(Err(()));
                        return;
                    }
                };
                let client = match builder.build() {
                    Ok(client) => client,
                    Err(_) => {
                        let _ = ready_sender.send(Err(()));
                        return;
                    }
                };
                if ready_sender.send(Ok(())).is_err() {
                    return;
                }
                while let Ok(command) = receiver.recv() {
                    match command {
                        Command::Run(task) => task(&runtime, &client),
                        Command::Shutdown => break,
                    }
                }
            })
            .map_err(|_| vector_error("qdrant worker initialization failed"))?;
        if ready_receiver.recv() != Ok(Ok(())) {
            let _ = worker.join();
            return Err(vector_error("qdrant worker initialization failed"));
        }
        Ok(Self {
            commands,
            thread: Some(worker),
        })
    }

    pub(super) fn run<T, E, F, Fut>(&self, action: &'static str, task: F) -> Result<T, Error>
    where
        T: Send + 'static,
        E: Send + 'static,
        F: FnOnce(Qdrant) -> Fut + Send + 'static,
        Fut: Future<Output = Result<T, E>> + Send + 'static,
    {
        let (result_sender, result_receiver) = mpsc::sync_channel(1);
        let run = Box::new(move |runtime: &Runtime, client: &Qdrant| {
            let result = runtime.block_on(task(client.clone())).map_err(|_| ());
            let _ = result_sender.send(result);
        });
        self.commands
            .send(Command::Run(run))
            .map_err(|_| vector_error(&format!("{action} worker failed")))?;
        result_receiver
            .recv()
            .map_err(|_| vector_error(&format!("{action} worker failed")))?
            .map_err(|()| vector_error(&format!("{action} failed")))
    }
}

impl Drop for RuntimeWorker {
    fn drop(&mut self) {
        let _ = self.commands.send(Command::Shutdown);
        if let Some(worker) = self.thread.take() {
            let _ = worker.join();
        }
    }
}
