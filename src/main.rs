use std::{fs, process};

use anyhow::{bail, Context, Result};

use ascii_presenter::{
    editor::Editor,
    engine::{source::SourcePresentation, Engine},
    player::Player,
    renderer::Renderer,
    types::{PlayablePresentation, TerminalContract},
};

fn main() {
    if let Err(e) = run() {
        eprintln!("Error: {e:#}");
        process::exit(1);
    }
}

const COMPILE_USAGE: &str = "ascii-presenter compile <source.json> <output.json>";
const PLAY_USAGE: &str = "ascii-presenter play <presentation.json>";
const EDIT_USAGE: &str = "ascii-presenter edit <source.json>";

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
            let path = args.next().context(EDIT_USAGE)?;
            edit(&path)
        }
        _ => bail!(
            "ASCII Presenter — terminal-native presentation engine\n\nUsage:\n  {COMPILE_USAGE}\n  {PLAY_USAGE}\n  {EDIT_USAGE}"
        ),
    }
}

fn compile(source_path: &str, output_path: &str) -> Result<()> {
    let source_json =
        fs::read_to_string(source_path).with_context(|| format!("Failed to read {source_path}"))?;
    let source: SourcePresentation = serde_json::from_str(&source_json)
        .with_context(|| format!("Failed to parse {source_path}"))?;

    let scenes = Engine::compile(&source);
    let contract = TerminalContract {
        width: source.width,
        height: source.height,
    };
    let presentation = Renderer::render(&scenes, contract);

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

fn edit(path: &str) -> Result<()> {
    let mut editor = Editor::open(path)?;
    editor.run()
}

fn play(path: &str) -> Result<()> {
    let json = fs::read_to_string(path).with_context(|| format!("Failed to read {path}"))?;
    let presentation: PlayablePresentation =
        serde_json::from_str(&json).with_context(|| format!("Failed to parse {path}"))?;

    let mut player = Player::new(presentation);
    player.play()
}
