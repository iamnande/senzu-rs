use std::collections::HashMap;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::bean::{Bean, Phase};
use crate::output::{self, Filter};
use crate::tabs::Live;

/// what the event loop should do after a key. everything else is state
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Action {
    Quit,
    Launch(String),
    Reload,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Mode {
    List,
    /// typing a filter
    Filter,
    /// reading a linked draft, by its index in `drafts_for_selected`
    Draft(usize),
    /// the keys popup
    Keys,
}

/// the whole tui state. `update` changes it and never touches a terminal
pub(crate) struct Board {
    pub(crate) beans: Vec<Bean>,
    /// indexes into `beans`, sorted and filtered, in display order
    pub(crate) visible: Vec<usize>,
    /// index into `visible`
    pub(crate) selected: usize,
    /// every priority and done beans too, like `ls --all --phase` for each
    pub(crate) all: bool,
    pub(crate) filter: String,
    pub(crate) mode: Mode,
    pub(crate) status: Option<String>,
    /// old `.md` beans in the dir, skipped. the empty screen says so
    pub(crate) markdown: usize,
    /// beans with a zellij tab open, by slug
    pub(crate) live: HashMap<String, Live>,
}

impl Board {
    pub(crate) fn new(beans: Vec<Bean>) -> Self {
        let mut board = Self {
            beans,
            visible: Vec::new(),
            selected: 0,
            all: false,
            filter: String::new(),
            mode: Mode::List,
            status: None,
            markdown: 0,
            live: HashMap::new(),
        };
        board.refresh(None);
        board
    }

    pub(crate) fn selected_bean(&self) -> Option<&Bean> {
        self.visible.get(self.selected).map(|&i| &self.beans[i])
    }

    /// new beans from a reload, keeping the selection on the same slug
    pub(crate) fn replace_beans(&mut self, beans: Vec<Bean>) {
        let keep = self.selected_bean().map(|b| b.slug.clone());
        self.beans = beans;
        self.refresh(keep.as_deref());
    }

    /// the draft ids the selected bean links to: next's first, then open items
    /// in order, each once even when two things link the same draft
    pub(crate) fn drafts_for_selected(&self) -> Vec<&str> {
        let Some(bean) = self.selected_bean() else {
            return Vec::new();
        };
        let mut ids: Vec<&str> = Vec::new();
        let linked = bean
            .next
            .draft
            .as_deref()
            .into_iter()
            .chain(bean.open.iter().filter_map(|o| o.draft.as_deref()));
        for id in linked {
            if !ids.contains(&id) {
                ids.push(id);
            }
        }
        ids
    }

    pub(crate) fn update(&mut self, key: KeyEvent) -> Option<Action> {
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
            return Some(Action::Quit);
        }
        match self.mode {
            Mode::List => self.on_list(key.code),
            Mode::Filter => {
                self.on_filter(key.code);
                None
            }
            Mode::Draft(n) => {
                self.on_draft(key.code, n);
                None
            }
            Mode::Keys => {
                if matches!(key.code, KeyCode::Esc | KeyCode::Char('q' | '?')) {
                    self.mode = Mode::List;
                }
                None
            }
        }
    }

    fn on_list(&mut self, code: KeyCode) -> Option<Action> {
        self.status = None;
        match code {
            KeyCode::Char('q') => return Some(Action::Quit),
            KeyCode::Char('j') | KeyCode::Down => {
                if self.selected + 1 < self.visible.len() {
                    self.selected += 1;
                }
            }
            KeyCode::Char('k') | KeyCode::Up => self.selected = self.selected.saturating_sub(1),
            KeyCode::Char('/') => self.mode = Mode::Filter,
            KeyCode::Char('?') => self.mode = Mode::Keys,
            KeyCode::Char('a') => {
                self.all = !self.all;
                self.refresh_keeping_selection();
            }
            KeyCode::Char('r') => return Some(Action::Reload),
            KeyCode::Char('d') => {
                if self.drafts_for_selected().is_empty() {
                    self.status = Some("no drafts linked".into());
                } else {
                    self.mode = Mode::Draft(0);
                }
            }
            KeyCode::Esc if !self.filter.is_empty() => {
                self.filter.clear();
                self.refresh_keeping_selection();
            }
            KeyCode::Enter => {
                let bean = self.selected_bean()?;
                if bean.phase == Phase::Done {
                    self.status = Some(format!("{} is done", bean.slug));
                    return None;
                }
                return Some(Action::Launch(bean.slug.clone()));
            }
            _ => {}
        }
        None
    }

    fn on_filter(&mut self, code: KeyCode) {
        match code {
            KeyCode::Enter => self.mode = Mode::List,
            KeyCode::Esc => {
                self.filter.clear();
                self.mode = Mode::List;
                self.refresh_keeping_selection();
            }
            KeyCode::Backspace => {
                self.filter.pop();
                self.refresh_keeping_selection();
            }
            KeyCode::Char(c) => {
                self.filter.push(c);
                self.refresh_keeping_selection();
            }
            _ => {}
        }
    }

    fn on_draft(&mut self, code: KeyCode, n: usize) {
        match code {
            KeyCode::Esc | KeyCode::Char('q') => self.mode = Mode::List,
            // d again steps to the next linked draft, wrapping around
            KeyCode::Char('d') => {
                let count = self.drafts_for_selected().len().max(1);
                self.mode = Mode::Draft((n + 1) % count);
            }
            _ => {}
        }
    }

    fn refresh_keeping_selection(&mut self) {
        let keep = self.selected_bean().map(|b| b.slug.clone());
        self.refresh(keep.as_deref());
    }

    /// recompute what's visible, then put the selection back on `keep` if it's still there
    fn refresh(&mut self, keep: Option<&str>) {
        let filter = Filter {
            all: self.all,
            ..Filter::default()
        };
        let needle = self.filter.to_lowercase();
        let beans = &self.beans;
        // the tui's "all" means everything, done beans included, sorted last
        let shown = |b: &Bean| filter.keeps(b) || (self.all && b.phase == Phase::Done);
        let matches = |b: &Bean| {
            needle.is_empty()
                || b.slug.to_lowercase().contains(&needle)
                || b.title.to_lowercase().contains(&needle)
        };
        let mut visible: Vec<usize> = (0..beans.len())
            .filter(|&i| shown(&beans[i]) && matches(&beans[i]))
            .collect();
        visible.sort_by(|&a, &b| {
            let (a, b) = (&beans[a], &beans[b]);
            (a.phase == Phase::Done)
                .cmp(&(b.phase == Phase::Done))
                .then_with(|| output::order(a, b))
        });
        self.visible = visible;
        self.selected = keep
            .and_then(|slug| {
                self.visible
                    .iter()
                    .position(|&i| self.beans[i].slug == slug)
            })
            .unwrap_or(0)
            .min(self.visible.len().saturating_sub(1));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bean([slug, title, priority, phase]: [&str; 4]) -> Bean {
        serde_json::from_value(serde_json::json!({
            "schema": 1, "slug": slug, "title": title, "phase": phase, "priority": priority,
            "issue": null, "thread": null, "plan": null,
            "next": { "text": format!("/senzu-{phase} {slug}"), "draft": null },
        }))
        .unwrap()
    }

    fn board() -> Board {
        Board::new(vec![
            bean(["crab-shell", "a shell", "urgent", "execute"]),
            bean(["harden-the-hut", "locks", "urgent", "plan"]),
            bean(["kelp-budget", "money for kelp", "medium", "ground"]),
            bean(["old-molt", "shed it", "high", "done"]),
            bean(["tidy-the-tidepool", "rocks", "high", "ground"]),
        ])
    }

    fn key(c: char) -> KeyEvent {
        KeyEvent::from(KeyCode::Char(c))
    }

    fn slugs(board: &Board) -> Vec<&str> {
        board
            .visible
            .iter()
            .map(|&i| board.beans[i].slug.as_str())
            .collect()
    }

    fn selected(board: &Board) -> &str {
        &board.selected_bean().unwrap().slug
    }

    #[test]
    fn starts_on_urgent_and_high_in_ls_order() {
        let board = board();
        assert_eq!(
            slugs(&board),
            ["crab-shell", "harden-the-hut", "tidy-the-tidepool"]
        );
        assert_eq!(selected(&board), "crab-shell");
    }

    #[test]
    fn j_and_k_stop_at_the_edges() {
        let mut board = board();
        board.update(key('k'));
        assert_eq!(selected(&board), "crab-shell");
        for _ in 0..5 {
            board.update(key('j'));
        }
        assert_eq!(selected(&board), "tidy-the-tidepool");
        board.update(key('k'));
        assert_eq!(selected(&board), "harden-the-hut");
    }

    #[test]
    fn a_shows_everything_done_last_and_keeps_the_selection() {
        let mut board = board();
        board.update(key('j'));
        board.update(key('a'));
        assert_eq!(
            slugs(&board),
            [
                "crab-shell",
                "harden-the-hut",
                "tidy-the-tidepool",
                "kelp-budget",
                "old-molt"
            ]
        );
        assert_eq!(selected(&board), "harden-the-hut");
        board.update(key('a'));
        assert_eq!(slugs(&board).len(), 3);
    }

    #[test]
    fn filter_narrows_by_slug_or_title_as_you_type() {
        let mut board = board();
        board.update(key('/'));
        assert_eq!(board.mode, Mode::Filter);
        for c in "ROCK".chars() {
            board.update(key(c));
        }
        assert_eq!(slugs(&board), ["tidy-the-tidepool"]);
        // q is a letter while filtering, not quit
        assert_eq!(board.update(key('q')), None);
        board.update(KeyEvent::from(KeyCode::Backspace));
        assert_eq!(board.filter, "ROCK");
    }

    #[test]
    fn enter_keeps_the_filter_and_esc_clears_it() {
        let mut board = board();
        board.update(key('/'));
        board.update(key('h'));
        board.update(key('u'));
        board.update(KeyEvent::from(KeyCode::Enter));
        assert_eq!(board.mode, Mode::List);
        assert_eq!(slugs(&board), ["harden-the-hut"]);
        board.update(KeyEvent::from(KeyCode::Esc));
        assert_eq!(slugs(&board).len(), 3);
        assert_eq!(selected(&board), "harden-the-hut");
    }

    #[test]
    fn a_filter_that_matches_nothing_selects_nothing() {
        let mut board = board();
        board.update(key('/'));
        board.update(key('z'));
        assert!(board.selected_bean().is_none());
        assert_eq!(board.update(KeyEvent::from(KeyCode::Enter)), None);
        assert_eq!(board.update(KeyEvent::from(KeyCode::Enter)), None);
    }

    #[test]
    fn enter_launches_the_selected_bean() {
        let mut board = board();
        board.update(key('j'));
        assert_eq!(
            board.update(KeyEvent::from(KeyCode::Enter)),
            Some(Action::Launch("harden-the-hut".into()))
        );
    }

    #[test]
    fn enter_on_a_done_bean_says_so_and_launches_nothing() {
        let mut board = board();
        board.update(key('a'));
        for _ in 0..4 {
            board.update(key('j'));
        }
        assert_eq!(selected(&board), "old-molt");
        assert_eq!(board.update(KeyEvent::from(KeyCode::Enter)), None);
        assert_eq!(board.status.as_deref(), Some("old-molt is done"));
    }

    #[test]
    fn q_quits_r_reloads_and_ctrl_c_always_quits() {
        let mut board = board();
        assert_eq!(board.update(key('q')), Some(Action::Quit));
        assert_eq!(board.update(key('r')), Some(Action::Reload));
        board.update(key('/'));
        let ctrl_c = KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL);
        assert_eq!(board.update(ctrl_c), Some(Action::Quit));
    }

    #[test]
    fn reload_keeps_the_selection_by_slug() {
        let mut board = board();
        board.update(key('j'));
        board.update(key('j'));
        let mut beans = board.beans.clone();
        beans.retain(|b| b.slug != "crab-shell");
        board.replace_beans(beans);
        assert_eq!(selected(&board), "tidy-the-tidepool");
    }

    #[test]
    fn question_mark_opens_the_keys_and_anything_closing_goes_back() {
        let mut board = board();
        board.update(key('?'));
        assert_eq!(board.mode, Mode::Keys);
        // keys are inert behind the popup
        assert_eq!(board.update(key('j')), None);
        assert_eq!(selected(&board), "crab-shell");
        assert_eq!(board.update(key('q')), None);
        assert_eq!(board.mode, Mode::List);
        board.update(key('?'));
        board.update(KeyEvent::from(KeyCode::Esc));
        assert_eq!(board.mode, Mode::List);
    }

    #[test]
    fn d_opens_linked_drafts_in_order_and_esc_goes_back() {
        let mut board = board();
        assert_eq!(board.update(key('d')), None);
        assert_eq!(board.status.as_deref(), Some("no drafts linked"));

        let mut b = board.beans[0].clone();
        b.next.draft = Some("map".into());
        b.open = vec![
            serde_json::from_value(
                serde_json::json!({"text": "send", "blocks": false, "draft": "email"}),
            )
            .unwrap(),
        ];
        board.beans[0] = b;
        assert_eq!(board.drafts_for_selected(), ["map", "email"]);
        board.update(key('d'));
        assert_eq!(board.mode, Mode::Draft(0));
        board.update(key('d'));
        assert_eq!(board.mode, Mode::Draft(1));
        board.update(key('d'));
        assert_eq!(board.mode, Mode::Draft(0));
        board.update(KeyEvent::from(KeyCode::Esc));
        assert_eq!(board.mode, Mode::List);
    }

    #[test]
    fn a_draft_linked_twice_is_listed_once() {
        let mut board = board();
        let mut b = board.beans[0].clone();
        b.next.draft = Some("map".into());
        b.open = vec![
            serde_json::from_value(
                serde_json::json!({"text": "send", "blocks": false, "draft": "map"}),
            )
            .unwrap(),
        ];
        board.beans[0] = b;
        assert_eq!(board.drafts_for_selected(), ["map"]);
    }
}
