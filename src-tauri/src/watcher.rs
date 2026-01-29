//! Folder watcher module - monitors directories for new PDF files

use notify::{Config, Event, RecommendedWatcher, RecursiveMode, Watcher};
use std::path::Path;
use std::sync::mpsc::{channel, Receiver};

pub struct FolderWatcher {
    _watcher: RecommendedWatcher,
    pub receiver: Receiver<Result<Event, notify::Error>>,
    pub watch_path: String,
}

impl FolderWatcher {
    pub fn new(path: &str) -> Result<Self, notify::Error> {
        let (tx, rx) = channel();

        let mut watcher = RecommendedWatcher::new(
            move |res| {
                let _ = tx.send(res);
            },
            Config::default(),
        )?;

        watcher.watch(Path::new(path), RecursiveMode::Recursive)?;

        Ok(Self {
            _watcher: watcher,
            receiver: rx,
            watch_path: path.to_string(),
        })
    }

    /// Check for new PDF files in the watched folder
    pub fn poll_new_pdfs(&self) -> Vec<String> {
        let mut new_pdfs = Vec::new();

        while let Ok(event_result) = self.receiver.try_recv() {
            if let Ok(event) = event_result {
                if matches!(event.kind, notify::EventKind::Create(_)) {
                    for path in event.paths {
                        if let Some(ext) = path.extension() {
                            if ext.eq_ignore_ascii_case("pdf") {
                                new_pdfs.push(path.to_string_lossy().to_string());
                            }
                        }
                    }
                }
            }
        }

        new_pdfs
    }
}
