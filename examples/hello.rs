//! Minimal boilerplate example — builds a presentation programmatically
//! and plays it directly.
//!
//! Run with: cargo run --example hello

use ascii_presenter::{
    engine::{
        source::{
            Coordinate, FrameRange, HLine, Header, Label, Position, Rect, SceneObject,
            SourcePresentation,
        },
        Engine,
    },
    player::Player,
    renderer::Renderer,
    types::{Color, NamedColor, Style, TerminalContract},
};

fn main() -> anyhow::Result<()> {
    let source = SourcePresentation {
        width: 80,
        height: 24,
        frame_count: 25,
        objects: vec![
            // ── Slide 1: big header (frames 0–4) ──────────────
            SceneObject::Header(Header {
                text: "Gnosis VPN".to_string(),
                position: Position {
                    x: Coordinate::Fixed(25),
                    y: Coordinate::Fixed(9),
                },
                style: Style {
                    fg: Some(Color::Named(NamedColor::Cyan)),
                    bold: true,
                    ..Default::default()
                },
                frames: FrameRange { start: 0, end: 1 },
                z_order: 1,
                ch: '█',
            }),
            SceneObject::Label(Label {
                text: "Welcome to ASCII Presenter".to_string(),
                position: Position {
                    x: Coordinate::Fixed(27),
                    y: Coordinate::Fixed(16),
                },
                style: Style {
                    dim: true,
                    ..Default::default()
                },
                frames: FrameRange { start: 0, end: 1 },
                z_order: 0,
            }),

            // ── Slide 2: big "HOPR" header (frames 5–9) ──────────────
            SceneObject::Header(Header {
                text: "HOPR".to_string(),
                position: Position {
                    x: Coordinate::Fixed(28),
                    y: Coordinate::Fixed(9),
                },
                style: Style {
                    fg: Some(Color::Named(NamedColor::Green)),
                    bold: true,
                    ..Default::default()
                },
                frames: FrameRange { start: 2, end: 3 },
                z_order: 1,
                ch: '█',
            }),
            SceneObject::Label(Label {
                text: "Let's see it in action...".to_string(),
                position: Position {
                    x: Coordinate::Fixed(28),
                    y: Coordinate::Fixed(16),
                },
                style: Style {
                    dim: true,
                    ..Default::default()
                },
                frames: FrameRange { start: 2, end: 3 },
                z_order: 0,
            }),

            // ── Original slides (frames 10–24) ───────────────────────

            // Title (visible from the start of this section)
            SceneObject::Label(Label {
                text: "ASCII Presenter".to_string(),
                position: Position {
                    x: Coordinate::Fixed(32),
                    y: Coordinate::Fixed(2),
                },
                style: Style {
                    bold: true,
                    ..Default::default()
                },
                frames: FrameRange { start: 4, end: 25 },
                z_order: 1,
            }),
            // Horizontal divider (appears on frame 11)
            SceneObject::HLine(HLine {
                y: 4,
                x_start: 20,
                x_end: 60,
                ch: '─',
                style: Style {
                    fg: Some(Color::Named(NamedColor::Cyan)),
                    ..Default::default()
                },
                frames: FrameRange { start: 11, end: 25 },
                z_order: 0,
            }),
            // Animated packet moving along the divider
            SceneObject::Label(Label {
                text: "[*]".to_string(),
                position: Position {
                    x: Coordinate::Animated {
                        from: 20,
                        to: 57,
                        start_frame: 13,
                        end_frame: 22,
                    },
                    y: Coordinate::Fixed(4),
                },
                style: Style {
                    fg: Some(Color::Named(NamedColor::Green)),
                    bold: true,
                    ..Default::default()
                },
                frames: FrameRange { start: 13, end: 22 },
                z_order: 10,
            }),
            // Status box (appears mid-presentation)
            SceneObject::Rect(Rect {
                position: Position {
                    x: Coordinate::Fixed(25),
                    y: Coordinate::Fixed(7),
                },
                width: 30,
                height: 5,
                style: Style {
                    fg: Some(Color::Named(NamedColor::Yellow)),
                    ..Default::default()
                },
                frames: FrameRange { start: 15, end: 25 },
                z_order: 0,
                title: Some("Status".to_string()),
            }),
            // Text inside the box
            SceneObject::Label(Label {
                text: "Packet delivered!".to_string(),
                position: Position {
                    x: Coordinate::Fixed(31),
                    y: Coordinate::Fixed(9),
                },
                style: Style::default(),
                frames: FrameRange { start: 18, end: 25 },
                z_order: 1,
            }),
            // Footer
            SceneObject::Label(Label {
                text: "A minimal boilerplate example".to_string(),
                position: Position {
                    x: Coordinate::Fixed(25),
                    y: Coordinate::Fixed(20),
                },
                style: Style {
                    dim: true,
                    ..Default::default()
                },
                frames: FrameRange { start: 10, end: 25 },
                z_order: 0,
            }),
        ],
    };

    // ── Pipeline: Source → Engine → Renderer → Player ──

    // 1. Compile: evaluate the source over all frames
    let scenes = Engine::compile(&source);

    // 2. Render: rasterize resolved scenes into a playable presentation
    let contract = TerminalContract {
        width: source.width,
        height: source.height,
    };
    let presentation = Renderer::render(&scenes, contract);

    // 3. Play: drive the presentation to the terminal
    let mut player = Player::new(presentation);
    player.play()?;

    Ok(())
}
