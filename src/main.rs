use std::path::PathBuf;
use anyhow::Result;
use notify::{Watcher, RecursiveMode, Event, EventKind};
use tokio::sync::mpsc;
use std::collections::HashSet;
use std::sync::{Arc, Mutex};

mod services;
use services::livestream_archiver::LivestreamArchiver;

#[tokio::main]
async fn main() -> Result<()> {
    let watch_path = PathBuf::from("/home/rockvilleav/Sync/Livestreams");
    let output_path = PathBuf::from("/media/archive/jellyfin/livestreams");

    // Ensure directories exist
    if !watch_path.exists() {
        std::fs::create_dir_all(&watch_path)?;
    }
    if !output_path.exists() {
        std::fs::create_dir_all(&output_path)?;
    }

    println!("Starting livestream archiver service...");
    println!("Watching directory: {}", watch_path.display());
    println!("Output directory: {}", output_path.display());

    let archiver = LivestreamArchiver::new(&output_path);
    let processed_files = Arc::new(Mutex::new(HashSet::new()));

    // Process existing files first
    println!("Checking for existing files...");
    if let Ok(entries) = std::fs::read_dir(&watch_path) {
        for entry in entries {
            if let Ok(entry) = entry {
                let path = entry.path();
                // Only process .mp4 files
                if path.extension().and_then(|ext| ext.to_str()) == Some("mp4") {
                    // Extract date from filename to check if output exists
                    if let Some(filename) = path.file_name().and_then(|f| f.to_str()) {
                        if let Ok(date) = archiver.extract_date_from_filename(filename).await {
                            // Check if either Divine Worship or Afternoon Program exists for this date
                            let year_dir = output_path.join(date.format("%Y").to_string());
                            let month_dir = year_dir.join(format!("{}-{}", 
                                date.format("%m"),
                                date.format("%B")
                            ));
                            
                            let divine_worship_file = month_dir.join(format!(
                                "Divine Worship Service - RTSDA | {}.mp4",
                                date.format("%B %d %Y")
                            ));
                            let afternoon_program_file = month_dir.join(format!(
                                "Afternoon Program - RTSDA | {}.mp4",
                                date.format("%B %d %Y")
                            ));
                            
                            if !divine_worship_file.exists() && !afternoon_program_file.exists() {
                                println!("Found unprocessed file: {}", path.display());
                                if let Err(e) = archiver.process_file(path).await {
                                    eprintln!("Error processing existing file: {}", e);
                                }
                            } else {
                                println!("Skipping already processed file: {}", path.display());
                            }
                        }
                    }
                }
            }
        }
    }

    // Set up file watcher for new files
    let (tx, mut rx) = mpsc::channel(100);
    
    let mut watcher = notify::recommended_watcher(move |res: Result<Event, notify::Error>| {
        let tx = tx.clone();
        match res {
            Ok(event) => {
                println!("Received event: {:?}", event);
                if let Err(e) = tx.blocking_send(event) {
                    eprintln!("Error sending event: {}", e);
                }
            }
            Err(e) => eprintln!("Watch error: {}", e),
        }
    })?;

    watcher.watch(&watch_path, RecursiveMode::NonRecursive)?;

    while let Some(event) = rx.recv().await {
        println!("Processing event: {:?}", event);
        
        match event.kind {
            EventKind::Create(_) | EventKind::Modify(_) => {
                for path in event.paths {
                    if let Ok(canonical_path) = std::fs::canonicalize(&path) {
                        let path_str = canonical_path.to_string_lossy().to_string();
                        let mut processed = processed_files.lock().unwrap();
                        
                        if !processed.contains(&path_str) {
                            println!("Processing file: {}", path_str);
                            if let Err(e) = archiver.process_file(path).await {
                                eprintln!("Error processing file: {}", e);
                            } else {
                                processed.insert(path_str);
                                if processed.len() > 1000 {
                                    processed.clear();
                                }
                            }
                        } else {
                            println!("Skipping already processed file: {}", path_str);
                        }
                    }
                }
            },
            _ => println!("Ignoring event: {:?}", event),
        }
    }

    Ok(())
}
