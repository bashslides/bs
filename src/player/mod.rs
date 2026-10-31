//! Player — the runtime playback controller.
//!
//! Consumes a `PlayablePresentation` and drives it to the terminal.
//! The player does not interpret semantics or perform rendering decisions;
//! it treats the presentation as an immutable, authoritative visual script.

use std::io::{self, Read, Write};
use std::process::{Child, Command as ProcCommand, Stdio};
use std::sync::mpsc::{self, Receiver, SyncSender};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{bail, Result};
use crossterm::{cursor, event, queue, style, terminal};

use crate::menubar::print_menu_item;
use crate::types::{
    Cell, Color, CommandRegion, Frame, LoopRegion, NamedColor, PlayablePresentation, Style,
    TerminalContract, narrow_char,
};

/// Rows reserved above the canvas for the menu bar (when not in fullscreen).
const CANVAS_OFFSET: u16 = 1;

/// Frames moved per Shift+arrow jump during playback navigation.
const FRAMES_PER_JUMP: usize = 10;

/// How long to keep collecting output after the child exits before finalizing
/// anyway. Normally the pipes reach EOF at once; a descendant that left the
/// command's process group can hold them open, and must never stall the loop.
const DRAIN_GRACE: Duration = Duration::from_millis(250);

/// Reader chunks buffered between ticks; a full channel blocks the readers (and
/// so the command, on its pipe) instead of growing memory.
const OUTPUT_CHANNEL_CHUNKS: usize = 64;

/// Most reader chunks taken per tick, so a flood can never keep `poll` busy.
const OUTPUT_CHUNKS_PER_TICK: usize = 256;

/// Captured output kept for a `w`×`h` box: (bytes kept per line, bytes kept in
/// all). Only the start of each of the last `h` lines is ever shown, so a line
/// keeps a generous prefix (8 bytes per column, at least 4 KiB — enough unless
/// nearly every char carries its own long escape sequence in a very wide box)
/// and the buffer keeps more than `h` such lines.
fn output_caps(w: usize, h: usize) -> (usize, usize) {
    let line_cap = (8 * w).max(4096);
    (line_cap, line_cap * (h + 2))
}

/// Append `chunk` to the captured output, bounded: a line keeps at most
/// `line_cap` bytes (the rest of it, up to its newline, is dropped), and once
/// the buffer passes `2 * keep` bytes whole lines are dropped from the front
/// until at most `keep` remain. Lines are only ever cut at a newline or past
/// `line_cap`, so the first `line_cap` bytes of each of the last lines are kept.
///
/// A lone `\r` drops the line's text so far (a terminal would only move the
/// cursor back and overwrite, which is the same for a progress line redrawn in
/// place — curl, wget, pv — so the box shows its latest state). `\r\n` is a newline. A `\r` ending a chunk is kept as the
/// buffer's last byte until the next chunk says which it is (layout drops it).
fn append_output(out: &mut Vec<u8>, chunk: &[u8], line_cap: usize, keep: usize) {
    if chunk.is_empty() {
        return;
    }
    let pending_cr = out.last() == Some(&b'\r');
    if pending_cr {
        out.pop();
    }
    let mut line_len = out.len() - out.iter().rposition(|&b| b == b'\n').map_or(0, |p| p + 1);
    if pending_cr && chunk[0] != b'\n' {
        out.truncate(out.len() - line_len);
        line_len = 0;
    }
    for seg in chunk.split_inclusive(|&b| b == b'\n') {
        let (body, newline) = match seg.split_last() {
            Some((b'\n', body)) => (body, true),
            _ => (seg, false),
        };
        let pieces: Vec<&[u8]> = body.split(|&b| b == b'\r').collect();
        let mut trailing_cr = false;
        for (k, piece) in pieces.iter().enumerate() {
            if k > 0 {
                if k == pieces.len() - 1 && piece.is_empty() {
                    // A `\r` right before the `\n` (CRLF), or at the chunk's end.
                    trailing_cr = !newline;
                } else {
                    out.truncate(out.len() - line_len);
                    line_len = 0;
                }
            }
            let take = piece.len().min(line_cap.saturating_sub(line_len));
            out.extend_from_slice(&piece[..take]);
            line_len += take;
        }
        if newline {
            out.push(b'\n');
            line_len = 0;
        }
        if trailing_cr {
            out.push(b'\r');
        }
    }
    if out.len() > 2 * keep {
        let from = out.len() - keep;
        if let Some(p) = out[from..].iter().position(|&b| b == b'\n') {
            out.drain(..from + p + 1);
        }
    }
}

/// The process group a command runs in (on unix its own, led by the child).
/// Dropping it SIGKILLs the whole tree the command started, so no grandchild
/// outlives it or holds its output pipes open. It is only ever dropped while
/// the leader is unreaped, which keeps the PGID from being reused.
struct ProcGroup(u32);

impl Drop for ProcGroup {
    fn drop(&mut self) {
        #[cfg(unix)]
        if let Ok(pgid) = libc::pid_t::try_from(self.0) {
            // SAFETY: kill(2) takes only integers; a negative pid signals the group.
            unsafe { libc::kill(-pgid, libc::SIGKILL) };
        }
    }
}

/// The terminal size (columns, rows) the player needs for `contract`: the canvas
/// plus one row each for the menu bar and the status bar. In u32, so a canvas
/// height near `u16::MAX` asks for more rows than any terminal has instead of
/// overflowing (a debug panic, or in release a wrapped check that passes).
fn needed_terminal(contract: &TerminalContract) -> (u32, u32) {
    (u32::from(contract.width), u32::from(contract.height) + 2)
}

/// SIGHUP (terminal closed), SIGTERM and SIGINT turned into a stop request the
/// event loop checks every tick, so a signal ends playback through the normal
/// teardown — the running command's group is killed and the terminal restored —
/// instead of killing `bs` outright and orphaning the command's process group
/// (which, being its own group, the terminal's hangup no longer reaches).
#[cfg(unix)]
mod stop_signals {
    use std::sync::atomic::{AtomicBool, Ordering};

    const SIGNALS: [libc::c_int; 3] = [libc::SIGHUP, libc::SIGTERM, libc::SIGINT];
    static STOP: AtomicBool = AtomicBool::new(false);

    extern "C" fn on_signal(_: libc::c_int) {
        STOP.store(true, Ordering::SeqCst); // async-signal-safe
    }

    /// Catches the signals while alive; restores the previous handlers on drop.
    pub struct Guard([libc::sigaction; 3]);

    impl Guard {
        pub fn install() -> Self {
            STOP.store(false, Ordering::SeqCst);
            // SAFETY: plain sigaction(2) calls with zero-initialised structs; the
            // handler only stores to an atomic.
            unsafe {
                let mut old: [libc::sigaction; 3] = std::mem::zeroed();
                let mut sa: libc::sigaction = std::mem::zeroed();
                sa.sa_sigaction = on_signal as extern "C" fn(libc::c_int) as libc::sighandler_t;
                libc::sigemptyset(&mut sa.sa_mask);
                for (sig, prev) in SIGNALS.iter().zip(old.iter_mut()) {
                    libc::sigaction(*sig, &sa, prev);
                }
                Guard(old)
            }
        }
    }

    impl Drop for Guard {
        fn drop(&mut self) {
            for (sig, prev) in SIGNALS.iter().zip(self.0.iter()) {
                // SAFETY: restores the disposition saved by `install`.
                unsafe { libc::sigaction(*sig, prev, std::ptr::null_mut()) };
            }
        }
    }

    /// Whether a stop signal arrived since `install`.
    pub fn requested() -> bool {
        STOP.load(Ordering::SeqCst)
    }
}

#[cfg(not(unix))]
mod stop_signals {
    pub struct Guard;
    impl Guard {
        pub fn install() -> Self {
            Guard
        }
    }
    pub fn requested() -> bool {
        false
    }
}

/// Whether `child` has exited, without reaping it (unix: `waitid` + `WNOWAIT`),
/// so its PID — the group's PGID — stays reserved until the group is killed.
#[cfg(unix)]
fn exited_unreaped(child: &mut Child) -> bool {
    // SAFETY: an all-zero siginfo_t is valid; waitid only writes into it.
    let mut info: libc::siginfo_t = unsafe { std::mem::zeroed() };
    let flags = libc::WEXITED | libc::WNOHANG | libc::WNOWAIT;
    let rc = unsafe { libc::waitid(libc::P_PID, child.id() as libc::id_t, &mut info, flags) };
    if rc != 0 {
        // Only a transient EINTR means "unknown"; anything else (ECHILD) is gone.
        return io::Error::last_os_error().raw_os_error() != Some(libc::EINTR);
    }
    // With WNOHANG and nothing to report, the zeroed siginfo stays zeroed.
    info.si_signo != 0
}

#[cfg(not(unix))]
fn exited_unreaped(child: &mut Child) -> bool {
    // No process groups here, so reaping early is harmless.
    !matches!(child.try_wait(), Ok(None))
}

/// A binary currently executing for the active frame. The child runs with piped
/// stdio (it can never touch the real terminal) and is read on background
/// threads, so the event loop stays responsive — arrow keys kill it and move on.
/// Dropping it kills the child and its whole process group and reaps the child
/// (or, if it could not be signalled, leaves it to a background reaper).
struct RunningCommand {
    region: CommandRegion,
    child: Child,
    /// Dropped (killing the tree) when the leader exits or times out.
    group: Option<ProcGroup>,
    start: Instant,
    /// Kill-after duration, or `None` to run with no timeout.
    timeout: Option<Duration>,
    rx: Receiver<Vec<u8>>,
    /// Accumulated stdout+stderr bytes, bounded by `append_output`.
    out: Vec<u8>,
    /// New output arrived since the last paint.
    dirty: bool,
    /// Once the child is finished: (success, timed out, could not be stopped,
    /// when).
    exited: Option<(bool, bool, bool, Instant)>,
}

impl Drop for RunningCommand {
    fn drop(&mut self) {
        // Once `poll` has finished the command, it is reaped (or handed to a
        // background reaper) already; touching the PID again could hit a reused one.
        if self.exited.is_some() {
            return;
        }
        self.group = None; // kill the tree before reaping the leader
        let _ = self.child.kill();
        let _ = reap_without_blocking(&mut self.child);
    }
}

/// How long to wait for a just-killed leader to die before giving up on it.
const REAP_WAIT: Duration = Duration::from_millis(100);

/// Reap `child` after it was killed, never blocking the UI thread for more
/// than [`REAP_WAIT`]: a leader the player cannot signal (a setuid program such
/// as `sudo` runs under another real uid, so kill(2) fails with EPERM) keeps
/// running, and is then left to a background thread that reaps it whenever it
/// exits. Returns its exit status, or `None` if it was left running.
fn reap_without_blocking(child: &mut Child) -> Option<std::process::ExitStatus> {
    let deadline = Instant::now() + REAP_WAIT;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return Some(status),
            Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(5)),
            Ok(None) => break,
            Err(_) => return None,
        }
    }
    #[cfg(unix)]
    if let Ok(pid) = libc::pid_t::try_from(child.id()) {
        thread::spawn(move || {
            let mut status = 0;
            // SAFETY: waitpid on our own child, which nothing else reaps now.
            unsafe { libc::waitpid(pid, &mut status, 0) };
        });
    }
    None
}

/// Spawn `region`'s binary with piped stdio, in its own session (unix: so it
/// leads its own process group and has no controlling terminal), with
/// background readers feeding one channel.
fn spawn_command(region: CommandRegion) -> io::Result<RunningCommand> {
    let mut cmd = ProcCommand::new(&region.command);
    cmd.args(&region.args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(cwd) = &region.cwd {
        cmd.current_dir(cwd);
    }
    #[cfg(unix)]
    {
        // SAFETY: getpid is always safe.
        #[cfg(target_os = "linux")]
        let parent = unsafe { libc::getpid() };
        // SAFETY: the closure only makes async-signal-safe calls (setsid,
        // prctl, getppid) and builds an io::Error without allocating.
        unsafe {
            std::os::unix::process::CommandExt::pre_exec(&mut cmd, move || {
                // A new session: the child leads its own process group (the
                // group `ProcGroup` kills) and has no controlling terminal, so
                // a program that opens /dev/tty to prompt fails at once instead
                // of writing over the slide and stopping on SIGTTIN.
                if libc::setsid() == -1 {
                    return Err(io::Error::last_os_error());
                }
                // Backstop for a `bs` that dies without any teardown (SIGKILL):
                // the kernel SIGKILLs the command (the session leader) when the
                // spawning thread exits.
                #[cfg(target_os = "linux")]
                {
                    if libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL) != 0 {
                        return Err(io::Error::last_os_error());
                    }
                    if libc::getppid() != parent {
                        // The player died before prctl took effect.
                        return Err(io::Error::from_raw_os_error(libc::ESRCH));
                    }
                }
                Ok(())
            });
        }
    }

    let mut child = cmd.spawn()?;
    let (tx, rx) = mpsc::sync_channel(OUTPUT_CHANNEL_CHUNKS);
    if let Some(out) = child.stdout.take() {
        spawn_reader(out, tx.clone());
    }
    if let Some(err) = child.stderr.take() {
        spawn_reader(err, tx);
    }
    Ok(RunningCommand {
        timeout: region.timeout_secs.map(Duration::from_secs),
        region,
        group: Some(ProcGroup(child.id())),
        child,
        start: Instant::now(),
        rx,
        out: Vec::new(),
        dirty: true, // paint the (styled, cleared) box at once, even if silent
        exited: None,
    })
}

impl RunningCommand {
    /// Collect available output without blocking; enforce the timeout. Returns
    /// `Some((success, output))` once the child is gone and its output drained
    /// (pipes at EOF, or `DRAIN_GRACE` passed), `None` while still running.
    fn poll(&mut self) -> Option<(bool, Vec<u8>)> {
        let (line_cap, keep) = output_caps(self.region.w as usize, self.region.h as usize);
        let mut disconnected = false;
        for _ in 0..OUTPUT_CHUNKS_PER_TICK {
            match self.rx.try_recv() {
                Ok(chunk) => {
                    append_output(&mut self.out, &chunk, line_cap, keep);
                    self.dirty = true;
                }
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => {
                    disconnected = true;
                    break;
                }
            }
        }
        if self.exited.is_none() {
            // Exit first: a command that finished in time but is only noticed
            // after its deadline (the loop polls every ~30 ms) did not time out.
            let done = exited_unreaped(&mut self.child);
            let timed_out = !done && self.timeout.is_some_and(|t| self.start.elapsed() > t);
            if !done && !timed_out {
                return None;
            }
            // Kill the whole tree (on timeout, or anything the command left
            // running) while the leader is still unreaped, then reap it —
            // without blocking if it could not be signalled.
            self.group = None;
            // On unix the group kill already got it; this also tells us whether
            // the leader may be signalled at all (EPERM for a `sudo` leader).
            let denied = timed_out
                && matches!(self.child.kill(), Err(e) if e.kind() == io::ErrorKind::PermissionDenied);
            let status = reap_without_blocking(&mut self.child);
            let success = status.is_some_and(|s| s.success()) && !timed_out;
            self.exited = Some((success, timed_out, status.is_none() && denied, Instant::now()));
        }
        let (success, timed_out, unstoppable, at) = self.exited?;
        if !disconnected && at.elapsed() < DRAIN_GRACE {
            return None;
        }
        let mut out = std::mem::take(&mut self.out);
        if timed_out {
            out.extend_from_slice(b"\n[timed out]");
        }
        if unstoppable {
            out.extend_from_slice(b"\n[could not stop the command]");
        }
        Some((success, out))
    }
}

/// A command region's play-time state on the current frame.
enum CommandRun {
    Running(RunningCommand),
    Finished { region: CommandRegion, success: bool, out: Vec<u8> },
}

impl CommandRun {
    fn region(&self) -> &CommandRegion {
        match self {
            CommandRun::Running(rc) => &rc.region,
            CommandRun::Finished { region, .. } => region,
        }
    }
}

/// Whether `region` is active on `frame`.
fn covers(region: &CommandRegion, frame: usize) -> bool {
    frame >= region.start_frame && frame < region.end_frame
}

/// Whether a command for `region` keeps running (or keeps its finished output)
/// when playback moves from `from` to `to`: only while both lie in its frames,
/// so a command runs once per entry into its region. Each command on a slide
/// is decided on its own.
fn keeps_command(region: &CommandRegion, from: usize, to: usize) -> bool {
    covers(region, from) && covers(region, to)
}

/// State for a loop that is currently auto-playing. A flat deck has at most one
/// active loop at a time (loops never overlap or nest).
struct LoopPlay {
    region: LoopRegion,
    /// Direction and pass count (see [`loop_step`]).
    state: LoopState,
    /// When the next frame should be shown.
    deadline: Instant,
}

/// Where a running loop is in its passes.
#[derive(Clone, Copy, Debug, PartialEq)]
struct LoopState {
    /// Current sweep direction (only meaningful when `region.bounce`).
    forward: bool,
    /// Completed passes so far (used to stop after `region.count`).
    iterations: usize,
    /// Whether playback has been on `start_frame` since the loop was entered:
    /// only then is a return to it a *full* pass. Entering mid-loop (← from the
    /// slide after it, a ±10 jump) first plays on to the start uncounted.
    seen_start: bool,
}

impl LoopState {
    /// The state of a loop entered on `frame`.
    fn entering(region: &LoopRegion, frame: usize) -> Self {
        LoopState { forward: true, iterations: 0, seen_start: frame == region.start_frame }
    }
}

/// One loop tick from `current`: the frame to show next and the new state, or
/// `None` once a finite loop has played `count` full passes (the caller then
/// continues just past the loop). Stepping is [`loop_next`]'s.
fn loop_step(region: &LoopRegion, current: usize, state: LoopState) -> Option<(usize, LoopState)> {
    let (next, forward, completed) =
        loop_next(region.start_frame, region.end_frame, current, state.forward, region.bounce);
    let iterations = state.iterations + usize::from(completed && state.seen_start);
    if region.count != 0 && iterations >= region.count {
        return None;
    }
    Some((next, LoopState { forward, iterations, seen_start: state.seen_start || completed }))
}

pub struct Player {
    presentation: PlayablePresentation,
    current_frame: usize,
    grid: Vec<Vec<Cell>>,
    fullscreen: bool,
    /// One entry per command region covering the current frame: running, or
    /// finished with its output (repainted when the grid is rebuilt within its
    /// frames). Dropping a running one kills it.
    commands: Vec<CommandRun>,
    loop_play: Option<LoopPlay>,
    /// The span of a loop just broken out of (or that finished its count): it is
    /// not re-armed until playback has left that span — so a press that moves
    /// nowhere at a deck edge (or steps within the span) cannot restart it.
    loop_exclude: Option<(usize, usize)>,
    /// When the deck should auto-advance because an auto-play animation covers
    /// the current frame boundary (and no loop is driving). `None` = wait for a
    /// keypress. Loops, when active, drive advancement instead (see `loop_play`).
    auto_deadline: Option<Instant>,
}

impl Player {
    pub fn new(presentation: PlayablePresentation) -> Self {
        let w = presentation.contract.width as usize;
        let h = presentation.contract.height as usize;
        Self {
            presentation,
            current_frame: 0,
            grid: vec![vec![Cell::default(); w]; h],
            fullscreen: false,
            commands: Vec::new(),
            loop_play: None,
            loop_exclude: None,
            auto_deadline: None,
        }
    }

    /// Play the presentation in the terminal.
    ///
    /// Sets up the terminal, enters the event loop, and restores the terminal
    /// on exit (even on error or panic, via `term::TerminalGuard`).
    pub fn play(&mut self) -> Result<()> {
        // Checked before raw mode: the run loop starts by applying frame 0.
        if self.presentation.frames.is_empty() {
            bail!("Presentation has no frames");
        }
        let (term_w, term_h) = terminal::size()?;
        let (need_w, need_h) = needed_terminal(&self.presentation.contract);
        if u32::from(term_w) < need_w || u32::from(term_h) < need_h {
            bail!("Terminal too small: need {need_w}x{need_h}, have {term_w}x{term_h}");
        }

        let mut stdout = io::stdout();
        // Restores the terminal on return and on panic.
        let _term = crate::term::TerminalGuard::enter(false)?;
        let _signals = stop_signals::Guard::install();
        let result = self.run_loop(&mut stdout);
        self.kill_running(); // also on error: never leave a command running
        result
    }

    // -----------------------------------------------------------------------
    // Event loop
    // -----------------------------------------------------------------------

    fn run_loop(&mut self, stdout: &mut io::Stdout) -> Result<()> {
        self.apply_frame(0)?;
        self.redraw_all(stdout)?;
        self.maybe_start_command(stdout)?;
        // Frame 0 may itself sit inside a loop, or under an auto-play animation.
        self.arm_loop(None);
        self.schedule_auto();

        loop {
            if stop_signals::requested() {
                self.kill_running();
                bail!("stopped by a signal");
            }
            // Drive any running command: drain output, repaint, finalize on exit.
            if self.any_command_running() {
                self.service_commands(stdout)?;
            }

            // Poll briefly while a command runs so output streams in; otherwise
            // wait longer (the loop is idle until the next keypress). While a
            // loop auto-plays, never wait past its next-frame deadline.
            let mut poll = if self.any_command_running() {
                Duration::from_millis(30)
            } else {
                Duration::from_millis(200)
            };
            if let Some(lp) = &self.loop_play {
                poll = poll.min(lp.deadline.saturating_duration_since(Instant::now()));
            }
            if let Some(dl) = self.auto_deadline {
                poll = poll.min(dl.saturating_duration_since(Instant::now()));
            }

            if !event::poll(poll)? {
                // No key arrived — advance on whichever timer elapsed. A loop, if
                // active, drives playback; otherwise an auto-play animation does.
                let now = Instant::now();
                if self.loop_play.as_ref().is_some_and(|lp| now >= lp.deadline) {
                    self.loop_tick(stdout)?;
                } else if self.auto_deadline.is_some_and(|dl| now >= dl) {
                    self.auto_tick(stdout)?;
                }
                continue;
            }

            let ev = event::read()?;
            match ev {
                // Act on presses only: a release (every key on Windows) would
                // repeat the step, toggle fullscreen back, or quit after Esc.
                event::Event::Key(_) if crate::term::key_press(&ev).is_none() => {}
                event::Event::Key(key) => {
                    use event::KeyCode::*;
                    match key.code {
                        // Quit also stops any running binary.
                        Char('q') => {
                            self.kill_running();
                            break;
                        }
                        // Esc leaves fullscreen first; otherwise it quits.
                        Esc => {
                            if self.fullscreen {
                                self.fullscreen = false;
                                self.redraw_all(stdout)?;
                            } else {
                                self.kill_running();
                                break;
                            }
                        }
                        // Shift+→ / Shift+← jump FRAMES_PER_JUMP frames at once
                        // (clamped), tearing down any loop — a quick coarse scrub.
                        Right if key.modifiers.contains(event::KeyModifiers::SHIFT) => {
                            let left = self.leave_loop();
                            let last = self.presentation.frames.len().saturating_sub(1);
                            let target = (self.current_frame + FRAMES_PER_JUMP).min(last);
                            self.nav_to(target, stdout)?;
                            self.arm_loop(left);
                        }
                        Left if key.modifiers.contains(event::KeyModifiers::SHIFT) => {
                            let left = self.leave_loop();
                            let target = self.current_frame.saturating_sub(FRAMES_PER_JUMP);
                            self.nav_to(target, stdout)?;
                            self.arm_loop(left);
                        }
                        // Navigation always moves on — a slow command (or a
                        // loop) can never trap the deck; it stops a running
                        // binary unless the step stays in its region. Inside a loop, → breaks out to the first
                        // frame after it and ← to the first frame before it.
                        // On an auto-play animation (no loop), an arrow skips the
                        // whole animation: → to the first frame past the last-
                        // ending overlapping span, ← to the slide before the
                        // earliest-starting one.
                        // PageDown/PageUp (what presentation clickers send) act
                        // exactly like →/←, as in the web viewer.
                        Right | Char(' ') | Enter | PageDown => {
                            if let Some(lp) = self.loop_play.take() {
                                let last = self.presentation.frames.len().saturating_sub(1);
                                let target = lp.region.end_frame.min(last);
                                self.nav_to(target, stdout)?;
                                self.arm_loop(Some(span(&lp.region)));
                            } else if let Some((_, hi)) = self.animation_cluster(self.current_frame) {
                                let last = self.presentation.frames.len().saturating_sub(1);
                                self.nav_to(hi.min(last), stdout)?;
                                self.arm_loop(None);
                            } else {
                                self.nav_forward(stdout)?;
                                self.arm_loop(None);
                            }
                        }
                        Left | PageUp => {
                            if let Some(lp) = self.loop_play.take() {
                                let target = lp.region.start_frame.saturating_sub(1);
                                self.nav_to(target, stdout)?;
                                self.arm_loop(Some(span(&lp.region)));
                            } else if let Some((lo, _)) = self.animation_cluster(self.current_frame) {
                                self.nav_to(lo.saturating_sub(1), stdout)?;
                                self.arm_loop(None);
                            } else {
                                self.nav_back(stdout)?;
                                self.arm_loop(None);
                            }
                        }
                        Home => {
                            let left = self.leave_loop();
                            self.nav_to(0, stdout)?;
                            self.arm_loop(left);
                        }
                        End => {
                            let left = self.leave_loop();
                            let last = self.presentation.frames.len().saturating_sub(1);
                            self.nav_to(last, stdout)?;
                            self.arm_loop(left);
                        }
                        // Toggle "no bars" fullscreen: hide the menu/status bars
                        // and give the canvas the whole screen.
                        Char('f') => {
                            self.fullscreen = !self.fullscreen;
                            self.redraw_all(stdout)?;
                        }
                        _ => {}
                    }
                    // After a navigation, re-arm the auto-play timer for the
                    // frame we landed on (disarmed while a loop drives playback).
                    // Any other key (fullscreen, an unmapped one) leaves the
                    // time already waited alone, as the web viewer does.
                    if navigates(key.code) {
                        self.schedule_auto();
                    }
                }
                event::Event::Resize(_, _) => {
                    self.redraw_all(stdout)?;
                }
                _ => {}
            }
        }

        Ok(())
    }

    // -----------------------------------------------------------------------
    // Loop playback
    // -----------------------------------------------------------------------

    /// The loop region active on `frame`, if any.
    fn loop_region_for(&self, frame: usize) -> Option<LoopRegion> {
        self.presentation
            .loops
            .iter()
            .find(|l| frame >= l.start_frame && frame < l.end_frame)
            .cloned()
    }

    /// Begin auto-playing the loop on the current frame, if there is one and one
    /// is not already running. `exclude` (given right after a break-out or a
    /// finished count) records the span just left in `loop_exclude`, which keeps
    /// that loop from restarting while playback stays inside its span (e.g. a
    /// loop pinned to the deck's first or last frame); it clears once the
    /// current frame is outside the span. Every navigation path calls this.
    fn arm_loop(&mut self, exclude: Option<(usize, usize)>) {
        if exclude.is_some() {
            self.loop_exclude = exclude;
        }
        let f = self.current_frame;
        if self.loop_exclude.is_some_and(|(s, e)| f < s || f >= e) {
            self.loop_exclude = None;
        }
        if self.loop_play.is_some() {
            return;
        }
        if let Some(region) = self.loop_region_for(self.current_frame) {
            if self.loop_exclude == Some(span(&region)) {
                return;
            }
            let state = LoopState::entering(&region, self.current_frame);
            let d = self.loop_wait(&region, self.current_frame, state);
            let deadline = Instant::now() + Duration::from_millis(d.max(1));
            self.loop_play = Some(LoopPlay { region, state, deadline });
        }
    }

    fn stop_loop(&mut self) {
        self.loop_play = None;
    }

    /// Tear down the running loop (if any) and return its span, for the jump
    /// keys to pass to [`arm_loop`](Self::arm_loop) so landing back inside the
    /// loop just left does not restart it.
    fn leave_loop(&mut self) -> Option<(usize, usize)> {
        self.loop_play.take().map(|lp| span(&lp.region))
    }

    // -----------------------------------------------------------------------
    // Animation auto-play
    // -----------------------------------------------------------------------

    /// The effective auto-advance delay across the boundary leaving `from` in
    /// the given direction (forward = `from`→`from+1`, backward = `from`→`from-1`),
    /// taken as the **minimum** `delay_ms` over every auto-play animation for
    /// which that boundary is *internal* (both frames lie within its span).
    /// `None` when no auto-play animation covers the boundary.
    fn auto_advance_delay(&self, from: usize, forward: bool) -> Option<u64> {
        // The boundary is between `lo` and `lo + 1`.
        let lo = if forward { from } else { from.checked_sub(1)? };
        self.presentation
            .animations
            .iter()
            .filter(|a| a.auto_play && a.start_frame <= lo && lo + 1 < a.end_frame)
            .map(|a| a.delay_ms)
            .min()
    }

    /// How long a running loop waits on `frame` before its next step: the
    /// auto-play delay ([`Self::auto_advance_delay`]) of the boundary that step
    /// actually crosses — at a bounce turnaround that is back the way it came —
    /// else the loop's own `delay_ms` (a wrap, a one-frame loop, or a step no
    /// auto-play animation covers).
    fn loop_wait(&self, region: &LoopRegion, frame: usize, state: LoopState) -> u64 {
        let (after, _, _) =
            loop_next(region.start_frame, region.end_frame, frame, state.forward, region.bounce);
        let step = if after == frame + 1 {
            self.auto_advance_delay(frame, true)
        } else if region.bounce && after + 1 == frame {
            self.auto_advance_delay(frame, false)
        } else {
            None
        };
        step.unwrap_or(region.delay_ms)
    }

    /// The merged span `[lo, hi)` of the connected cluster of **auto-play**
    /// animations covering `frame` — every auto-play animation reachable, by
    /// overlap, from one that covers `frame`. `None` when no auto-play animation
    /// covers `frame`. Arrow navigation uses this to skip a whole animation (or a
    /// chain of overlapping ones) in one keypress: `→` jumps to `hi` (the first
    /// frame past the last-ending span), `←` to `lo - 1` (the slide before the
    /// earliest-starting span).
    fn animation_cluster(&self, frame: usize) -> Option<(usize, usize)> {
        let auto: Vec<(usize, usize)> = self
            .presentation
            .animations
            .iter()
            .filter(|a| a.auto_play)
            .map(|a| (a.start_frame, a.end_frame))
            .collect();
        // Seed the cluster with every span covering `frame`.
        let mut lo = usize::MAX;
        let mut hi = 0usize;
        for &(s, e) in &auto {
            if s <= frame && frame < e {
                lo = lo.min(s);
                hi = hi.max(e);
            }
        }
        if lo == usize::MAX {
            return None;
        }
        // Grow across overlaps (`s < hi && lo < e`) until the cluster is closed.
        loop {
            let mut grew = false;
            for &(s, e) in &auto {
                if s < hi && lo < e {
                    if s < lo {
                        lo = s;
                        grew = true;
                    }
                    if e > hi {
                        hi = e;
                        grew = true;
                    }
                }
            }
            if !grew {
                break;
            }
        }
        Some((lo, hi))
    }

    /// The per-frame auto-advance delay for `frame`: the **minimum** `delay_ms`
    /// over every `AutoAdvanceRegion` covering it, provided a next frame exists to
    /// advance to (the last frame never auto-advances). `None` when no region
    /// covers the frame.
    fn frame_auto_advance_delay(&self, frame: usize) -> Option<u64> {
        if frame + 1 >= self.presentation.frames.len() {
            return None;
        }
        self.presentation
            .auto_advances
            .iter()
            .filter(|a| a.start_frame <= frame && frame < a.end_frame)
            .map(|a| a.delay_ms)
            .min()
    }

    /// The effective auto-advance delay leaving `frame` forward, combining the
    /// auto-play **animation** boundary delay and the per-frame **auto-advance**
    /// marker — whichever is smaller. `None` when neither applies (wait for a
    /// keypress).
    fn effective_auto_delay(&self, frame: usize) -> Option<u64> {
        [
            self.auto_advance_delay(frame, true),
            self.frame_auto_advance_delay(frame),
        ]
        .into_iter()
        .flatten()
        .min()
    }

    /// (Re)arm the non-loop auto-advance timer for the current frame. A loop, if
    /// active, drives advancement itself, so the timer stays disarmed while one
    /// is running. Both auto-play animations and per-frame auto-advance markers
    /// feed the deadline (see `effective_auto_delay`).
    fn schedule_auto(&mut self) {
        self.auto_deadline = if self.loop_play.is_some() {
            None
        } else {
            self.effective_auto_delay(self.current_frame)
                .map(|d| Instant::now() + Duration::from_millis(d.max(1)))
        };
    }

    /// Advance one frame because an auto-play animation's delay elapsed (no loop
    /// active). Re-arms the loop (in case we stepped into one) and the animation
    /// timer for the new frame.
    fn auto_tick(&mut self, stdout: &mut io::Stdout) -> Result<()> {
        let last = self.presentation.frames.len().saturating_sub(1);
        if self.current_frame < last {
            self.nav_forward(stdout)?;
            self.arm_loop(None);
        }
        self.schedule_auto();
        Ok(())
    }

    /// Advance the active loop by one frame (called when its delay elapses).
    fn loop_tick(&mut self, stdout: &mut io::Stdout) -> Result<()> {
        let Some(lp) = self.loop_play.as_ref() else {
            return Ok(());
        };
        let region = lp.region.clone();
        let step = loop_step(&region, self.current_frame, lp.state);

        // A finite loop that has played its full count stops and continues the
        // deck just past the loop (an infinite loop, count 0, never gets here).
        let Some((next, state)) = step else {
            self.stop_loop();
            let last = self.presentation.frames.len().saturating_sub(1);
            let target = region.end_frame.min(last);
            self.nav_to(target, stdout)?;
            self.arm_loop(Some(span(&region)));
            // The landing frame may auto-advance (marker or auto-play animation).
            self.schedule_auto();
            return Ok(());
        };

        // Otherwise show the next loop frame. A loop step can jump backward
        // (bounce / restart), so rebuild the grid from scratch rather than
        // diffing — same path as `nav_back`.
        self.go_to_frame(next, true, stdout)?;
        let next_delay = self.loop_wait(&region, next, state);
        if let Some(lp) = self.loop_play.as_mut() {
            lp.state = state;
            lp.deadline = Instant::now() + Duration::from_millis(next_delay.max(1));
        }
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Navigation
    // -----------------------------------------------------------------------

    fn nav_forward(&mut self, stdout: &mut io::Stdout) -> Result<()> {
        let last = self.presentation.frames.len().saturating_sub(1);
        if self.current_frame >= last {
            return Ok(());
        }
        self.go_to_frame(self.current_frame + 1, false, stdout)
    }

    fn nav_back(&mut self, stdout: &mut io::Stdout) -> Result<()> {
        if self.current_frame == 0 {
            return Ok(());
        }
        self.go_to_frame(self.current_frame - 1, true, stdout)
    }

    fn nav_to(&mut self, target: usize, stdout: &mut io::Stdout) -> Result<()> {
        self.go_to_frame(target, true, stdout)
    }

    /// Show frame `target` — every navigation path and loop step goes through
    /// here. `rebuild` replays the grid from scratch (needed for any backward
    /// step); a forward step diffs unless a command's output is overlaid. A
    /// command keeps running (or keeps its finished output) while playback stays
    /// in its region ([`keeps_command`], decided per command) and is repainted
    /// over the new frame; the others are stopped, and every other command
    /// region covering the target starts.
    fn go_to_frame(&mut self, target: usize, rebuild: bool, stdout: &mut io::Stdout) -> Result<()> {
        let from = self.current_frame;
        let overlaid = !self.commands.is_empty() || !self.regions_for(from).is_empty();
        self.commands.retain(|c| keeps_command(c.region(), from, target));
        self.current_frame = target;
        if rebuild || overlaid {
            self.rebuild_grid(target)?;
            self.render_full(stdout)?;
        } else {
            self.apply_frame(target)?;
            self.render_diff(stdout, target)?;
        }
        self.render_status(stdout)?;
        self.repaint_commands(stdout)?;
        self.maybe_start_command(stdout)
    }

    // -----------------------------------------------------------------------
    // Grid management
    // -----------------------------------------------------------------------

    fn apply_frame(&mut self, index: usize) -> Result<()> {
        // Same shape-preserving step as `grid_at`, so the grid stays w×h.
        self.presentation.frames[index].apply_to(&mut self.grid);
        Ok(())
    }

    fn rebuild_grid(&mut self, target: usize) -> Result<()> {
        // Full replay shares one implementation with the editor preview and the
        // test harness (see `PlayablePresentation::grid_at`).
        self.grid = self.presentation.grid_at(target);
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Terminal output
    // -----------------------------------------------------------------------

    /// Rows reserved above the canvas: one for the menu bar, or none in
    /// fullscreen ("no bars") mode where the canvas owns the whole screen.
    fn canvas_offset(&self) -> u16 {
        if self.fullscreen {
            0
        } else {
            CANVAS_OFFSET
        }
    }

    /// Clear and repaint everything for the current fullscreen state: the menu
    /// and status bars are drawn only when not in fullscreen (`render_status`
    /// self-guards on `fullscreen`; `render_menubar` is gated here).
    fn redraw_all(&self, stdout: &mut io::Stdout) -> Result<()> {
        queue!(stdout, terminal::Clear(terminal::ClearType::All))?;
        if !self.fullscreen {
            self.render_menubar(stdout)?;
        }
        self.render_full(stdout)?;
        self.render_status(stdout)?;
        Ok(())
    }

    fn render_menubar(&self, stdout: &mut io::Stdout) -> Result<()> {
        let items: &[&str] = &[
            "[←] prev",
            "[→][Space] next",
            "[⇧←][⇧→] ±10",
            "[Home] first",
            "[End] last",
            "[q][Esc] quit",
            "[f]ull",
        ];

        queue!(
            stdout,
            cursor::MoveTo(0, 0),
            terminal::Clear(terminal::ClearType::CurrentLine),
            style::Print(" "),
        )?;
        for (i, item) in items.iter().enumerate() {
            if i > 0 {
                queue!(stdout, style::Print("  "))?;
            }
            print_menu_item(stdout, item)?;
        }
        stdout.flush()?;
        Ok(())
    }

    fn render_full(&self, stdout: &mut io::Stdout) -> Result<()> {
        let offset = self.canvas_offset();
        for (y, row) in self.grid.iter().enumerate() {
            queue!(stdout, cursor::MoveTo(0, y as u16 + offset))?;
            for cell in row {
                let cs = to_content_style(&cell.style);
                queue!(
                    stdout,
                    style::PrintStyledContent(style::StyledContent::new(cs, cell.printable()))
                )?;
            }
        }
        stdout.flush()?;
        Ok(())
    }

    fn render_diff(&self, stdout: &mut io::Stdout, frame_index: usize) -> Result<()> {
        match &self.presentation.frames[frame_index] {
            Frame::Diff { changes } => {
                let c = &self.presentation.contract;
                // Skip what `apply_to` skipped, so nothing lands off the canvas.
                for change in changes.iter().filter(|ch| ch.x < c.width && ch.y < c.height) {
                    let cs = to_content_style(&change.cell.style);
                    queue!(
                        stdout,
                        cursor::MoveTo(change.x, change.y + self.canvas_offset()),
                        style::PrintStyledContent(style::StyledContent::new(cs, change.cell.printable())),
                    )?;
                }
                stdout.flush()?;
            }
            Frame::Full { .. } => {
                // Full frame — just re-render everything.
                self.render_full(stdout)?;
            }
        }
        Ok(())
    }

    fn render_status(&self, stdout: &mut io::Stdout) -> Result<()> {
        // Fullscreen ("no bars") owns the whole screen — no footer at all. Guard
        // here so every caller (navigation, loop steps, full repaint) honours it.
        if self.fullscreen {
            return Ok(());
        }
        let status_y = self.presentation.contract.height.saturating_add(self.canvas_offset());
        let (_, term_h) = terminal::size()?;
        if status_y >= term_h {
            return Ok(()); // No room for status bar.
        }

        let total = self.presentation.frames.len();
        let status = format!(
            " Frame {}/{} | \u{2190}\u{2192}: navigate | q: quit ",
            self.current_frame + 1,
            total,
        );

        let mut cs = style::ContentStyle::default();
        cs.attributes.set(style::Attribute::Dim);

        queue!(
            stdout,
            cursor::MoveTo(0, status_y),
            style::PrintStyledContent(style::StyledContent::new(cs, status)),
        )?;
        stdout.flush()?;
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Command execution
    // -----------------------------------------------------------------------

    /// Every command spec active on `frame` — a slide may hold several boxes.
    fn regions_for(&self, frame: usize) -> Vec<CommandRegion> {
        self.presentation.commands.iter().filter(|c| covers(c, frame)).cloned().collect()
    }

    fn any_command_running(&self) -> bool {
        self.commands.iter().any(|c| matches!(c, CommandRun::Running(_)))
    }

    /// Start every command region on the current frame that is not already
    /// running or finished here (a kept one is not re-run).
    fn maybe_start_command(&mut self, stdout: &mut io::Stdout) -> Result<()> {
        let mut have: Vec<CommandRegion> = self.commands.iter().map(|c| c.region().clone()).collect();
        for region in self.regions_for(self.current_frame) {
            match have.iter().position(|h| *h == region) {
                Some(i) => {
                    have.remove(i);
                }
                None => self.start_command(region, stdout)?,
            }
        }
        Ok(())
    }

    /// Spawn the binary with piped stdio and background readers. A spawn failure
    /// is rendered as an error in the box rather than crashing the player.
    fn start_command(&mut self, region: CommandRegion, stdout: &mut io::Stdout) -> Result<()> {
        match spawn_command(region.clone()) {
            Ok(rc) => self.commands.push(CommandRun::Running(rc)),
            Err(e) => {
                let msg = format!("failed to run '{}': {e}", region.command);
                let painted = self.paint_command(stdout, &region, Some(false), msg.as_bytes());
                self.commands.push(CommandRun::Finished { region, success: false, out: msg.into_bytes() });
                painted?;
            }
        }
        Ok(())
    }

    /// Repaint the kept commands' output (live, or finished with its ✓ / ✗)
    /// over a freshly rebuilt grid.
    fn repaint_commands(&mut self, stdout: &mut io::Stdout) -> Result<()> {
        let cmds = std::mem::take(&mut self.commands);
        let mut res = Ok(());
        for c in &cmds {
            let (region, status, out) = match c {
                CommandRun::Running(rc) => (&rc.region, None, &rc.out),
                CommandRun::Finished { region, success, out } => (region, Some(*success), out),
            };
            if res.is_ok() {
                res = self.paint_command(stdout, region, status, out);
            }
        }
        self.commands = cmds;
        res
    }

    /// Stop every running binary and everything it started, and reap them;
    /// forget finished ones' output.
    fn kill_running(&mut self) {
        self.commands.clear();
    }

    /// Service the running commands: drain output, repaint, and on exit (or
    /// timeout) finalize each with a ✓ / ✗ indicator. Called once per loop tick.
    fn service_commands(&mut self, stdout: &mut io::Stdout) -> Result<()> {
        let mut cmds = std::mem::take(&mut self.commands);
        let mut res = Ok(());
        for c in &mut cmds {
            let CommandRun::Running(rc) = c else { continue };
            let region = rc.region.clone();
            match rc.poll() {
                // Repaint only when new output arrived.
                None if rc.dirty => {
                    rc.dirty = false;
                    if res.is_ok() {
                        res = self.paint_command(stdout, &region, None, &rc.out);
                    }
                }
                None => {}
                Some((success, out)) => {
                    if res.is_ok() {
                        res = self.paint_command(stdout, &region, Some(success), &out);
                    }
                    *c = CommandRun::Finished { region, success, out };
                }
            }
        }
        // Put them back even on a paint error, so they are still killed later.
        self.commands = cmds;
        res
    }

    /// Paint captured output into the box interior, tailing to the last lines
    /// that fit. `status` is `None` while running, `Some(true/false)` on exit
    /// (drawing a ✓ / ✗ on the top edge).
    fn paint_command(
        &mut self,
        stdout: &mut io::Stdout,
        region: &CommandRegion,
        status: Option<bool>,
        out: &[u8],
    ) -> Result<()> {
        let w = region.w as usize;
        let h = region.h as usize;
        let rows = layout_output(out, w, h);

        let gh = self.grid.len();
        let gw = if gh > 0 { self.grid[0].len() } else { 0 };

        for (row, line) in rows.iter().enumerate() {
            let gy = region.y as usize + row;
            if gy >= gh {
                break;
            }
            for (col, ch) in line.chars().enumerate() {
                let gx = region.x as usize + col;
                if gx >= gw {
                    break;
                }
                self.grid[gy][gx] = Cell {
                    ch,
                    style: region.style.clone(),
                };
            }
        }

        if let Some(success) = status {
            let sx = region.status_x as usize;
            let sy = region.status_y as usize;
            if sy < gh && sx < gw {
                let (ch, color) = if success {
                    ('✓', NamedColor::Green)
                } else {
                    ('✗', NamedColor::Red)
                };
                self.grid[sy][sx] = Cell {
                    ch,
                    style: Style {
                        fg: Some(Color::Named(color)),
                        bold: true,
                        ..Style::default()
                    },
                };
            }
        }

        self.render_region(stdout, region.x, region.y, region.w, region.h)?;
        if status.is_some() {
            self.render_region(stdout, region.status_x, region.status_y, 1, 1)?;
        }
        Ok(())
    }

    /// Render a rectangular slice of the grid to the terminal.
    fn render_region(&self, stdout: &mut io::Stdout, x: u16, y: u16, w: u16, h: u16) -> Result<()> {
        let gh = self.grid.len();
        for row in 0..h {
            let gy = (y + row) as usize;
            if gy >= gh {
                break;
            }
            let gw = self.grid[gy].len();
            queue!(stdout, cursor::MoveTo(x, y + row + self.canvas_offset()))?;
            for col in 0..w {
                let gx = (x + col) as usize;
                if gx >= gw {
                    break;
                }
                let cell = &self.grid[gy][gx];
                let cs = to_content_style(&cell.style);
                queue!(
                    stdout,
                    style::PrintStyledContent(style::StyledContent::new(cs, cell.printable()))
                )?;
            }
        }
        stdout.flush()?;
        Ok(())
    }
}

/// The `(start, end)` span of a loop region, used as an identity key when
/// suppressing an immediate re-arm after breaking out.
fn span(r: &LoopRegion) -> (usize, usize) {
    (r.start_frame, r.end_frame)
}

/// Whether a key press navigates (the arms of `run_loop` that move the deck):
/// only those re-arm the auto-advance timer.
fn navigates(code: event::KeyCode) -> bool {
    use event::KeyCode::*;
    matches!(code, Right | Left | Char(' ') | Enter | PageDown | PageUp | Home | End)
}

/// Compute the next frame of a loop given the current position and direction.
///
/// Returns `(next_frame, next_forward, completed_iteration)`:
/// - **non-bounce**: plays `start..end` forward, then jumps back to `start`;
///   each wrap to `start` counts as one completed iteration.
/// - **bounce**: plays forward to `end-1`, then backward to `start` (a triangle
///   wave, endpoints shown once per turn); returning to `start` completes one
///   iteration.
///
/// A single-frame range (`end == start + 1`) has nowhere to move, so it stays
/// put and completes an iteration on every tick.
fn loop_next(
    start: usize,
    end: usize,
    current: usize,
    forward: bool,
    bounce: bool,
) -> (usize, bool, bool) {
    let last = end.saturating_sub(1);
    let (next, next_forward) = if !bounce {
        // Forward sweep; wrap from the last frame back to the first.
        if current < last {
            (current + 1, true)
        } else {
            (start, true)
        }
    } else if last == start {
        // Single-frame range: nowhere to move.
        (start, true)
    } else if forward {
        if current < last {
            (current + 1, true)
        } else {
            (last - 1, false) // turn around at the top
        }
    } else if current > start {
        (current - 1, false)
    } else {
        (start + 1, true) // turn around at the bottom
    };
    // Landing back on the first frame (a wrap, or the bottom of a bounce) is one
    // completed pass.
    (next, next_forward, next == start)
}

/// Read a child pipe to EOF on a background thread, forwarding chunks. The
/// thread exits on EOF, read error, or when the receiver is dropped.
fn spawn_reader<R: Read + Send + 'static>(mut r: R, tx: SyncSender<Vec<u8>>) {
    thread::spawn(move || {
        let mut buf = [0u8; 4096];
        loop {
            match r.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    if tx.send(buf[..n].to_vec()).is_err() {
                        break;
                    }
                }
            }
        }
    });
}

/// Lay captured bytes out into a `w`×`h` text region: strip ANSI, keep the last
/// `h` lines (tail), and clip/pad each line to exactly `w` columns. Each char
/// becomes one grid cell, so every char is first made exactly one terminal
/// column wide ([`narrow_char`]). Pure, so the player's output rendering can be
/// tested without a terminal.
pub fn layout_output(bytes: &[u8], w: usize, h: usize) -> Vec<String> {
    let text = strip_ansi(bytes);
    let lines: Vec<&str> = text.lines().collect();
    let first = lines.len().saturating_sub(h);
    (0..h)
        .map(|row| {
            let line = lines.get(first + row).copied().unwrap_or("");
            let mut s: String = line.chars().filter_map(narrow_char).take(w).collect();
            let len = s.chars().count();
            if len < w {
                s.extend(std::iter::repeat(' ').take(w - len));
            }
            s
        })
        .collect()
}

/// Strip escape sequences and control characters so output paints cleanly into
/// the fixed cell grid: no byte of a sequence or control char (BEL, BS, SO, CR,
/// DEL, C1, ...) may reach a cell, since cells are printed to the terminal raw.
/// Tabs become a space; newlines are kept for the caller's line split.
fn strip_ansi(bytes: &[u8]) -> String {
    let s = String::from_utf8_lossy(bytes);
    let chars: Vec<char> = s.chars().collect();
    let mut out = String::with_capacity(s.len());
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        i += 1;
        // 7-bit introducers (ESC + char) and their 8-bit C1 equivalents.
        let intro = match c {
            // (A control char after ESC is not consumed: it is handled on its own.)
            '\x1b' if i < chars.len() && !chars[i].is_control() => {
                i += 1;
                chars[i - 1]
            }
            '\u{9b}' => '[',
            '\u{90}' => 'P',
            '\u{98}' => 'X',
            '\u{9d}' => ']',
            '\u{9e}' => '^',
            '\u{9f}' => '_',
            '\t' => {
                out.push(' ');
                continue;
            }
            '\n' => {
                out.push('\n');
                continue;
            }
            c if c.is_control() => continue,
            c => {
                out.push(c);
                continue;
            }
        };
        match intro {
            // CSI: parameters/intermediates up to a final byte in @..~.
            // A control char (newline, ESC, ...) cuts a malformed one short.
            '[' => {
                while i < chars.len() && !chars[i].is_control() {
                    i += 1;
                    if ('@'..='~').contains(&chars[i - 1]) {
                        break;
                    }
                }
            }
            // OSC/DCS/SOS/PM/APC strings: up to BEL or ST (ESC \ or U+9C). A
            // newline or any other ESC also ends the string (and is kept).
            ']' | 'P' | 'X' | '^' | '_' => {
                while i < chars.len() {
                    match chars[i] {
                        '\n' => break, // never swallow a line break
                        '\x07' | '\u{9c}' => {
                            i += 1;
                            break;
                        }
                        '\x1b' => {
                            if chars.get(i + 1) == Some(&'\\') {
                                i += 2;
                            }
                            break;
                        }
                        _ => i += 1,
                    }
                }
            }
            // nF sequences (e.g. ESC ( B): intermediates 0x20..0x2F, then a final.
            ' '..='/' => {
                while i < chars.len() && (' '..='/').contains(&chars[i]) {
                    i += 1;
                }
                if i < chars.len() && !chars[i].is_control() {
                    i += 1;
                }
            }
            _ => {} // two-character escape (ESC 7, ESC =, ...) or a lone ESC
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Style conversion
// ---------------------------------------------------------------------------

pub fn to_content_style(s: &Style) -> style::ContentStyle {
    let mut cs = style::ContentStyle::default();
    if let Some(fg) = &s.fg {
        cs.foreground_color = Some(to_ct_color(fg));
    }
    if let Some(bg) = &s.bg {
        cs.background_color = Some(to_ct_color(bg));
    }
    if s.bold {
        cs.attributes.set(style::Attribute::Bold);
    }
    if s.dim {
        cs.attributes.set(style::Attribute::Dim);
    }
    cs
}

pub fn to_ct_color(c: &Color) -> style::Color {
    match c {
        Color::Named(n) => match n {
            NamedColor::Black => style::Color::Black,
            NamedColor::Red => style::Color::Red,
            NamedColor::Green => style::Color::Green,
            NamedColor::Yellow => style::Color::Yellow,
            NamedColor::Blue => style::Color::Blue,
            NamedColor::Magenta => style::Color::Magenta,
            NamedColor::Cyan => style::Color::Cyan,
            NamedColor::White => style::Color::White,
        },
        Color::Rgb { r, g, b } => style::Color::Rgb {
            r: *r,
            g: *g,
            b: *b,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::{loop_next, loop_step, navigates, LoopState, Player};
    use crate::types::{
        AnimationRegion, AutoAdvanceRegion, Cell, Frame, PlayablePresentation, TerminalContract,
    };

    /// A player over `frames` blank frames carrying the given animation regions.
    fn player_with(frames: usize, animations: Vec<AnimationRegion>) -> Player {
        let pres = PlayablePresentation {
            contract: TerminalContract { width: 1, height: 1 },
            frames: (0..frames).map(|_| Frame::Full { cells: vec![vec![Cell::default()]] }).collect(),
            markers: Vec::new(),
            commands: Vec::new(),
            loops: Vec::new(),
            animations,
            auto_advances: Vec::new(),
        };
        Player::new(pres)
    }

    #[test]
    fn play_rejects_a_presentation_with_no_frames() {
        let err = player_with(0, Vec::new()).play().unwrap_err().to_string();
        assert!(err.contains("no frames"), "got: {err}");
    }

    /// A player over `frames` blank frames carrying the given auto-advance regions.
    fn player_with_auto(frames: usize, auto_advances: Vec<AutoAdvanceRegion>) -> Player {
        let pres = PlayablePresentation {
            contract: TerminalContract { width: 1, height: 1 },
            frames: (0..frames).map(|_| Frame::Full { cells: vec![vec![Cell::default()]] }).collect(),
            markers: Vec::new(),
            commands: Vec::new(),
            loops: Vec::new(),
            animations: Vec::new(),
            auto_advances,
        };
        Player::new(pres)
    }

    #[test]
    fn a_loop_left_by_break_out_stays_off_until_playback_leaves_its_span() {
        // Loop 5..10 pinned to the last frame of a 10-frame deck.
        let mut p = player_with(10, Vec::new());
        p.presentation.loops.push(crate::types::LoopRegion {
            start_frame: 5, end_frame: 10, delay_ms: 100, count: 0, bounce: true,
        });
        // → breaks out to end_frame.min(last) = 9, inside the loop: not re-armed.
        p.current_frame = 9;
        p.arm_loop(Some((5, 10)));
        assert!(p.loop_play.is_none());
        // Another → is a no-op at the last frame; re-arming must still not happen,
        // nor when stepping back within the span.
        p.arm_loop(None);
        assert!(p.loop_play.is_none());
        p.current_frame = 8;
        p.arm_loop(None);
        assert!(p.loop_play.is_none());
        // Leaving the span clears the exclusion; coming back in arms the loop.
        p.current_frame = 4;
        p.arm_loop(None);
        p.current_frame = 5;
        p.arm_loop(None);
        assert!(p.loop_play.is_some());
        // A jump key (End / Shift+→) leaving the running loop for its last
        // frame passes the torn-down span on, so it does not restart either.
        let left = p.leave_loop();
        p.current_frame = 9;
        p.arm_loop(left);
        assert!(p.loop_play.is_none());
    }

    fn aa(start: usize, end: usize, delay: u64) -> AutoAdvanceRegion {
        AutoAdvanceRegion { start_frame: start, end_frame: end, delay_ms: delay }
    }

    #[test]
    fn frame_auto_advance_delay_covers_its_range_but_not_the_last_frame() {
        // Region [1,3): frames 1 and 2 carry it. The last frame (frame 4 of
        // 0..=4) never auto-advances — there is nowhere to go.
        let p = player_with_auto(5, vec![aa(1, 3, 2000)]);
        assert_eq!(p.frame_auto_advance_delay(0), None); // before the range
        assert_eq!(p.frame_auto_advance_delay(1), Some(2000));
        assert_eq!(p.frame_auto_advance_delay(2), Some(2000));
        assert_eq!(p.frame_auto_advance_delay(3), None); // exclusive end
    }

    #[test]
    fn frame_auto_advance_delay_is_suppressed_on_the_final_frame() {
        // A marker spanning the whole deck still does nothing on the last frame.
        let p = player_with_auto(3, vec![aa(0, 3, 1000)]);
        assert_eq!(p.frame_auto_advance_delay(2), None); // last frame, no next
        assert_eq!(p.frame_auto_advance_delay(1), Some(1000));
    }

    #[test]
    fn frame_auto_advance_delay_takes_the_minimum_over_overlapping_markers() {
        let p = player_with_auto(6, vec![aa(0, 4, 5000), aa(2, 5, 1500)]);
        assert_eq!(p.frame_auto_advance_delay(1), Some(5000)); // only the first
        assert_eq!(p.frame_auto_advance_delay(3), Some(1500)); // both → min
    }

    #[test]
    fn effective_auto_delay_combines_animation_and_per_frame_markers() {
        // Frame 1: a 5s per-frame marker and a 3s auto-play animation both cover
        // the forward boundary; the faster one wins.
        let pres = PlayablePresentation {
            contract: TerminalContract { width: 1, height: 1 },
            frames: (0..5).map(|_| Frame::Full { cells: vec![vec![Cell::default()]] }).collect(),
            markers: Vec::new(),
            commands: Vec::new(),
            loops: Vec::new(),
            animations: vec![anim(0, 3, 3000)],
            auto_advances: vec![aa(0, 4, 5000)],
        };
        let p = Player::new(pres);
        assert_eq!(p.effective_auto_delay(1), Some(3000)); // min(5000, 3000)
        assert_eq!(p.effective_auto_delay(3), Some(5000)); // animation ended; marker only
    }

    fn anim(start: usize, end: usize, delay: u64) -> AnimationRegion {
        AnimationRegion { start_frame: start, end_frame: end, auto_play: true, delay_ms: delay }
    }

    #[test]
    fn only_navigation_keys_restart_the_auto_advance_timer() {
        use crossterm::event::KeyCode::*;
        for code in [Right, Left, Char(' '), Enter, PageDown, PageUp, Home, End] {
            assert!(navigates(code), "{code:?} navigates");
        }
        // Fullscreen, Esc (out of fullscreen), a stray key.
        for code in [Char('f'), Esc, Char('x'), Tab] {
            assert!(!navigates(code), "{code:?} leaves the timer alone");
        }
    }

    #[test]
    fn loop_wait_prices_the_step_actually_taken() {
        // Loop 5..9 over an auto-play animation 5..9 at 100 ms (loop 500 ms).
        let p = player_with(10, vec![anim(5, 9, 100)]);
        let region = |bounce| crate::types::LoopRegion {
            start_frame: 5, end_frame: 9, delay_ms: 500, count: 0, bounce,
        };
        let st = |forward| LoopState { forward, iterations: 0, seen_start: true };
        // Bounce: the turnarounds at both ends step back inside the animation.
        assert_eq!(p.loop_wait(&region(true), 8, st(true)), 100, "top turnaround");
        assert_eq!(p.loop_wait(&region(true), 5, st(false)), 100, "bottom turnaround");
        assert_eq!(p.loop_wait(&region(true), 6, st(true)), 100, "mid-sweep");
        // Without bounce, the wrap from the last frame to the start waits the
        // loop's own delay.
        assert_eq!(p.loop_wait(&region(false), 8, st(true)), 500, "wrap");
        assert_eq!(p.loop_wait(&region(false), 6, st(true)), 100);
    }

    #[test]
    fn auto_advance_delay_covers_only_internal_boundaries() {
        // Animation spans frames [2,5): internal forward boundaries are 2→3 and
        // 3→4 (both frames in span). 4→5 is NOT internal (5 is the exclusive end).
        let p = player_with(8, vec![anim(2, 5, 300)]);
        assert_eq!(p.auto_advance_delay(2, true), Some(300));
        assert_eq!(p.auto_advance_delay(3, true), Some(300));
        assert_eq!(p.auto_advance_delay(4, true), None); // boundary 4→5 not internal
        assert_eq!(p.auto_advance_delay(1, true), None); // before the span
    }

    #[test]
    fn auto_advance_delay_takes_the_minimum_over_overlapping_animations() {
        // Two auto-play animations overlap on the 4→5 boundary; the faster one
        // (200 ms) sets the pace there.
        let p = player_with(10, vec![anim(0, 6, 500), anim(3, 8, 200)]);
        assert_eq!(p.auto_advance_delay(1, true), Some(500)); // only the first covers 1→2
        assert_eq!(p.auto_advance_delay(4, true), Some(200)); // both cover 4→5 → min
        assert_eq!(p.auto_advance_delay(6, true), Some(200)); // only the second covers 6→7
    }

    #[test]
    fn auto_advance_delay_handles_backward_boundaries() {
        // Backward from frame 4 crosses 3→4, internal to [2,5).
        let p = player_with(8, vec![anim(2, 5, 300)]);
        assert_eq!(p.auto_advance_delay(4, false), Some(300));
        assert_eq!(p.auto_advance_delay(2, false), None); // boundary 1→2 not internal
        assert_eq!(p.auto_advance_delay(0, false), None); // no boundary below 0
    }

    #[test]
    fn auto_advance_delay_ignores_non_auto_play_animations() {
        let mut a = anim(2, 6, 300);
        a.auto_play = false;
        let p = player_with(8, vec![a]);
        assert_eq!(p.auto_advance_delay(3, true), None);
    }

    #[test]
    fn animation_cluster_spans_a_single_animation() {
        // Animation [2,5): every covered frame yields the same span; frames
        // outside (1 before, 5 = exclusive end) yield None.
        let p = player_with(8, vec![anim(2, 5, 300)]);
        assert_eq!(p.animation_cluster(2), Some((2, 5)));
        assert_eq!(p.animation_cluster(4), Some((2, 5)));
        assert_eq!(p.animation_cluster(1), None);
        assert_eq!(p.animation_cluster(5), None);
    }

    #[test]
    fn animation_cluster_merges_overlapping_animations() {
        // [0,6) and [3,8) overlap → one cluster [0,8) for any frame either covers.
        let p = player_with(10, vec![anim(0, 6, 500), anim(3, 8, 200)]);
        assert_eq!(p.animation_cluster(1), Some((0, 8))); // only [0,6) covers 1, grown via overlap
        assert_eq!(p.animation_cluster(7), Some((0, 8))); // only [3,8) covers 7, grown via overlap
    }

    #[test]
    fn animation_cluster_keeps_disjoint_and_touching_animations_separate() {
        // [1,3) and [3,5) touch at the boundary but share no frame → not merged.
        let p = player_with(8, vec![anim(1, 3, 300), anim(3, 5, 300)]);
        assert_eq!(p.animation_cluster(2), Some((1, 3)));
        assert_eq!(p.animation_cluster(3), Some((3, 5)));
    }

    #[test]
    fn animation_cluster_ignores_non_auto_play_animations() {
        let mut a = anim(2, 6, 300);
        a.auto_play = false;
        let p = player_with(8, vec![a]);
        assert_eq!(p.animation_cluster(3), None);
    }

    #[test]
    fn animation_cluster_skip_target_clamps_to_the_last_frame() {
        // 8 frames (0..=7); animation runs to the last frame ([5,8), end == len).
        // The forward skip target `hi.min(last)` lands on the last frame itself.
        let p = player_with(8, vec![anim(5, 8, 300)]);
        let (_, hi) = p.animation_cluster(6).unwrap();
        let last = 7;
        assert_eq!(hi.min(last), 7); // "just jump there"
        // Backward: the slide before the earliest start.
        let (lo, _) = p.animation_cluster(6).unwrap();
        assert_eq!(lo.saturating_sub(1), 4);
    }

    /// Replay a loop from `start` for `steps` ticks, collecting the frames shown
    /// and how many iterations completed.
    fn run(start: usize, end: usize, bounce: bool, steps: usize) -> (Vec<usize>, usize) {
        let mut frame = start;
        let mut forward = true;
        let mut iters = 0;
        let mut seq = vec![frame];
        for _ in 0..steps {
            let (next, nf, done) = loop_next(start, end, frame, forward, bounce);
            frame = next;
            forward = nf;
            iters += usize::from(done);
            seq.push(frame);
        }
        (seq, iters)
    }

    #[test]
    fn non_bounce_wraps_to_start() {
        // 5,6,7,8 then back to 5; each wrap is one completed iteration.
        let (seq, iters) = run(5, 9, false, 5);
        assert_eq!(seq, vec![5, 6, 7, 8, 5, 6]);
        assert_eq!(iters, 1);
    }

    #[test]
    fn bounce_ping_pongs_without_duplicating_endpoints() {
        // 5,6,7,8,7,6,5,6,8… — endpoints shown once per turn; one iteration is a
        // full there-and-back (return to start).
        let (seq, iters) = run(5, 9, true, 6);
        assert_eq!(seq, vec![5, 6, 7, 8, 7, 6, 5]);
        assert_eq!(iters, 1);
    }

    #[test]
    fn single_frame_range_completes_each_tick() {
        // A one-frame loop has nowhere to go; it just counts ticks.
        let (seq, iters) = run(3, 4, true, 3);
        assert_eq!(seq, vec![3, 3, 3, 3]);
        assert_eq!(iters, 3);
        let (_, iters_nb) = run(3, 4, false, 2);
        assert_eq!(iters_nb, 2);
    }

    #[test]
    fn a_finite_loop_entered_mid_range_plays_a_full_pass_first() {
        // Loop on 2..5, count 1, entered on its last frame (← from the slide
        // after it): it must show frame 2 and play a whole pass before exiting,
        // not count the first return to 2 as the pass.
        for (bounce, want) in [(false, vec![4, 2, 3, 4]), (true, vec![4, 3, 2, 3, 4, 3])] {
            let region = crate::types::LoopRegion {
                start_frame: 2, end_frame: 5, delay_ms: 500, count: 1, bounce,
            };
            let mut frame = 4;
            let mut state = LoopState::entering(&region, frame);
            let mut shown = vec![frame];
            while let Some((next, s)) = loop_step(&region, frame, state) {
                (frame, state) = (next, s);
                shown.push(frame);
                assert!(shown.len() < 20, "never exits: {shown:?}");
            }
            assert_eq!(shown, want, "bounce {bounce}");
        }
        // Entered on its start, one pass is unchanged: 2,3,4 then exit.
        let region = crate::types::LoopRegion { start_frame: 2, end_frame: 5, delay_ms: 500, count: 1, bounce: false };
        let state = LoopState::entering(&region, 2);
        let (a, s) = loop_step(&region, 2, state).unwrap();
        let (b, s) = loop_step(&region, a, s).unwrap();
        assert_eq!((a, b), (3, 4));
        assert_eq!(loop_step(&region, b, s), None);
    }

    #[test]
    fn two_frame_bounce_alternates() {
        // start..end = 5..7 → frames 5,6. Bounce: 5,6,5,6,… one iteration per
        // return to start.
        let (seq, iters) = run(5, 7, true, 4);
        assert_eq!(seq, vec![5, 6, 5, 6, 5]);
        assert_eq!(iters, 2);
    }

    #[test]
    fn a_command_keeps_running_only_while_playback_stays_in_its_region() {
        // One command on frames 1..4, then the same command moved (R1-6 splits
        // a geometry change into its own region) on 4..6.
        let region = |start, end, x| crate::types::CommandRegion {
            start_frame: start, end_frame: end, x, y: 0, w: 1, h: 1, status_x: 0, status_y: 0,
            command: "true".into(), args: Vec::new(), cwd: None, timeout_secs: None,
            style: Default::default(),
        };
        let (a, b) = (region(1, 4, 0), region(4, 6, 2));
        use super::keeps_command as keeps;
        assert!(keeps(&a, 1, 2) && keeps(&a, 3, 1), "steps within the region");
        assert!(keeps(&a, 2, 2), "a one-frame loop tick stays put");
        assert!(!keeps(&a, 3, 4) && !keeps(&b, 3, 4), "a different geometry is a new region");
        assert!(!keeps(&a, 0, 1) && !keeps(&b, 3, 6), "entering / leaving");
    }

    /// A `CommandRegion` on frames `start..end` at column `x` running `cmd`.
    fn cmd_region(start: usize, end: usize, x: u16, cmd: &str) -> crate::types::CommandRegion {
        crate::types::CommandRegion {
            start_frame: start, end_frame: end, x, y: 0, w: 1, h: 1, status_x: x, status_y: 0,
            command: cmd.into(), args: Vec::new(), cwd: None, timeout_secs: None,
            style: Default::default(),
        }
    }

    #[test]
    fn every_command_on_a_frame_is_active_not_only_the_first() {
        let mut p = player_with(3, Vec::new());
        let (a, b) = (cmd_region(0, 3, 0, "a"), cmd_region(1, 2, 1, "b"));
        p.presentation.commands = vec![a.clone(), b.clone()];
        assert_eq!(p.regions_for(0), vec![a.clone()]);
        assert_eq!(p.regions_for(1), vec![a.clone(), b.clone()], "both boxes on frame 1");
        // Moving 1 -> 2 keeps the deck-wide one and stops the one-slide one.
        use super::keeps_command as keeps;
        assert!(keeps(&a, 1, 2) && !keeps(&b, 1, 2));
    }

    #[cfg(unix)]
    #[test]
    fn two_commands_on_one_frame_both_start_and_finish() {
        let mut p = player_with(1, Vec::new());
        p.presentation.commands = vec![cmd_region(0, 1, 0, "true"), cmd_region(0, 1, 0, "false")];
        // Both spawn (no painting happens unless a spawn fails).
        p.maybe_start_command(&mut std::io::stdout()).unwrap();
        assert_eq!(p.commands.len(), 2, "one running command per box");
        p.maybe_start_command(&mut std::io::stdout()).unwrap();
        assert_eq!(p.commands.len(), 2, "a command already here is not started twice");
        let start = std::time::Instant::now();
        let mut done = Vec::new();
        for c in &mut p.commands {
            let super::CommandRun::Running(rc) = c else { unreachable!() };
            loop {
                if let Some((ok, _)) = rc.poll() {
                    done.push(ok);
                    break;
                }
                assert!(start.elapsed().as_secs() < 10, "command never finished");
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
        }
        assert_eq!(done, vec![true, false], "each box gets its own check / cross");
    }

    /// A command region running `sh -c script`.
    #[cfg(unix)]
    fn sh_region(script: &str, timeout_secs: Option<u64>) -> crate::types::CommandRegion {
        crate::types::CommandRegion {
            start_frame: 0, end_frame: 1, x: 0, y: 0, w: 1, h: 1, status_x: 0, status_y: 0,
            command: "sh".into(),
            args: vec!["-c".into(), script.into()],
            cwd: None,
            timeout_secs,
            style: Default::default(),
        }
    }

    /// Poll like the event loop does until the command finishes; fail past 10 s.
    #[cfg(unix)]
    fn poll_until_done(rc: &mut super::RunningCommand) -> (bool, String) {
        let start = std::time::Instant::now();
        loop {
            if let Some((ok, out)) = rc.poll() {
                return (ok, String::from_utf8_lossy(&out).into_owned());
            }
            assert!(start.elapsed().as_secs() < 10, "command never finished");
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
    }

    #[test]
    fn needed_terminal_adds_the_two_bars_without_overflow() {
        let need = |width, height| super::needed_terminal(&TerminalContract { width, height });
        assert_eq!(need(80, 22), (80, 24));
        assert_eq!(need(15, u16::MAX), (15, 65537)); // no terminal is that tall
    }

    #[cfg(unix)]
    #[test]
    fn a_stop_signal_becomes_a_stop_request_instead_of_killing_the_player() {
        use super::stop_signals;
        let guard = stop_signals::Guard::install();
        assert!(!stop_signals::requested());
        // SAFETY: raise(3) on our own process; the guard's handler catches it.
        unsafe { libc::raise(libc::SIGTERM) };
        assert!(stop_signals::requested(), "SIGTERM must reach the event loop as a flag");
        drop(guard);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn a_command_is_killed_when_its_spawning_thread_dies_without_teardown() {
        // Stand-in for a SIGKILLed player: the spawning thread ends and the
        // RunningCommand is leaked, so no destructor kills the command.
        let pid = std::thread::spawn(|| {
            let rc = super::spawn_command(sh_region("sleep 30", None)).unwrap();
            let pid = rc.child.id() as libc::pid_t;
            std::mem::forget(rc);
            pid
        })
        .join()
        .unwrap();
        let start = std::time::Instant::now();
        let mut status = 0;
        loop {
            // SAFETY: waitpid on our own child; reaps it once it has died.
            let rc = unsafe { libc::waitpid(pid, &mut status, libc::WNOHANG) };
            if rc == pid {
                break;
            }
            assert!(start.elapsed().as_secs() < 10, "command outlived its spawning thread");
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        assert!(libc::WIFSIGNALED(status) && libc::WTERMSIG(status) == libc::SIGKILL);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn a_command_leads_its_own_session_without_a_terminal() {
        // Field 6 of /proc/<pid>/stat is the session id: the shell must lead its
        // own session. Opening /dev/tty must then fail at once (ENXIO) rather
        // than reach the player's terminal; `|| echo` shows the shell got the error.
        let script = "read -r _ _ _ _ _ sid _ </proc/$$/stat; echo sid=$sid; \
                      true </dev/tty || echo no-tty";
        let mut rc = super::spawn_command(sh_region(script, Some(5))).unwrap();
        let pid = rc.child.id();
        let (ok, out) = poll_until_done(&mut rc);
        assert!(out.contains(&format!("sid={pid}")), "not a session leader: {out}");
        assert!(out.contains("no-tty"), "got: {out}");
        assert!(ok);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn reaping_a_command_that_could_not_be_killed_never_blocks() {
        // Stand-in for a leader kill(2) cannot reach (a sudo command): it is not
        // signalled at all. Reaping must give up within REAP_WAIT and leave it
        // to a background reaper, which reaps it once it exits on its own.
        let mut child = std::process::Command::new("sleep").arg("1").spawn().unwrap();
        let pid = child.id();
        let start = std::time::Instant::now();
        assert!(super::reap_without_blocking(&mut child).is_none());
        assert!(start.elapsed() < std::time::Duration::from_millis(900), "blocked on the child");
        let proc_entry = format!("/proc/{pid}");
        while std::path::Path::new(&proc_entry).exists() {
            assert!(start.elapsed().as_secs() < 10, "the background reaper never reaped it");
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        drop(child); // dropping a Child never waits or kills
    }

    #[cfg(unix)]
    #[test]
    fn a_command_that_exited_in_time_is_not_timed_out_when_noticed_late() {
        // The command exits 0 well within its 1 s timeout, but the first poll
        // that sees it comes after the deadline: it still succeeds.
        let mut rc = super::spawn_command(sh_region("echo ok", Some(1))).unwrap();
        let start = std::time::Instant::now();
        while !super::exited_unreaped(&mut rc.child) {
            assert!(start.elapsed().as_secs() < 10, "command never exited");
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        rc.start = std::time::Instant::now() - std::time::Duration::from_secs(2); // past the deadline
        let (ok, out) = poll_until_done(&mut rc);
        assert!(ok, "exit 0 within the timeout is a success: {out}");
        assert!(!out.contains("[timed out]"), "got: {out}");
    }

    #[cfg(unix)]
    #[test]
    fn timeout_finishes_even_when_a_grandchild_holds_the_pipe() {
        // `sleep` is a grandchild holding stdout; the timeout must still win.
        let mut rc = super::spawn_command(sh_region("sleep 30; echo x", Some(1))).unwrap();
        let (ok, out) = poll_until_done(&mut rc);
        assert!(!ok);
        assert!(out.ends_with("[timed out]"), "{out:?}");
        assert!(rc.start.elapsed().as_secs() < 5);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn a_grandchild_left_running_is_killed_when_the_command_exits() {
        let mut rc = super::spawn_command(sh_region("sleep 30 & echo $!", None)).unwrap();
        let (ok, out) = poll_until_done(&mut rc);
        assert!(ok, "{out:?}");
        assert!(rc.start.elapsed().as_secs() < 5, "finalizing waited on the grandchild");
        let pid: u32 = out.trim().parse().unwrap();
        let start = std::time::Instant::now();
        loop {
            let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).unwrap_or_default();
            // Gone, or a zombie awaiting its reaper.
            if stat.is_empty() || stat.rsplit(')').next().unwrap().trim_start().starts_with('Z') {
                break;
            }
            assert!(start.elapsed().as_secs() < 5, "grandchild survived: {stat}");
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn a_descendant_outside_the_group_holding_the_pipe_does_not_block() {
        // `setsid` escapes the group kill, so only the bounded drain can finish.
        let mut rc = super::spawn_command(sh_region("setsid sleep 4 & echo hi", None)).unwrap();
        let (ok, out) = poll_until_done(&mut rc);
        assert!(ok && out.contains("hi"), "{out:?}");
        assert!(rc.start.elapsed().as_secs() < 2, "finalizing waited on the pipe holder");
    }

    #[test]
    fn append_output_matches_plain_concatenation_under_the_caps() {
        let full: &[u8] = b"one\ntwo \x1b[31mred\x1b[0m\n\nlast partial";
        let mut out = Vec::new();
        for chunk in full.chunks(3) {
            super::append_output(&mut out, chunk, 64, 1024);
        }
        assert_eq!(out, full);
    }

    #[test]
    fn append_output_truncates_a_line_past_its_cap() {
        let mut out = Vec::new();
        for chunk in [&b"abc"[..], b"defg\nh", b"ijklm", b"\nz"] {
            super::append_output(&mut out, chunk, 4, 1024);
        }
        assert_eq!(out, b"abcd\nhijk\nz");
    }

    #[test]
    fn append_output_returns_to_the_line_start_on_a_lone_cr() {
        let run = |chunks: &[&[u8]]| {
            let mut out = Vec::new();
            for c in chunks {
                super::append_output(&mut out, c, 64, 1024);
            }
            out
        };
        assert_eq!(run(&[b"a\rbb\rccc\n"]), b"ccc\n");
        assert_eq!(run(&[b"x\r\ny"]), b"x\ny");
        // Split across reader chunks: a CR then its LF, and a CR then text.
        assert_eq!(run(&[b"x\r", b"\ny"]), b"x\ny");
        assert_eq!(run(&[b"h\n 10%\r", b" 99%\r", b"100%\n"]), b"h\n100%\n");
        // A progress line still pending shows its latest state.
        assert_eq!(super::layout_output(&run(&[b" 10%\r 55%\r"]), 8, 1), vec![" 55%    ".to_string()]);
    }

    #[test]
    fn append_output_keeps_a_bounded_tail_with_the_same_last_lines() {
        let (w, h) = (3, 4);
        let mut out = Vec::new();
        let mut full = Vec::new();
        for n in 0..10_000 {
            let line = format!("{n}\n");
            super::append_output(&mut out, line.as_bytes(), 4, 20);
            full.extend_from_slice(line.as_bytes());
            assert!(out.len() <= 2 * 20 + 6, "buffer grew to {}", out.len());
        }
        assert_eq!(super::layout_output(&out, w, h), super::layout_output(&full, w, h));
    }

    #[cfg(unix)]
    #[test]
    fn a_flooding_command_keeps_a_bounded_buffer() {
        // ~2 MB of "y" lines: the buffer must stay bounded while it streams in.
        let mut rc = super::spawn_command(sh_region("yes | head -n 1000000", None)).unwrap();
        let (_, keep) = super::output_caps(1, 1);
        let start = std::time::Instant::now();
        let (ok, out) = loop {
            let done = rc.poll();
            assert!(rc.out.len() <= 2 * keep + 4096, "buffer grew to {}", rc.out.len());
            if let Some((ok, out)) = done {
                break (ok, out);
            }
            assert!(start.elapsed().as_secs() < 30, "command never finished");
        };
        assert!(ok);
        assert!(out.len() <= 2 * keep + 4096);
        assert_eq!(super::layout_output(&out, 1, 1), vec!["y"]);
    }
}
