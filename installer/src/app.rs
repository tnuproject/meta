//! Event handling.

use std::io;

use meta_eds_detect::Inventory;
use ratatui::DefaultTerminal;
use ratatui::crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

use crate::config::InstallConfig;
use crate::disks::Disk;
use crate::plan::Plan;
use crate::screens::{Field, Screen};
use crate::ui;

#[derive(Debug)]
pub enum Outcome {
    Quit,
    SavePlan(Box<Plan>),
}

#[derive(Debug)]
pub struct App {
    pub config: InstallConfig,
    pub disks: Vec<Disk>,
    /// Why no disks are listed, when that is the case.
    pub disk_notice: Option<String>,
    /// Detected hardware; `None` where detection is not possible.
    pub inventory: Option<Inventory>,
    pub screen_index: usize,
    pub field_index: usize,
    /// First visible line on screens without fields.
    pub scroll: usize,
    pub confirm_quit: bool,
    pub message: Option<String>,
}

impl App {
    pub fn new(
        disks: Vec<Disk>,
        disk_notice: Option<String>,
        inventory: Option<Inventory>,
    ) -> Self {
        Self {
            config: InstallConfig::default(),
            disks,
            disk_notice,
            inventory,
            screen_index: 0,
            field_index: 0,
            scroll: 0,
            confirm_quit: false,
            message: None,
        }
    }

    pub fn screen(&self) -> Screen {
        Screen::ALL[self.screen_index]
    }

    pub fn field(&self) -> Option<Field> {
        self.screen().fields().get(self.field_index).copied()
    }

    pub fn run(&mut self, terminal: &mut DefaultTerminal) -> io::Result<Outcome> {
        loop {
            terminal.draw(|frame| ui::draw(frame, self))?;
            if let Event::Key(key) = event::read()?
                // Windows also reports key releases.
                && key.kind != KeyEventKind::Release
                && let Some(outcome) = self.handle(key)
            {
                return Ok(outcome);
            }
        }
    }

    pub fn handle(&mut self, key: KeyEvent) -> Option<Outcome> {
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
            return Some(Outcome::Quit);
        }
        if self.confirm_quit {
            self.confirm_quit = false;
            return matches!(key.code, KeyCode::Char('y' | 'Y')).then_some(Outcome::Quit);
        }
        self.message = None;
        match key.code {
            KeyCode::Esc => self.confirm_quit = true,
            KeyCode::PageDown | KeyCode::F(3) => self.move_screen(true),
            KeyCode::PageUp | KeyCode::F(2) => self.move_screen(false),
            KeyCode::Down | KeyCode::Tab => self.move_field(true),
            KeyCode::Up | KeyCode::BackTab => self.move_field(false),
            KeyCode::Enter => return self.enter(),
            _ => self.edit(key.code),
        }
        None
    }

    /// Enter moves to the next field, then the next screen; on the review
    /// screen it saves the plan.
    fn enter(&mut self) -> Option<Outcome> {
        if self.screen() == Screen::Review {
            return match Plan::build(&self.config) {
                Ok(plan) => Some(Outcome::SavePlan(Box::new(plan))),
                Err(problems) => {
                    self.message = Some(format!("{} problems must be fixed", problems.len()));
                    None
                }
            };
        }
        if self.field_index + 1 < self.screen().fields().len() {
            self.field_index += 1;
        } else {
            self.move_screen(true);
        }
        None
    }

    fn edit(&mut self, code: KeyCode) {
        let Some(field) = self.field() else { return };
        match code {
            KeyCode::Left => field.cycle(&mut self.config, &self.disks, false),
            KeyCode::Right => field.cycle(&mut self.config, &self.disks, true),
            KeyCode::Char(' ') if !field.is_text() => {
                field.cycle(&mut self.config, &self.disks, true);
            }
            KeyCode::Char(c) => field.insert(&mut self.config, c),
            KeyCode::Backspace => field.backspace(&mut self.config),
            _ => {}
        }
    }

    fn move_screen(&mut self, forward: bool) {
        let last = Screen::ALL.len() - 1;
        self.screen_index = if forward {
            (self.screen_index + 1).min(last)
        } else {
            self.screen_index.saturating_sub(1)
        };
        self.field_index = 0;
        self.scroll = 0;
    }

    fn move_field(&mut self, forward: bool) {
        let count = self.screen().fields().len();
        if count > 0 {
            let next = if forward {
                self.field_index + 1
            } else {
                self.field_index + count - 1
            };
            self.field_index = next % count;
        } else {
            self.scroll = if forward {
                self.scroll + 1
            } else {
                self.scroll.saturating_sub(1)
            };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app() -> App {
        let disk = Disk {
            name: "ada0".into(),
            size_bytes: 64 << 30,
            description: "Test".into(),
            in_use: false,
        };
        App::new(vec![disk], None, None)
    }

    fn press(app: &mut App, code: KeyCode) -> Option<Outcome> {
        app.handle(KeyEvent::new(code, KeyModifiers::NONE))
    }

    fn type_text(app: &mut App, text: &str) {
        for c in text.chars() {
            assert!(press(app, KeyCode::Char(c)).is_none());
        }
    }

    #[test]
    fn complete_walkthrough_produces_a_plan() {
        let mut app = app();
        while app.screen() != Screen::Disk {
            press(&mut app, KeyCode::PageDown);
        }
        press(&mut app, KeyCode::Right);
        while app.screen() != Screen::User {
            press(&mut app, KeyCode::PageDown);
        }
        type_text(&mut app, "alice");
        while app.screen() != Screen::Review {
            press(&mut app, KeyCode::Enter);
        }
        let Some(Outcome::SavePlan(plan)) = press(&mut app, KeyCode::Enter) else {
            panic!("no plan: {:?}", app.config.problems());
        };
        assert_eq!(plan.configuration.disk.as_deref(), Some("ada0"));
        assert_eq!(plan.configuration.user.name, "alice");
    }

    #[test]
    fn no_disk_is_selected_by_default() {
        let mut app = app();
        app.screen_index = Screen::ALL.len() - 1;
        assert!(press(&mut app, KeyCode::Enter).is_none());
        assert!(app.message.is_some());
    }

    #[test]
    fn quitting_needs_confirmation() {
        let mut app = app();
        press(&mut app, KeyCode::Esc);
        assert!(app.confirm_quit);
        assert!(press(&mut app, KeyCode::Char('n')).is_none());
        assert!(!app.confirm_quit);
        press(&mut app, KeyCode::Esc);
        assert!(matches!(
            press(&mut app, KeyCode::Char('y')),
            Some(Outcome::Quit)
        ));
    }

    #[test]
    fn spaces_can_be_typed_in_text_fields() {
        let mut app = app();
        while app.screen() != Screen::User {
            press(&mut app, KeyCode::PageDown);
        }
        press(&mut app, KeyCode::Down);
        type_text(&mut app, "Alice Example");
        assert_eq!(app.config.user.full_name, "Alice Example");
    }

    #[test]
    fn navigation_stops_at_both_ends() {
        let mut app = app();
        press(&mut app, KeyCode::PageUp);
        assert_eq!(app.screen(), Screen::Welcome);
        for _ in 0..40 {
            press(&mut app, KeyCode::PageDown);
        }
        assert_eq!(app.screen(), Screen::Review);
    }
}
