mod app;
mod theme;
mod ui;

use std::path::Path;
use std::time::Duration;

use crossterm::event::{self, Event, KeyEventKind};
use ratatui::DefaultTerminal;

use crate::config::Env;
use crate::launch::{self, Launch, Launcher, Profile};
use crate::{load, tabs};
use app::{Action, Board};

/// how often the live tabs get checked. a `zellij action` call each time
const TICK: Duration = Duration::from_secs(2);

/// the whole screen, or just the list beside an agent in a bean's tab
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Layout {
    Full,
    Sidebar,
}

/// how the tui runs, from the flags
#[derive(Clone, Copy)]
pub(crate) struct Opts {
    pub(crate) profile: Profile,
    pub(crate) layout: Layout,
}

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
    env: &'a Env,
    opts: Opts,
}

/// exec hands the terminal to the agent, zellij opens a pane and comes back
pub(crate) fn run(beans_dir: &Path, env: &Env, opts: Opts) -> anyhow::Result<()> {
    let mut board = Board::new(Vec::new());
    reload(&mut board, beans_dir)?;
    // init sets raw mode and the alternate screen, and hooks panics to undo both
    let mut terminal = ratatui::init();
    title::push();
    let ctx = Ctx {
        beans_dir,
        env,
        opts,
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
    let mut watch = Watch::default();
    watch.tick(board, ctx);
    loop {
        let view = ui::View {
            home: &ctx.env.home,
            today: jiff::Zoned::now().date(),
            agent: ctx.opts.profile.agent.name,
            launcher: ctx.opts.profile.launcher,
            fallback: launch::fallback_cwd(ctx.beans_dir),
            layout: ctx.opts.layout,
        };
        terminal.draw(|frame| {
            let _span = tracing::debug_span!("render").entered();
            ui::render(frame, board, view);
        })?;
        if !event::poll(TICK)? {
            watch.tick(board, ctx);
            continue;
        }
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
                let _span = tracing::info_span!("reload").entered();
                reload(board, ctx.beans_dir)?;
                watch.tick(board, ctx);
                if board.status.is_none() {
                    board.status = Some(format!("reloaded {} beans", board.beans.len()));
                }
            }
            Some(Action::Launch(slug)) => {
                let _span = tracing::info_span!("open", slug = %slug).entered();
                if let Some(outcome) = launch_from(board, &slug, ctx) {
                    return Ok(outcome);
                }
                watch.tick(board, ctx);
            }
        }
    }
}

/// keeps the live tabs fresh. a failing zellij is logged once, not every tick
#[derive(Default)]
struct Watch {
    last_error: Option<String>,
}

impl Watch {
    fn tick(&mut self, board: &mut Board, ctx: Ctx) {
        if !ctx.env.in_zellij {
            return;
        }
        let names = match tabs::names() {
            Ok(names) => {
                self.last_error = None;
                names
            }
            Err(e) => {
                let msg = e.to_string();
                if self.last_error.as_deref() != Some(msg.as_str()) {
                    tracing::info!(error = %msg, outcome = "no live tabs", "list tabs");
                    self.last_error = Some(msg);
                }
                Vec::new()
            }
        };
        let state = ctx.env.state_dir();
        let dirs = tabs::Dirs {
            beans: ctx.beans_dir,
            state: &state,
        };
        board.live = tabs::live(&names, board.beans.iter().map(|b| b.slug.as_str()), dirs);
    }
}

/// plan and preflight with the tui still up, so any problem lands in the status line
fn launch_from(board: &mut Board, slug: &str, ctx: Ctx) -> Option<Outcome> {
    let profile = ctx.opts.profile;
    let bean = board.beans.iter().find(|b| b.slug == slug)?;
    let planned = match launch::plan(bean, profile, ctx.beans_dir)
        .and_then(|p| launch::preflight(&p, profile.agent, ctx.env).map(|()| p))
    {
        Ok(p) => p,
        Err(e) => {
            tracing::error!(error = %e, "launch refused");
            board.status = Some(e.to_string());
            return None;
        }
    };
    if planned.launcher == Launcher::Exec {
        return Some(Outcome::Exec(planned));
    }
    board.status = Some(match launch::run(&planned) {
        Ok(()) => format!("opened {slug} in a new pane"),
        Err(e) => {
            tracing::error!(error = %e, "launch failed");
            e.to_string()
        }
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
