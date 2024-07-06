#![warn(clippy::all)]

use anyhow::{Context, Result};
use clap::Parser;
use reqwest::multipart::{Form, Part};
use serde_json::Value;
use tokio::{
    fs::{remove_file, File},
    io::AsyncWriteExt,
    process::Command,
};

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// YouTube video URL
    url: String,

    /// Output text file name (optional)
    #[arg(short, long)]
    output: Option<String>,
}

async fn download_audio(url: &str, output_file: &str) -> Result<()> {
    let output = Command::new("yt-dlp")
        .args(["-f", "bestaudio", "-N8", "-o", output_file, url])
        .output()
        .await
        .context("Failed to execute yt-dlp")?;

    if !output.status.success() {
        anyhow::bail!("yt-dlp failed: {}", String::from_utf8_lossy(&output.stderr));
    }

    Ok(())
}

async fn convert_audio(input_file: &str, output_file: &str) -> Result<()> {
    let output = Command::new("ffmpeg")
        .args([
            "-i",
            input_file,
            "-c:a",
            "libopus",
            "-b:a",
            "24k",
            "-ar",
            "16000",
            "-ac",
            "1",
            "-map",
            "0:a:",
            "-vn",
            output_file,
        ])
        .output()
        .await
        .context("Failed to execute ffmpeg")?;

    if !output.status.success() {
        anyhow::bail!("ffmpeg failed: {}", String::from_utf8_lossy(&output.stderr));
    }

    Ok(())
}

async fn transcribe_audio(file_path: &str) -> Result<String> {
    let client = reqwest::Client::new();
    let url = "https://api.groq.com/openai/v1/audio/transcriptions";
    let api_key = std::env::var("GROQ_API_KEY").context("GROQ_API_KEY not set")?;

    let file_bytes = tokio::fs::read(file_path)
        .await
        .context("Failed to read audio file")?;
    let file_part = Part::bytes(file_bytes).file_name(file_path.to_string());

    let form = Form::new()
        .part("file", file_part)
        .text("model", "whisper-large-v3")
        .text("response_format", "json");

    let response = client
        .post(url)
        .header("Authorization", format!("Bearer {}", api_key))
        .multipart(form)
        .send()
        .await
        .context("Failed to send request")?;

    let json: Value = response
        .json()
        .await
        .context("Failed to parse JSON response")?;
    let transcript = json["text"]
        .as_str()
        .context("Failed to extract transcript from JSON")?
        .to_string();

    Ok(transcript)
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    // Download audio
    let audio_file = "temp_audio.webm";
    download_audio(&args.url, audio_file).await?;

    // Convert audio
    let converted_audio = "converted_audio.webm";
    convert_audio(audio_file, converted_audio).await?;

    // Transcribe audio
    let transcript = transcribe_audio(converted_audio).await?;

    if let Some(output_file) = args.output {
        // Save transcript to file
        let mut file = File::create(&output_file)
            .await
            .context("Failed to create output file")?;
        file.write_all(transcript.as_bytes())
            .await
            .context("Failed to write transcript to file")?;
        println!("Transcription completed. Output saved to {}", output_file);
    } else {
        // Print to stdout (and optionally copy to clipboard)
        println!("Transcription:");
        println!("{}", transcript);

        {
            use clipboard::{ClipboardContext, ClipboardProvider};
            let mut ctx: ClipboardContext = ClipboardProvider::new().unwrap();
            ctx.set_contents(transcript.clone()).unwrap();
            println!("\nThe transcription has been copied to your clipboard.");
        }
    }

    // Clean up temporary files
    remove_file(audio_file)
        .await
        .context("Failed to remove temporary audio file")?;
    remove_file(converted_audio)
        .await
        .context("Failed to remove converted audio file")?;

    Ok(())
}
