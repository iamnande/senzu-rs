use std::path::Path;

use jiff::civil::Date;
use ratatui::Frame;
use ratatui::layout::{Constraint, Flex, Layout, Margin, Position, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Clear, List, ListItem, ListState, Padding, Paragraph, Wrap};
use tui_big_text::{BigText, PixelSize};

use crate::bean::{Bean, Phase, Priority};
use crate::launch::Launcher;
use crate::output::tildify;
use crate::tui::app::{Board, Mode};
use crate::tui::theme::{ACCENT, BG, FAINT, FG, GREEN, MUTED, PANEL, RAISED, RED, fg};

/// days without a log entry before a stream reads as stale
const STALE_DAYS: i32 = 7;
const SIDEBAR: u16 = 34;

/// the six phases a stream moves through, done aside
const STEPS: [Phase; 6] = [
    Phase::Ground,
    Phase::Plan,
    Phase::Execute,
    Phase::Refine,
    Phase::Pr,
    Phase::Rollout,
];

/// what the screen needs beyond the board state
#[derive(Clone, Copy)]
pub(crate) struct View<'a> {
    pub(crate) home: &'a Path,
    pub(crate) today: Date,
    /// the agent profile's name, for the enter hint
    pub(crate) agent: &'a str,
    pub(crate) launcher: Launcher,
    /// where a bean with no worktree opens
    pub(crate) fallback: &'a Path,
}

/// the board and the view together, so a panel takes one argument for both
#[derive(Clone, Copy)]
struct Screen<'a> {
    board: &'a Board,
    view: View<'a>,
}

/// one bean and the view it's drawn in
#[derive(Clone, Copy)]
struct Stream<'a> {
    bean: &'a Bean,
    view: View<'a>,
}

/// a raised box in the middle of the screen
struct Popup<'a> {
    title: String,
    lines: Vec<Line<'a>>,
}

/// a filled panel: no border lines, a bold label, padding inside
fn panel<'a>(title: &str) -> Block<'a> {
    Block::new()
        .style(Style::new().bg(PANEL).fg(FG))
        .title(Span::styled(format!("  {title}"), fg(FG).bold()))
        .padding(Padding::new(2, 2, 1, 0))
}

/// sidebar on the left, the selected stream on the right, a bar along the bottom
pub(crate) fn render(frame: &mut Frame, board: &Board, view: View) {
    frame.render_widget(Block::new().style(Style::new().bg(BG).fg(FG)), frame.area());
    let [main, bar] =
        Layout::vertical([Constraint::Min(1), Constraint::Length(1)]).areas(frame.area());
    let [side, _, right] = Layout::horizontal([
        Constraint::Length(SIDEBAR),
        Constraint::Length(1),
        Constraint::Min(30),
    ])
    .areas(main.inner(Margin::new(1, 1)));

    let screen = Screen { board, view };
    sidebar(frame, screen, side);
    match board.selected_bean() {
        Some(bean) => detail(frame, Stream { bean, view }, right),
        None => frame.render_widget(
            Paragraph::new(Line::styled(empty_note(board), fg(MUTED))).block(panel("")),
            right,
        ),
    }
    status_bar(frame, screen, bar);

    match board.mode {
        Mode::Keys => popup(
            frame,
            main,
            Popup {
                title: "keys".into(),
                lines: keys_lines(),
            },
        ),
        Mode::Draft(n) => {
            if let Some(bean) = board.selected_bean() {
                let (title, lines) = draft_lines(board, bean, n);
                popup(frame, main, Popup { title, lines });
            }
        }
        Mode::List | Mode::Filter => {}
    }
}

/// why there's nothing to show: nothing matches, or nothing's there to match
fn empty_note(board: &Board) -> String {
    if !board.beans.is_empty() {
        return "no streams match".into();
    }
    match board.markdown {
        0 => "no beans in this dir".into(),
        n => format!("no json beans yet, ignoring {n} markdown beans"),
    }
}

/// cut to `width` characters, with an ellipsis when something got cut
fn fit(s: &str, width: usize) -> String {
    if s.chars().count() <= width {
        return s.to_owned();
    }
    s.chars()
        .take(width.saturating_sub(1))
        .chain(['…'])
        .collect()
}

fn age(bean: &Bean, today: Date) -> Option<i32> {
    let last = bean.log.iter().map(|e| e.date).max()?;
    Some((today - last).get_days())
}

fn sidebar(frame: &mut Frame, screen: Screen, area: Rect) {
    let Screen { board, view } = screen;
    frame.render_widget(Block::new().style(Style::new().bg(PANEL)), area);
    let inner = area.inner(Margin::new(2, 1));
    let [logo, filter, list_area, foot] = Layout::vertical([
        Constraint::Length(4),
        Constraint::Length(2),
        Constraint::Min(1),
        Constraint::Length(3),
    ])
    .areas(inner);

    frame.render_widget(
        BigText::builder()
            .pixel_size(PixelSize::Quadrant)
            .style(fg(ACCENT))
            .lines(vec!["senzu".into()])
            .build(),
        logo,
    );

    let [_, filter_line] =
        Layout::vertical([Constraint::Length(1), Constraint::Length(1)]).areas(filter);
    if board.mode == Mode::Filter || !board.filter.is_empty() {
        let cursor = if board.mode == Mode::Filter {
            "▏"
        } else {
            ""
        };
        frame.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled("/ ", fg(ACCENT).bold()),
                Span::styled(format!("{}{cursor}", board.filter), fg(FG)),
            ])),
            filter_line,
        );
    }

    let (items, selected) = list_items(board, view.today, usize::from(list_area.width));
    let mut state = ListState::default();
    state.select(selected);
    frame.render_stateful_widget(
        List::new(items).highlight_style(Style::new().bg(ACCENT).fg(BG).bold()),
        list_area,
        &mut state,
    );

    let urgent = board
        .beans
        .iter()
        .filter(|b| b.priority == Priority::Urgent && b.phase != Phase::Done)
        .count();
    let done = board
        .beans
        .iter()
        .filter(|b| b.phase == Phase::Done)
        .count();
    frame.render_widget(
        Paragraph::new(vec![
            Line::from(vec![
                Span::styled("● ", fg(GREEN)),
                Span::raw(format!(
                    "{} of {} streams",
                    board.visible.len(),
                    board.beans.len()
                )),
            ]),
            Line::styled(format!("{urgent} urgent · {done} done"), fg(MUTED)),
            Line::styled(format!("v{}", env!("CARGO_PKG_VERSION")), fg(FAINT)),
        ]),
        foot,
    );
}

/// streams under quiet priority labels. returns the item to highlight, since
/// the labels shift every index
fn list_items(board: &Board, today: Date, width: usize) -> (Vec<ListItem<'static>>, Option<usize>) {
    let mut items = Vec::new();
    let mut selected = None;
    let mut last: Option<(Priority, bool)> = None;
    for (pos, &i) in board.visible.iter().enumerate() {
        let b = &board.beans[i];
        let group = (b.priority, b.phase == Phase::Done);
        if last != Some(group) {
            if last.is_some() {
                items.push(ListItem::new(""));
            }
            let label = if group.1 {
                "done".to_owned()
            } else {
                b.priority.to_string()
            };
            items.push(ListItem::new(Line::styled(label, fg(FAINT))));
            last = Some(group);
        }
        if pos == board.selected {
            selected = Some(items.len());
        }
        let days = age(b, today);
        let age_text = days.map(|d| format!("{d}d")).unwrap_or_default();
        let room = width.saturating_sub(age_text.len() + 2);
        let slug = fit(&b.slug, room);
        let pad = width.saturating_sub(slug.chars().count() + age_text.len() + 1);
        let stale = days.is_some_and(|d| d > STALE_DAYS) && b.phase != Phase::Done;
        items.push(ListItem::new(Line::from(vec![
            Span::raw(format!(" {slug}{}", " ".repeat(pad))),
            Span::styled(age_text, fg(if stale { RED } else { FAINT })),
        ])));
    }
    (items, selected)
}

fn detail(frame: &mut Frame, stream: Stream, area: Rect) {
    let bean = stream.bean;
    // label, a blank row, the items, a blank row
    let open_h = u16::try_from(bean.open.len().max(1)).unwrap_or(1) + 3;
    let [top, _, open, _, log] = Layout::vertical([
        Constraint::Length(9),
        Constraint::Length(1),
        Constraint::Length(open_h),
        Constraint::Length(1),
        Constraint::Min(3),
    ])
    .areas(area);
    let [now, _, next] = Layout::horizontal([
        Constraint::Percentage(40),
        Constraint::Length(1),
        Constraint::Percentage(60),
    ])
    .areas(top);

    now_panel(frame, bean, now);
    next_panel(frame, stream, next);
    open_panel(frame, bean, open);
    log_panel(frame, bean, log);
}

/// where the stream is: priority, the phase in pixels, the phases as dots
fn now_panel(frame: &mut Frame, bean: &Bean, area: Rect) {
    let block = panel(&bean.slug);
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let [title, priority, _, hero, _, steps] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(2),
        Constraint::Length(1),
        Constraint::Length(1),
    ])
    .areas(inner);

    let width = usize::from(inner.width);
    frame.render_widget(
        Paragraph::new(Line::styled(fit(&bean.title, width), fg(MUTED))),
        title,
    );
    let color = if bean.priority == Priority::Urgent {
        RED
    } else {
        ACCENT
    };
    let mut line = vec![
        Span::styled("● ", fg(color)),
        Span::styled(bean.priority.to_string(), fg(color).bold()),
    ];
    if let Some(note) = &bean.priority_note {
        let room = width.saturating_sub(2 + bean.priority.to_string().len() + 5);
        line.push(Span::styled(format!("  ·  {}", fit(note, room)), fg(MUTED)));
    }
    frame.render_widget(Paragraph::new(Line::from(line)), priority);
    // octants, half the logo's height. needs a font with unicode 16 octants,
    // ghostty draws its own
    frame.render_widget(
        BigText::builder()
            .pixel_size(PixelSize::Octant)
            .style(fg(FG))
            .lines(vec![bean.phase.to_string().into()])
            .build(),
        hero,
    );

    let mut dots: Vec<Span> = STEPS
        .iter()
        .map(|&step| {
            let (glyph, color) = match step.cmp(&bean.phase) {
                std::cmp::Ordering::Equal => ("●", ACCENT),
                std::cmp::Ordering::Less => ("●", MUTED),
                std::cmp::Ordering::Greater => ("○", FAINT),
            };
            Span::styled(format!("{glyph} "), fg(color))
        })
        .collect();
    if let Some(n) = STEPS.iter().position(|&s| s == bean.phase) {
        dots.push(Span::styled(
            format!(" {} of {}", n + 1, STEPS.len()),
            fg(FAINT),
        ));
    }
    frame.render_widget(Paragraph::new(Line::from(dots)), steps);
}

/// what enter does: the skill call, where it runs, which launcher
fn next_panel(frame: &mut Frame, stream: Stream, area: Rect) {
    let Stream { bean, view } = stream;
    let row = |k: &'static str, v: String| {
        Line::from(vec![
            Span::styled(format!("{k:<10}"), fg(MUTED)),
            Span::raw(v),
        ])
    };
    let mut lines = vec![
        Line::styled(bean.next.text.clone(), fg(ACCENT).bold()),
        Line::raw(""),
    ];
    let worktree = match bean.worktree() {
        Some(p) => tildify(p, view.home),
        None => format!("none, opens in {}", tildify(view.fallback, view.home)),
    };
    lines.push(row("worktree", worktree));
    if let Some(branch) = bean.repos.first().and_then(|r| r.branch.clone()) {
        lines.push(row("branch", branch));
    }
    let prs = match bean.prs.len() {
        0 => "none yet".to_owned(),
        n => n.to_string(),
    };
    lines.push(row("prs", prs));
    lines.push(Line::raw(""));
    let agent = view.agent;
    let hint = match (bean.phase, view.launcher) {
        (Phase::Done, _) => "done, nothing to open".to_owned(),
        (_, Launcher::Zellij) => format!("opens {agent} in a new pane"),
        (_, Launcher::Exec) => format!("swaps senzu for {agent}"),
    };
    lines.push(Line::from(vec![
        Span::styled("enter ", fg(ACCENT).bold()),
        Span::styled(hint, fg(MUTED)),
    ]));
    frame.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .block(panel("next")),
        area,
    );
}

fn open_panel(frame: &mut Frame, bean: &Bean, area: Rect) {
    let lines: Vec<Line> = if bean.open.is_empty() {
        vec![Line::styled("nothing open", fg(MUTED))]
    } else {
        bean.open
            .iter()
            .map(|o| {
                let (mark, color) = if o.blocks {
                    ("■ ", RED)
                } else {
                    ("□ ", MUTED)
                };
                let mut spans = vec![Span::styled(mark, fg(color)), Span::raw(o.text.clone())];
                if o.draft.is_some() {
                    spans.push(Span::styled("  draft", fg(ACCENT)));
                }
                Line::from(spans)
            })
            .collect()
    };
    frame.render_widget(
        Paragraph::new(lines).block(panel(&format!("open  {}", bean.open.len()))),
        area,
    );
}

/// newest first, as many as fit, each cut to one line
fn log_panel(frame: &mut Frame, bean: &Bean, area: Rect) {
    // "nov 03  " + a 9-wide phase, inside 2+2 padding
    let room = usize::from(area.width.saturating_sub(4 + 8 + 9));
    let lines: Vec<Line> = bean
        .log
        .iter()
        .rev()
        .map(|e| {
            let text = fit(&e.text, room);
            Line::from(vec![
                Span::styled(
                    e.date.strftime("%b %d  ").to_string().to_lowercase(),
                    fg(FAINT),
                ),
                Span::styled(format!("{:<9}", e.phase.to_string()), fg(MUTED)),
                Span::raw(text),
            ])
        })
        .collect();
    frame.render_widget(
        Paragraph::new(lines).block(panel(&format!("log  {}", bean.log.len()))),
        area,
    );
}

fn status_bar(frame: &mut Frame, screen: Screen, area: Rect) {
    let Screen { board, view } = screen;
    frame.render_widget(Block::new().style(Style::new().bg(PANEL)), area);
    let keys: &[(&str, &str)] = match board.mode {
        Mode::List => &[
            ("enter", "open"),
            ("/", "filter"),
            ("a", "all"),
            ("?", "keys"),
            ("q", "quit"),
        ],
        Mode::Filter => &[("enter", "keep"), ("esc", "clear")],
        Mode::Draft(_) => &[("d", "next draft"), ("esc", "back")],
        Mode::Keys => &[("esc", "close")],
    };
    let mut right = Vec::new();
    for (i, (k, v)) in keys.iter().enumerate() {
        if i > 0 {
            right.push(Span::styled(" · ", fg(FAINT)));
        }
        right.push(Span::styled(*k, fg(FG).bold()));
        right.push(Span::styled(format!(" {v}"), fg(MUTED)));
    }
    right.push(Span::raw(" "));
    let width: u16 = right
        .iter()
        .map(|s| u16::try_from(s.width()).unwrap_or(0))
        .sum();
    let [l, r] = Layout::horizontal([Constraint::Min(1), Constraint::Length(width)]).areas(area);

    // a status message takes the left side, otherwise the launcher does
    let left = if let Some(msg) = &board.status {
        Line::from(vec![
            Span::styled(" ● ", fg(ACCENT)),
            Span::styled(msg.clone(), fg(FG)),
        ])
    } else {
        let name = match view.launcher {
            Launcher::Exec => "EXEC",
            Launcher::Zellij => "ZELLIJ",
        };
        Line::from(vec![
            Span::styled(" ● ", fg(GREEN)),
            Span::styled(name, fg(FG).bold()),
        ])
    };
    frame.render_widget(Paragraph::new(left), l);
    frame.render_widget(Paragraph::new(Line::from(right)), r);
}

fn keys_lines() -> Vec<Line<'static>> {
    [
        ("j k", "move"),
        ("enter", "open the agent on the next skill"),
        ("/", "filter by slug or title"),
        ("a", "everything, done included"),
        ("d", "read the linked drafts"),
        ("r", "reload from disk"),
        ("q", "quit"),
    ]
    .iter()
    .map(|(k, v)| {
        Line::from(vec![
            Span::styled(format!("{k:<7}"), fg(ACCENT).bold()),
            Span::raw(*v),
        ])
    })
    .collect()
}

/// the draft's title, and its body with light markdown: # headings, - bullets
fn draft_lines(board: &Board, bean: &Bean, n: usize) -> (String, Vec<Line<'static>>) {
    let ids = board.drafts_for_selected();
    let Some(draft) = ids
        .get(n)
        .and_then(|id| bean.drafts.iter().find(|d| d.id == *id))
    else {
        return ("draft".into(), vec![Line::raw("that draft is gone")]);
    };
    let mut lines = vec![
        Line::styled(
            format!(
                "{}  ·  {} of {}  ·  d for the next",
                draft.date,
                n + 1,
                ids.len()
            ),
            fg(FAINT),
        ),
        Line::raw(""),
    ];
    for l in draft.body.lines() {
        lines.push(if let Some(h) = l.strip_prefix("# ") {
            Line::styled(h.to_owned(), fg(ACCENT).bold())
        } else if let Some(item) = l.strip_prefix("- ") {
            Line::from(vec![
                Span::styled("  • ", fg(ACCENT)),
                Span::raw(item.to_owned()),
            ])
        } else {
            Line::raw(l.to_owned())
        });
    }
    (draft.title.clone(), lines)
}

/// a raised panel in the middle, with everything behind it faded
fn popup(frame: &mut Frame, area: Rect, popup: Popup) {
    let Popup { title, lines } = popup;
    let width = lines
        .iter()
        .map(Line::width)
        .max()
        .unwrap_or(0)
        .clamp(30, usize::from(area.width.saturating_sub(8)));
    let height = lines.len().min(usize::from(area.height.saturating_sub(4)));
    let [v] = Layout::vertical([Constraint::Length(u16::try_from(height).unwrap_or(0) + 4)])
        .flex(Flex::Center)
        .areas(area);
    let [r] = Layout::horizontal([Constraint::Length(u16::try_from(width).unwrap_or(0) + 6)])
        .flex(Flex::Center)
        .areas(v);

    let full = frame.area();
    let buf = frame.buffer_mut();
    for y in full.top()..full.bottom() {
        for x in full.left()..full.right() {
            if !r.contains(Position { x, y }) {
                let cell = &mut buf[(x, y)];
                cell.set_fg(FAINT);
                // the selection bar fades too, or it outshines the popup
                if cell.bg == ACCENT {
                    cell.set_bg(PANEL);
                }
            }
        }
    }
    frame.render_widget(Clear, r);
    frame.render_widget(
        Paragraph::new(lines).wrap(Wrap { trim: false }).block(
            Block::new()
                .style(Style::new().bg(RAISED).fg(FG))
                .title(Span::styled(format!("  {title}"), fg(FG).bold()))
                .padding(Padding::new(2, 2, 1, 1)),
        ),
        r,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyCode, KeyEvent};
    use ratatui::Terminal as Term;
    use ratatui::backend::TestBackend;
    use ratatui::buffer::Buffer;

    const W: u16 = 120;
    const H: u16 = 32;

    fn board() -> Board {
        let bean = |slug: &str, priority: &str, last: &str| -> Bean {
            serde_json::from_value(serde_json::json!({
                "schema": 1, "slug": slug, "title": format!("the {slug} title"), "phase": "plan",
                "priority": priority, "issue": null, "thread": null, "plan": null,
                "next": { "text": format!("/iam-plan {slug}"), "draft": "the-map" },
                "open": [{ "text": "a blocker", "blocks": true, "draft": null }],
                "drafts": [{ "id": "the-map", "title": "a map", "date": "2026-11-03",
                    "body": "# rocks\n- more rocks" }],
                "log": [
                    { "date": "2026-10-01", "phase": "ground", "text": "oldest entry" },
                    { "date": last, "phase": "plan", "text": "newest entry" },
                ],
            }))
            .unwrap()
        };
        Board::new(vec![
            bean("crab-shell", "urgent", "2026-11-03"),
            bean("kelp", "high", "2026-10-01"),
        ])
    }

    fn view(launcher: Launcher) -> View<'static> {
        View {
            home: Path::new("/home/crab"),
            today: "2026-11-03".parse().unwrap(),
            agent: "crabby",
            launcher,
            fallback: Path::new("/home/crab/senzu"),
        }
    }

    fn draw(board: &Board, launcher: Launcher) -> Buffer {
        let mut t = Term::new(TestBackend::new(W, H)).unwrap();
        t.draw(|f| render(f, board, view(launcher))).unwrap();
        t.backend().buffer().clone()
    }

    fn text(buf: &Buffer) -> String {
        buf.content()
            .chunks(usize::from(buf.area.width))
            .map(|row| {
                row.iter()
                    .map(ratatui::buffer::Cell::symbol)
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// the cell where `needle` starts on screen
    fn find(buf: &Buffer, needle: &str) -> (u16, u16) {
        find_within(buf, needle, W)
    }

    /// the same, only looking left of column `max_x`
    fn find_within(buf: &Buffer, needle: &str, max_x: u16) -> (u16, u16) {
        let t = text(buf);
        for (y, row) in t.lines().enumerate() {
            let row: String = row.chars().take(usize::from(max_x)).collect();
            if let Some(x) = row.find(needle) {
                #[expect(clippy::string_slice, reason = "find lands on a char boundary")]
                let x = row[..x].chars().count();
                return (u16::try_from(x).unwrap(), u16::try_from(y).unwrap());
            }
        }
        panic!("{needle:?} not on screen:\n{t}");
    }

    fn in_sidebar(buf: &Buffer, needle: &str) -> (u16, u16) {
        find_within(buf, needle, SIDEBAR + 1)
    }

    fn key(board: &mut Board, c: char) {
        board.update(KeyEvent::from(KeyCode::Char(c)));
    }

    #[test]
    fn the_selected_stream_gets_the_accent_bar() {
        let mut board = board();
        key(&mut board, 'j');
        let buf = draw(&board, Launcher::Zellij);
        let (x, y) = in_sidebar(&buf, "kelp");
        assert_eq!(buf[(x, y)].bg, ACCENT);
        let (x, y) = in_sidebar(&buf, "crab-shell");
        assert_eq!(buf[(x, y)].bg, PANEL);
    }

    #[test]
    fn the_detail_follows_the_selection() {
        let mut board = board();
        key(&mut board, 'j');
        let got = text(&draw(&board, Launcher::Zellij));
        assert!(got.contains("the kelp title"), "{got}");
        assert!(got.contains("/iam-plan kelp"), "{got}");
        assert!(!got.contains("the crab-shell title"), "{got}");
    }

    /// checked on unselected rows, since the bar recolors the selected one
    #[test]
    fn a_stale_stream_shows_its_age_in_red() {
        let mut board = board();
        let buf = draw(&board, Launcher::Zellij);
        let (x, y) = in_sidebar(&buf, "33d");
        assert_eq!(buf[(x, y)].fg, RED);
        key(&mut board, 'j');
        let buf = draw(&board, Launcher::Zellij);
        let (x, y) = in_sidebar(&buf, " 0d");
        assert_eq!(buf[(x + 1, y)].fg, FAINT);
    }

    #[test]
    fn long_text_is_cut_with_an_ellipsis() {
        assert_eq!(fit("senzu", 5), "senzu");
        assert_eq!(fit("senzu-cli", 5), "senz…");
        assert_eq!(fit("ünïcödé", 4), "ünï…");
    }

    #[test]
    fn the_phase_is_step_two_of_six() {
        let got = text(&draw(&board(), Launcher::Zellij));
        assert!(got.contains("2 of 6"), "{got}");
    }

    /// octant glyphs live at U+1CD00..U+1CDE5, sextants and quadrants elsewhere
    #[test]
    fn the_phase_is_drawn_in_octant_pixels() {
        let buf = draw(&board(), Launcher::Zellij);
        let octants = buf
            .content()
            .iter()
            .filter(|c| {
                c.symbol()
                    .chars()
                    .any(|ch| ('\u{1CD00}'..='\u{1CDE5}').contains(&ch))
            })
            .count();
        assert!(octants > 5, "only {octants} octant cells");
    }

    #[test]
    fn enter_says_what_it_will_do_for_the_launcher() {
        let got = text(&draw(&board(), Launcher::Zellij));
        assert!(got.contains("opens crabby in a new pane"), "{got}");
        assert!(got.contains("none, opens in ~/senzu"), "{got}");
        assert!(got.contains("ZELLIJ"), "{got}");
        let got = text(&draw(&board(), Launcher::Exec));
        assert!(got.contains("swaps senzu for crabby"), "{got}");
        assert!(got.contains("EXEC"), "{got}");
    }

    #[test]
    fn the_log_is_newest_first() {
        let got = text(&draw(&board(), Launcher::Zellij));
        let newest = got.find("newest entry").unwrap();
        let oldest = got.find("oldest entry").unwrap();
        assert!(newest < oldest, "{got}");
    }

    #[test]
    fn blocking_items_are_red() {
        let buf = draw(&board(), Launcher::Zellij);
        let (x, y) = find(&buf, "■ a blocker");
        assert_eq!(buf[(x, y)].fg, RED);
    }

    #[test]
    fn the_filter_shows_in_the_sidebar() {
        let mut board = board();
        key(&mut board, '/');
        key(&mut board, 'k');
        let got = text(&draw(&board, Launcher::Zellij));
        assert!(got.contains("/ k▏"), "{got}");
        assert!(got.contains("1 of 2 streams"), "{got}");
        assert!(got.contains("esc clear"), "{got}");
    }

    #[test]
    fn a_status_takes_the_left_of_the_bar() {
        let mut board = board();
        board.status = Some("claude not found on PATH".into());
        let got = text(&draw(&board, Launcher::Zellij));
        assert!(got.contains("claude not found on PATH"), "{got}");
        assert!(!got.contains("ZELLIJ"), "{got}");
    }

    #[test]
    fn question_mark_pops_the_keys_over_a_faded_screen() {
        let mut board = board();
        key(&mut board, '?');
        let buf = draw(&board, Launcher::Zellij);
        let got = text(&buf);
        assert!(got.contains("open the agent on the next skill"), "{got}");
        let (x, y) = find(&buf, "the crab-shell title");
        assert_eq!(buf[(x, y)].fg, FAINT);
        let (x, y) = in_sidebar(&buf, "crab-shell");
        assert_eq!(buf[(x, y)].bg, PANEL, "the selection bar fades too");
    }

    #[test]
    fn d_pops_the_draft_with_light_markdown() {
        let mut board = board();
        key(&mut board, 'd');
        let buf = draw(&board, Launcher::Zellij);
        let got = text(&buf);
        assert!(got.contains("a map"), "{got}");
        assert!(got.contains("1 of 1"), "{got}");
        assert!(got.contains("• more rocks"), "{got}");
        assert!(!got.contains("- more rocks"), "{got}");
        let (x, y) = find(&buf, "rocks ");
        assert_eq!(buf[(x, y)].fg, ACCENT);
    }

    #[test]
    fn an_empty_jar_says_why() {
        let mut board = Board::new(Vec::new());
        board.markdown = 33;
        let got = text(&draw(&board, Launcher::Zellij));
        assert!(
            got.contains("no json beans yet, ignoring 33 markdown beans"),
            "{got}"
        );
        board.markdown = 0;
        assert!(text(&draw(&board, Launcher::Zellij)).contains("no beans in this dir"));
    }
}
