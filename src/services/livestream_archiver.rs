use std::path::PathBuf;
use anyhow::{Result, anyhow};
use chrono::NaiveDateTime;
use tokio::process::Command;
use tokio::time::Duration;

pub struct LivestreamArchiver {
    output_path: PathBuf,
}

impl LivestreamArchiver {
    pub fn new(output_path: PathBuf) -> Self {
        LivestreamArchiver {
            output_path,
        }
    }

    pub fn get_output_path(&self) -> &PathBuf {
        &self.output_path
    }
   
    async fn wait_for_file_ready(&self, path: &PathBuf) -> Result<()> {
        println!("Waiting for file to be ready: {}", path.display());
        
        // Initial delay - let OBS get started
        tokio::time::sleep(Duration::from_secs(10)).await;
        
        let mut last_size = 0;
        let mut stable_count = 0;
        let mut last_modified = std::time::SystemTime::now();
        let required_stable_checks = 15; // Must be stable for 30 seconds
        
        // Check for up to 4 hours (14400 seconds / 2 second interval = 7200 iterations)
        for i in 0..7200 {
            match tokio::fs::metadata(path).await {
                Ok(metadata) => {
                    let current_size = metadata.len();
                    let current_modified = metadata.modified()?;
                    
                    println!("Check {}: Size = {} bytes, Last Modified: {:?}", i, current_size, current_modified);
                    
                    if current_size > 0 {
                        if current_size == last_size {
                            // Also check if file hasn't been modified recently
                            if current_modified == last_modified {
                                stable_count += 1;
                                println!("Size and modification time stable for {} checks", stable_count);
                                
                                if stable_count >= required_stable_checks {
                                    println!("File appears complete - size and modification time stable for 30 seconds");
                                    // Extra 30 second buffer after stability to be sure
                                    tokio::time::sleep(Duration::from_secs(30)).await;
                                    return Ok(());
                                }
                            } else {
                                println!("File still being modified");
                                stable_count = 0;
                            }
                        } else {
                            println!("Size changed: {} -> {}", last_size, current_size);
                            stable_count = 0;
                        }
                        
                        last_size = current_size;
                        last_modified = current_modified;
                    }
                },
                Err(e) => {
                    println!("Error checking file: {}", e);
                    return Err(anyhow!("Failed to check file metadata: {}", e));
                }
            }
            tokio::time::sleep(Duration::from_secs(2)).await;
        }
        
        // If we reach here, it timed out after 4 hours - something is wrong
        println!("Timeout after 4 hours - file is still being written?");
        Err(anyhow!("Timeout after 4 hours waiting for file to stabilize"))
    }

    pub async fn extract_date_from_filename(&self, filename: &str) -> Result<NaiveDateTime> {
        // Example filename: "2024-12-27_18-42-36.mp4"
        let date_time_str = filename
            .strip_suffix(".mp4")
            .ok_or_else(|| anyhow!("Invalid filename format"))?;
        
        // Parse the full date and time
        let date = NaiveDateTime::parse_from_str(date_time_str, "%Y-%m-%d_%H-%M-%S")?;
        Ok(date)
    }

    pub async fn process_file(&self, path: PathBuf) -> Result<()> {
        // Only process .mp4 files
        if path.extension().and_then(|ext| ext.to_str()) != Some("mp4") {
            return Err(anyhow!("Ignoring non-MP4 file"));
        }

        println!("Processing livestream recording: {}", path.display());

        // Wait for file to be fully copied
        self.wait_for_file_ready(&path).await?;
        
        // Get the filename
        let filename = path.file_name()
            .ok_or_else(|| anyhow!("Invalid filename"))?
            .to_str()
            .ok_or_else(|| anyhow!("Invalid UTF-8 in filename"))?;

        // Extract date from filename
        let date = self.extract_date_from_filename(filename).await?;
        
        // Create date-based directory structure
        let year_dir = self.output_path.join(date.format("%Y").to_string());
        let month_dir = year_dir.join(format!("{}-{}", 
            date.format("%m"),    // numeric month (12)
            date.format("%B")     // full month name (December)
        ));
        
        // Create directories if they don't exist
        tokio::fs::create_dir_all(&month_dir).await?;

        // Check for existing files
        let divine_worship_file = month_dir.join(format!(
            "Divine Worship Service - RTSDA | {}.mp4",
            date.format("%B %d %Y")
        ));
        let afternoon_program_file = month_dir.join(format!(
            "Afternoon Program - RTSDA | {}.mp4",
            date.format("%B %d %Y")
        ));

        // Determine which filename to use
        let (base_filename, nfo_title, nfo_tag) = if !divine_worship_file.exists() {
            (
                format!("Divine Worship Service - RTSDA | {}", date.format("%B %d %Y")),
                format!("Divine Worship Service - RTSDA | {}", date.format("%B %-d %Y")),
                "Divine Worship Service"
            )
        } else if !afternoon_program_file.exists() {
            (
                format!("Afternoon Program - RTSDA | {}", date.format("%B %d %Y")),
                format!("Afternoon Program - RTSDA | {}", date.format("%B %-d %Y")),
                "Afternoon Program"
            )
        } else {
            // Both exist, add suffix to Afternoon Program
            let mut suffix = 1;
            let mut test_file = month_dir.join(format!(
                "Afternoon Program - RTSDA | {} ({}).mp4",
                date.format("%B %d %Y"),
                suffix
            ));
            while test_file.exists() {
                suffix += 1;
                test_file = month_dir.join(format!(
                    "Afternoon Program - RTSDA | {} ({}).mp4",
                    date.format("%B %d %Y"),
                    suffix
                ));
            }
            (
                format!("Afternoon Program - RTSDA | {} ({})", date.format("%B %d %Y"), suffix),
                format!("Afternoon Program - RTSDA | {} ({})", date.format("%B %-d %Y"), suffix),
                "Afternoon Program"
            )
        };

        let output_file = month_dir.join(format!("{}.mp4", base_filename));
        
        println!("Converting to AV1 and saving to: {}", output_file.display());

        // Build ffmpeg command for AV1 conversion using QSV
        let status = Command::new("ffmpeg")
            .arg("-init_hw_device").arg("qsv=hw")
            .arg("-filter_hw_device").arg("hw")
            .arg("-hwaccel").arg("qsv")
            .arg("-hwaccel_output_format").arg("qsv")
            .arg("-i").arg(&path)
            .arg("-c:v").arg("av1_qsv")
            .arg("-preset").arg("4")
            .arg("-b:v").arg("6M")
            .arg("-maxrate").arg("12M")
            .arg("-bufsize").arg("24M")
            .arg("-c:a").arg("copy")
            .arg("-n")  // Never overwrite existing files
            .arg(&output_file)
            .status()
            .await?;

        if !status.success() {
            return Err(anyhow!("FFmpeg conversion failed"));
        }

        // Create NFO file
        println!("Creating NFO file...");
        let nfo_content = format!(r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<episodedetails>
    <title>{}</title>
    <showtitle>LiveStreams</showtitle>
    <season>{}</season>
    <episode>{}</episode>
    <aired>{}</aired>
    <displayseason>{}</displayseason>
    <displayepisode>{}</displayepisode>
    <tag>{}</tag>
</episodedetails>"#,
            nfo_title,
            date.format("%Y").to_string(),
            date.format("%m%d").to_string(),
            date.format("%Y-%m-%d"),
            date.format("%Y"),
            date.format("%m%d"),
            nfo_tag
        );

        let nfo_path = output_file.with_extension("nfo");
        tokio::fs::write(nfo_path, nfo_content).await?;

        println!("Successfully converted {} to AV1 and created NFO", path.display());

        // Don't delete original file
        println!("Original file preserved at: {}", path.display());

        Ok(())
    }
}
