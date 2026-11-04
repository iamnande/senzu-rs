mod app;
mod theme;
mod ui;

use std::path::Path;

use crossterm::event::{self, Event, KeyEventKind};
use ratatui::DefaultTerminal;

use crate::launch::{self, Host, Launch, Launcher, Profile};
use crate::load;
use app::{Action, Board};

/// what's left to do once the screen is handed back
enum Outcome {
    Quit,
    /// exec can't happen with the tui up: restore first, then replace the process
    Exec(Launch),
}

/// what stays fixed for the whole session
#[derive(Clone, Copy)]
struct Ctx<'a> {
    beans_dir: &'a Path,
    home: &'a Path,
    profile: Profile,
}

/// the tui is home base: it stays open, and enter sends the agent somewhere else
pub(crate) fn run(beans_dir: &Path, home: &Path, profile: Profile) -> anyhow::Result<()> {
    let mut board = Board::new(Vec::new());
    reload(&mut board, beans_dir)?;
    // init sets raw mode and the alternate screen, and hooks panics to undo both
    let mut terminal = ratatui::init();
    title::push();
    let ctx = Ctx {
        beans_dir,
        home,
        profile,
    };
    let outcome = event_loop(&mut terminal, &mut board, ctx);
    title::pop();
    ratatui::restore();
    match outcome? {
        Outcome::Quit => Ok(()),
        Outcome::Exec(planned) => Ok(launch::run(&planned)?),
    }
}

fn event_loop(
    terminal: &mut DefaultTerminal,
    board: &mut Board,
    ctx: Ctx,
) -> anyhow::Result<Outcome> {
    loop {
        let view = ui::View {
            home: ctx.home,
            today: jiff::Zoned::now().date(),
            agent: ctx.profile.agent.name,
            launcher: ctx.profile.launcher,
            fallback: launch::fallback_cwd(ctx.beans_dir),
        };
        terminal.draw(|frame| {
            let _span = tracing::debug_span!("render").entered();
            ui::render(frame, board, view);
        })?;
        let Event::Key(key) = event::read()? else {
            continue;
        };
        if key.kind != KeyEventKind::Press {
            continue;
        }
        match board.update(key) {
            None => {}
            Some(Action::Quit) => return Ok(Outcome::Quit),
            Some(Action::Reload) => {
                reload(board, ctx.beans_dir)?;
                if board.status.is_none() {
                    board.status = Some(format!("reloaded {} beans", board.beans.len()));
                }
            }
            Some(Action::Launch(slug)) => {
                if let Some(outcome) = launch_from(board, &slug, ctx) {
                    return Ok(outcome);
                }
            }
        }
    }
}

/// plan and preflight with the tui still up, so any problem lands in the status line
fn launch_from(board: &mut Board, slug: &str, ctx: Ctx) -> Option<Outcome> {
    let bean = board.beans.iter().find(|b| b.slug == slug)?;
    let planned = match launch::plan(bean, ctx.profile, ctx.beans_dir)
        .and_then(|p| launch::preflight(&p, ctx.profile.agent, &Host::from_process()).map(|()| p))
    {
        Ok(p) => p,
        Err(e) => {
            board.status = Some(e.to_string());
            return None;
        }
    };
    if planned.launcher == Launcher::Exec {
        return Some(Outcome::Exec(planned));
    }
    board.status = Some(match launch::run(&planned) {
        Ok(()) => format!("opened {slug} in a new pane"),
        Err(e) => e.to_string(),
    });
    None
}

/// a missing dir is fatal. bad beans are a count in the status line, since
/// anything printed to stderr would vanish under the alternate screen
fn reload(board: &mut Board, beans_dir: &Path) -> anyhow::Result<()> {
    let harvest = load::load(beans_dir)?;
    let skipped = harvest.errors.len();
    let unknown = harvest.unknown.len();
    board.markdown = harvest.markdown;
    board.replace_beans(harvest.beans);
    board.status = match (skipped, unknown) {
        (0, 0) => None,
        (0, n) => Some(format!(
            "{n} unknown fields, a newer senzu? see `senzu check`"
        )),
        (n, _) => Some(format!("skipped {n} bad beans, see `senzu check`")),
    };
    Ok(())
}

/// the terminal title, which zellij shows as the pane name. xterm's title stack
/// saves the old one and puts it back on the way out. terminals without the
/// stack ignore the push and pop, and keep our title
mod title {
    use std::io::Write as _;

    pub(crate) fn push() {
        let mut out = std::io::stdout();
        let _ = write!(out, "\x1b[22;0t");
        let _ = crossterm::execute!(out, crossterm::terminal::SetTitle("🫘 senzu"));
    }

    pub(crate) fn pop() {
        let mut out = std::io::stdout();
        let _ = write!(out, "\x1b[23;0t");
        let _ = out.flush();
    }
}
