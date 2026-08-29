use std::{fs, process};

use anyhow::{bail, Context, Result};

use bs::{
    editor::Editor, engine::source::SourcePresentation, player::Player,
    types::PlayablePresentation,
};

fn main() {
    if let Err(e) = run() {
        eprintln!("Error: {e:#}");
        process::exit(1);
    }
}

const COMPILE_USAGE: &str = "bs compile <source.json> <output.json>";
const PLAY_USAGE: &str = "bs play <presentation.json>";
const EDIT_USAGE: &str = "bs edit <source.json> [more.json ...]";
const MIGRATE_USAGE: &str = "bs migrate <source.json>   (upgrades the file in place; writes <source.json>.bak)";

fn run() -> Result<()> {
    let mut args = std::env::args().skip(1);

    match args.next().as_deref() {
        Some("compile") => {
            let source_path = args.next().context(COMPILE_USAGE)?;
            let output_path = args.next().context(COMPILE_USAGE)?;
            compile(&source_path, &output_path)
        }
        Some("play") => {
            let path = args.next().context(PLAY_USAGE)?;
            play(&path)
        }
        Some("edit") => {
            let paths: Vec<String> = args.collect();
            if paths.is_empty() {
                bail!(EDIT_USAGE);
            }
            edit(&paths)
        }
        Some("migrate") => {
            let path = args.next().context(MIGRATE_USAGE)?;
            bs::migrate::migrate_file(&path)
        }
        _ => bail!(
            "bs — terminal-native presentation engine\n\nUsage:\n  {COMPILE_USAGE}\n  {PLAY_USAGE}\n  {EDIT_USAGE}\n  {MIGRATE_USAGE}"
        ),
    }
}

fn compile(source_path: &str, output_path: &str) -> Result<()> {
    let source_json =
        fs::read_to_string(source_path).with_context(|| format!("Failed to read {source_path}"))?;
    let source: SourcePresentation = serde_json::from_str(&source_json)
        .with_context(|| format!("Failed to parse {source_path}"))?;

    // The pipeline itself lives in `bs::compile` so the CLI and the browser's
    // WebAssembly compiler run the same code (see `wasm/src/lib.rs`).
    let presentation =
        bs::compile::compile(&source).with_context(|| format!("Invalid deck {source_path}"))?;

    let output_json = serde_json::to_string_pretty(&presentation)?;
    fs::write(output_path, &output_json)
        .with_context(|| format!("Failed to write {output_path}"))?;

    eprintln!(
        "Compiled {} frames from {} -> {}",
        presentation.frames.len(),
        source_path,
        output_path,
    );

    Ok(())
}

fn edit(paths: &[String]) -> Result<()> {
    let mut editor = Editor::open_many(paths)?;
    editor.run()
}

fn play(path: &str) -> Result<()> {
    let json = fs::read_to_string(path).with_context(|| format!("Failed to read {path}"))?;
    let presentation: PlayablePresentation =
        serde_json::from_str(&json).with_context(|| format!("Failed to parse {path}"))?;

    let mut player = Player::new(presentation);
    player.play()
}
