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
    // A compiled playable can't be turned back into a source deck, so writing
    // it over the source would lose the deck for good.
    if bs::editor::same_file(source_path, output_path) {
        bail!("output {output_path} is the source file; pick another name");
    }
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
    // Replaying a frame allocates a full canvas grid; refuse an absurd contract
    // before entering raw mode rather than aborting on allocation failure.
    if let Err(e) = presentation.contract.validate() {
        bail!("Invalid playable {path}: {e}");
    }

    let mut player = Player::new(presentation);
    player.play()
}

#[cfg(test)]
mod tests {
    use super::compile;

    #[test]
    fn compile_refuses_to_write_over_its_source() {
        let dir = std::env::temp_dir().join(format!("{}_bs_compile_self", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let src = dir.join("deck.json");
        let deck = r#"{"width":4,"height":1,"frame_count":1,"objects":[]}"#;
        std::fs::write(&src, deck).unwrap();
        let s = src.to_str().unwrap();
        let mut same = vec![s.to_string(), format!("{}/./deck.json", dir.display())];
        #[cfg(unix)]
        {
            let link = dir.join("link.json");
            std::os::unix::fs::symlink(&src, &link).unwrap();
            same.push(link.to_str().unwrap().to_string());
        }
        for out in &same {
            assert!(compile(s, out).is_err(), "{out}");
            assert_eq!(std::fs::read_to_string(&src).unwrap(), deck, "{out}: source untouched");
        }
        // A different output still compiles.
        let out = dir.join("out.json");
        compile(s, out.to_str().unwrap()).unwrap();
        assert!(out.exists());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
